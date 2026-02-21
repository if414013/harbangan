//! Core account manager for multi-account load balancing.
//!
//! Manages a pool of Google OAuth accounts, tracks their health,
//! rate limits, and delegates account selection to a pluggable strategy.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use tokio::sync::RwLock;

use super::auth::AntigravityTokenManager;
use super::strategies::health_tracker::HealthTracker;
use super::strategies::quota_tracker::QuotaTracker;
use super::strategies::token_bucket::TokenBucket;
use super::strategies::{create_strategy, SelectionStrategy, StrategyKind};
use super::account_storage::{self, StoredAccount};

// === Constants ===

/// Dedup window to prevent thundering herd on rate limits.
const DEDUP_WINDOW: Duration = Duration::from_secs(2);

/// Extended cooldown after N consecutive failures.
const EXTENDED_COOLDOWN: Duration = Duration::from_secs(60);

/// Number of consecutive failures before extended cooldown.
const EXTENDED_COOLDOWN_THRESHOLD: u32 = 3;

/// Capacity backoff tiers (seconds) for progressive rate limit backoff.
const CAPACITY_BACKOFF_TIERS: &[u64] = &[5, 10, 20, 30, 60];

// === Account State ===

/// Runtime state for a single account.
pub struct AccountState {
    /// Account email address.
    pub email: String,
    /// Composite refresh token (refreshToken|projectId|managedProjectId).
    pub composite_refresh_token: String,
    /// Health score tracker.
    pub health: HealthTracker,
    /// Token bucket for rate limiting.
    pub token_bucket: TokenBucket,
    /// Quota awareness tracker.
    pub quota: QuotaTracker,
    /// Per-model rate limit expiry times.
    pub rate_limits: HashMap<String, Instant>,
    /// Number of consecutive failures.
    pub consecutive_failures: u32,
    /// Last time this account was used.
    pub last_used: Option<Instant>,
    /// Whether this account has been marked invalid.
    pub is_invalid: bool,
    /// Reason for invalidation (if any).
    pub invalid_reason: Option<String>,
    /// When the account was added.
    pub added_at: chrono::DateTime<chrono::Utc>,
}

impl AccountState {
    /// Creates a new account state from credentials.
    pub fn new(email: String, composite_refresh_token: String) -> Self {
        Self {
            email,
            composite_refresh_token,
            health: HealthTracker::new(),
            token_bucket: TokenBucket::new(),
            quota: QuotaTracker::new(),
            rate_limits: HashMap::new(),
            consecutive_failures: 0,
            last_used: None,
            is_invalid: false,
            invalid_reason: None,
            added_at: chrono::Utc::now(),
        }
    }

    /// Returns true if this account is available for the given model.
    pub fn is_available(&self, model: &str) -> bool {
        if self.is_invalid {
            return false;
        }
        if !self.health.is_usable() {
            return false;
        }
        if self.is_rate_limited(model) {
            return false;
        }
        true
    }

    /// Returns true if this account is rate-limited for the given model.
    pub fn is_rate_limited(&self, model: &str) -> bool {
        if let Some(expiry) = self.rate_limits.get(model) {
            if Instant::now() < *expiry {
                return true;
            }
        }
        // Also check the wildcard "*" rate limit
        if let Some(expiry) = self.rate_limits.get("*") {
            if Instant::now() < *expiry {
                return true;
            }
        }
        false
    }

    /// Returns the time until the rate limit expires for a model (0 if not limited).
    pub fn rate_limit_remaining_ms(&self, model: &str) -> u64 {
        let mut max_remaining = Duration::ZERO;

        if let Some(expiry) = self.rate_limits.get(model) {
            if let Some(remaining) = expiry.checked_duration_since(Instant::now()) {
                max_remaining = max_remaining.max(remaining);
            }
        }
        if let Some(expiry) = self.rate_limits.get("*") {
            if let Some(remaining) = expiry.checked_duration_since(Instant::now()) {
                max_remaining = max_remaining.max(remaining);
            }
        }

        max_remaining.as_millis() as u64
    }
}

impl std::fmt::Debug for AccountState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountState")
            .field("email", &self.email)
            .field("composite_refresh_token", &"[REDACTED]")
            .field("health", &self.health)
            .field("token_bucket", &self.token_bucket)
            .field("quota", &self.quota)
            .field("rate_limits", &self.rate_limits)
            .field("consecutive_failures", &self.consecutive_failures)
            .field("last_used", &self.last_used)
            .field("is_invalid", &self.is_invalid)
            .field("invalid_reason", &self.invalid_reason)
            .field("added_at", &self.added_at)
            .finish()
    }
}

// === Account Manager ===

/// Multi-account manager with pluggable load balancing.
pub struct AccountManager {
    /// Account states keyed by email.
    accounts: Arc<DashMap<String, AccountState>>,
    /// Token manager for OAuth token refresh.
    token_manager: Arc<AntigravityTokenManager>,
    /// Account selection strategy.
    strategy: Arc<RwLock<Box<dyn SelectionStrategy>>>,
    /// Last rate limit dedup timestamp per (email, model).
    dedup_tracker: DashMap<(String, String), Instant>,
    /// Path to the accounts JSON file.
    storage_path: Option<std::path::PathBuf>,
}

impl AccountManager {
    /// Creates a new account manager with the given strategy.
    pub fn new(strategy_kind: StrategyKind) -> Self {
        Self {
            accounts: Arc::new(DashMap::new()),
            token_manager: Arc::new(AntigravityTokenManager::with_default_ttl()),
            strategy: Arc::new(RwLock::new(create_strategy(strategy_kind))),
            dedup_tracker: DashMap::new(),
            storage_path: account_storage::default_storage_path(),
        }
    }

    /// Creates a manager and loads accounts from persistent storage.
    pub fn load(strategy_kind: StrategyKind) -> Self {
        let mgr = Self::new(strategy_kind);

        if let Some(ref path) = mgr.storage_path {
            match account_storage::load_accounts(path) {
                Ok(stored) => {
                    for sa in stored {
                        let state =
                            AccountState::new(sa.email.clone(), sa.composite_refresh_token.clone());
                        mgr.accounts.insert(sa.email, state);
                    }
                    tracing::info!(count = mgr.accounts.len(), "Loaded accounts from storage");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to load accounts from storage");
                }
            }
        }

        mgr
    }

    // === Account CRUD ===

    /// Adds a new account. Returns error if already exists.
    pub fn add_account(
        &self,
        email: String,
        composite_refresh_token: String,
    ) -> anyhow::Result<()> {
        let state = AccountState::new(email.clone(), composite_refresh_token);
        match self.accounts.entry(email) {
            Entry::Occupied(o) => {
                anyhow::bail!("Account already exists: {}", o.key());
            }
            Entry::Vacant(v) => {
                v.insert(state);
            }
        }
        self.save_to_storage();

        tracing::info!(count = self.accounts.len(), "Account added");
        Ok(())
    }

    /// Removes an account by email.
    pub fn remove_account(&self, email: &str) -> anyhow::Result<()> {
        if self.accounts.remove(email).is_none() {
            anyhow::bail!("Account not found: {}", email);
        }
        self.token_manager.invalidate(email);
        self.save_to_storage();

        tracing::info!(count = self.accounts.len(), "Account removed");
        Ok(())
    }

    /// Returns the number of accounts.
    pub fn get_account_count(&self) -> usize {
        self.accounts.len()
    }

    // === Account Selection ===

    /// Returns accounts available for the given model.
    pub fn get_available_accounts(&self, model: &str) -> Vec<String> {
        self.accounts
            .iter()
            .filter(|entry| entry.value().is_available(model))
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Selects the best account for a request to the given model.
    ///
    /// Returns `(email, wait_ms)` or an error if no accounts are available.
    pub async fn select_account(&self, model: &str) -> anyhow::Result<(String, u64)> {
        // Collect available accounts into a snapshot
        let available: Vec<AccountState> = self
            .accounts
            .iter()
            .filter(|entry| entry.value().is_available(model))
            .map(|entry| {
                let v = entry.value();
                // Create a lightweight snapshot for the strategy
                AccountState {
                    email: v.email.clone(),
                    composite_refresh_token: String::new(), // don't leak tokens to strategy
                    health: v.health.clone(),
                    token_bucket: v.token_bucket.clone(),
                    quota: v.quota.clone(),
                    rate_limits: HashMap::new(), // already filtered by is_available
                    consecutive_failures: v.consecutive_failures,
                    last_used: v.last_used,
                    is_invalid: false,
                    invalid_reason: None,
                    added_at: v.added_at,
                }
            })
            .collect();

        if available.is_empty() {
            anyhow::bail!("No available accounts for model '{}'", model);
        }

        let strategy = self.strategy.read().await;
        match strategy.select(&available) {
            Some(result) => {
                let email = available[result.index].email.clone();

                // Update last_used
                if let Some(mut entry) = self.accounts.get_mut(&email) {
                    entry.last_used = Some(Instant::now());
                }

                Ok((email, result.wait_ms))
            }
            None => anyhow::bail!("Strategy returned no selection for model '{}'", model),
        }
    }

    // === Token / Project Access ===

    /// Gets an access token for the given account email.
    pub async fn get_token_for_account(&self, email: &str) -> anyhow::Result<String> {
        let composite = self
            .accounts
            .get(email)
            .map(|entry| entry.composite_refresh_token.clone())
            .ok_or_else(|| anyhow::anyhow!("Account not found: {}", email))?;

        self.token_manager
            .get_access_token(email, &composite)
            .await
            .map_err(|e| anyhow::anyhow!(e).context("Failed to get access token"))
    }

    /// Gets the project ID for the given account.
    pub fn get_project_for_account(&self, email: &str) -> Option<String> {
        self.accounts.get(email).and_then(|entry| {
            AntigravityTokenManager::get_project_id(&entry.composite_refresh_token)
        })
    }

    // === Event Notifications ===

    /// Marks an account as rate-limited for a model.
    pub fn mark_rate_limited(&self, email: &str, duration_ms: u64, model: &str) {
        // Dedup check
        let dedup_key = (email.to_string(), model.to_string());
        if let Some(last) = self.dedup_tracker.get(&dedup_key) {
            if last.elapsed() < DEDUP_WINDOW {
                return;
            }
        }
        self.dedup_tracker.insert(dedup_key, Instant::now());

        if let Some(mut entry) = self.accounts.get_mut(email) {
            let duration = Duration::from_millis(duration_ms);

            // Apply backoff tier based on consecutive failures
            let tier_idx =
                (entry.consecutive_failures as usize).min(CAPACITY_BACKOFF_TIERS.len() - 1);
            let backoff_secs = CAPACITY_BACKOFF_TIERS[tier_idx];
            let backoff_duration = Duration::from_secs(backoff_secs).max(duration);
            entry
                .rate_limits
                .insert(model.to_string(), Instant::now() + backoff_duration);

            tracing::debug!(
                email = email,
                model = model,
                backoff_secs = backoff_secs,
                "Account rate-limited"
            );
        }
    }

    /// Marks an account as invalid (permanently unusable until re-added).
    pub fn mark_invalid(&self, email: &str, reason: &str) {
        if let Some(mut entry) = self.accounts.get_mut(email) {
            entry.is_invalid = true;
            entry.invalid_reason = Some(reason.to_string());
            tracing::warn!(email = email, reason = reason, "Account marked invalid");
        }
    }

    /// Notifies a successful request for an account.
    pub fn notify_success(&self, email: &str, _model: &str) {
        if let Some(mut entry) = self.accounts.get_mut(email) {
            entry.health.notify_success();
            entry.consecutive_failures = 0;
            entry.token_bucket.try_consume();
        }
    }

    /// Notifies a failure for an account.
    pub fn notify_failure(&self, email: &str, _model: &str) {
        if let Some(mut entry) = self.accounts.get_mut(email) {
            entry.health.notify_failure();
            entry.consecutive_failures += 1;

            // Extended cooldown after threshold
            if entry.consecutive_failures >= EXTENDED_COOLDOWN_THRESHOLD {
                entry
                    .rate_limits
                    .insert("*".to_string(), Instant::now() + EXTENDED_COOLDOWN);
                tracing::warn!(
                    email = email,
                    failures = entry.consecutive_failures,
                    "Extended cooldown applied"
                );
            }
        }
    }

    /// Notifies a rate-limit response for an account.
    pub fn notify_rate_limit(&self, email: &str, model: &str) {
        if let Some(mut entry) = self.accounts.get_mut(email) {
            entry.health.notify_rate_limit();
            entry.consecutive_failures += 1;
        }

        // Also apply the rate limit with default backoff
        self.mark_rate_limited(email, 5000, model);
    }

    /// Updates quota tracking for an account from response headers.
    pub fn update_quota(&self, email: &str, remaining: u64, limit: u64) {
        if let Some(mut entry) = self.accounts.get_mut(email) {
            entry.quota.update(remaining, limit);
        }
    }

    // === Queries ===

    /// Increments consecutive failure count for an account.
    pub fn increment_consecutive_failures(&self, email: &str) {
        if let Some(mut entry) = self.accounts.get_mut(email) {
            entry.consecutive_failures += 1;
        }
    }

    /// Returns the consecutive failure count for an account.
    pub fn get_consecutive_failures(&self, email: &str) -> u32 {
        self.accounts
            .get(email)
            .map(|entry| entry.consecutive_failures)
            .unwrap_or(0)
    }

    /// Returns true if all accounts are rate-limited for the given model.
    pub fn is_all_rate_limited(&self, model: &str) -> bool {
        if self.accounts.is_empty() {
            return true;
        }
        self.accounts
            .iter()
            .filter(|entry| !entry.value().is_invalid)
            .all(|entry| entry.value().is_rate_limited(model))
    }

    /// Returns the minimum wait time across all accounts for a model.
    pub fn get_min_wait_time_ms(&self, model: &str) -> u64 {
        self.accounts
            .iter()
            .filter(|entry| !entry.value().is_invalid)
            .map(|entry| entry.value().rate_limit_remaining_ms(model))
            .min()
            .unwrap_or(0)
    }

    /// Clears expired rate limits across all accounts.
    pub fn clear_expired_limits(&self) {
        let now = Instant::now();
        for mut entry in self.accounts.iter_mut() {
            entry.rate_limits.retain(|_, expiry| *expiry > now);
        }
        // Also clean dedup tracker
        self.dedup_tracker
            .retain(|_, ts| ts.elapsed() < DEDUP_WINDOW * 5);
    }

    // === Persistence ===

    /// Saves current accounts to persistent storage.
    fn save_to_storage(&self) {
        if let Some(ref path) = self.storage_path {
            let stored: Vec<StoredAccount> = self
                .accounts
                .iter()
                .map(|entry| StoredAccount {
                    email: entry.email.clone(),
                    composite_refresh_token: entry.composite_refresh_token.clone(),
                    added_at: entry.added_at,
                    last_used: entry.last_used.map(|_| chrono::Utc::now()),
                })
                .collect();

            if let Err(e) = account_storage::save_accounts(path, &stored) {
                tracing::warn!(error = %e, "Failed to save accounts to storage");
            }
        }
    }
}

impl std::fmt::Debug for AccountManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountManager")
            .field("account_count", &self.accounts.len())
            .finish()
    }
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager() -> AccountManager {
        let mut mgr = AccountManager::new(StrategyKind::RoundRobin);
        mgr.storage_path = None; // disable persistence in tests
        mgr
    }

    #[test]
    fn test_add_and_remove_account() {
        let mgr = make_manager();
        mgr.add_account("a@test.com".into(), "token_a".into())
            .unwrap();
        assert_eq!(mgr.get_account_count(), 1);

        mgr.add_account("b@test.com".into(), "token_b".into())
            .unwrap();
        assert_eq!(mgr.get_account_count(), 2);

        // Duplicate should fail
        assert!(mgr
            .add_account("a@test.com".into(), "token_a".into())
            .is_err());

        mgr.remove_account("a@test.com").unwrap();
        assert_eq!(mgr.get_account_count(), 1);

        // Remove non-existent should fail
        assert!(mgr.remove_account("nonexistent@test.com").is_err());
    }

    #[test]
    fn test_get_available_accounts() {
        let mgr = make_manager();
        mgr.add_account("a@test.com".into(), "token_a".into())
            .unwrap();
        mgr.add_account("b@test.com".into(), "token_b".into())
            .unwrap();

        let available = mgr.get_available_accounts("gemini-3-flash");
        assert_eq!(available.len(), 2);
    }

    #[test]
    fn test_mark_invalid_removes_from_available() {
        let mgr = make_manager();
        mgr.add_account("a@test.com".into(), "token_a".into())
            .unwrap();
        mgr.add_account("b@test.com".into(), "token_b".into())
            .unwrap();

        mgr.mark_invalid("a@test.com", "test reason");

        let available = mgr.get_available_accounts("gemini-3-flash");
        assert_eq!(available.len(), 1);
        assert_eq!(available[0], "b@test.com");
    }

    #[test]
    fn test_rate_limit_tracking() {
        let mgr = make_manager();
        mgr.add_account("a@test.com".into(), "token_a".into())
            .unwrap();

        // Not rate limited initially
        assert!(!mgr.is_all_rate_limited("gemini-3-flash"));

        // Mark rate limited
        mgr.mark_rate_limited("a@test.com", 5000, "gemini-3-flash");

        // Now rate limited
        assert!(mgr.is_all_rate_limited("gemini-3-flash"));
        assert!(mgr.get_min_wait_time_ms("gemini-3-flash") > 0);

        // Different model should not be rate limited
        assert!(!mgr.is_all_rate_limited("claude-sonnet-4-5"));
    }

    #[test]
    fn test_consecutive_failures() {
        let mgr = make_manager();
        mgr.add_account("a@test.com".into(), "token_a".into())
            .unwrap();

        assert_eq!(mgr.get_consecutive_failures("a@test.com"), 0);

        mgr.increment_consecutive_failures("a@test.com");
        assert_eq!(mgr.get_consecutive_failures("a@test.com"), 1);

        mgr.increment_consecutive_failures("a@test.com");
        assert_eq!(mgr.get_consecutive_failures("a@test.com"), 2);
    }

    #[test]
    fn test_notify_success_resets_failures() {
        let mgr = make_manager();
        mgr.add_account("a@test.com".into(), "token_a".into())
            .unwrap();

        mgr.notify_failure("a@test.com", "gemini-3-flash");
        mgr.notify_failure("a@test.com", "gemini-3-flash");
        assert_eq!(mgr.get_consecutive_failures("a@test.com"), 2);

        mgr.notify_success("a@test.com", "gemini-3-flash");
        assert_eq!(mgr.get_consecutive_failures("a@test.com"), 0);
    }

    #[test]
    fn test_extended_cooldown_on_consecutive_failures() {
        let mgr = make_manager();
        mgr.add_account("a@test.com".into(), "token_a".into())
            .unwrap();

        // Three failures should trigger extended cooldown
        mgr.notify_failure("a@test.com", "gemini-3-flash");
        mgr.notify_failure("a@test.com", "gemini-3-flash");
        mgr.notify_failure("a@test.com", "gemini-3-flash");

        // Should be rate limited on all models (wildcard)
        assert!(mgr.is_all_rate_limited("gemini-3-flash"));
        assert!(mgr.is_all_rate_limited("claude-sonnet-4-5"));
    }

    #[test]
    fn test_clear_expired_limits() {
        let mgr = make_manager();
        mgr.add_account("a@test.com".into(), "token_a".into())
            .unwrap();

        // Add an already-expired rate limit
        if let Some(mut entry) = mgr.accounts.get_mut("a@test.com") {
            entry.rate_limits.insert(
                "old-model".to_string(),
                Instant::now() - Duration::from_secs(1),
            );
        }

        mgr.clear_expired_limits();

        // The expired limit should be gone
        if let Some(entry) = mgr.accounts.get("a@test.com") {
            assert!(entry.rate_limits.is_empty());
        };
    }

    #[test]
    fn test_get_project_for_account() {
        let mgr = make_manager();
        mgr.add_account("a@test.com".into(), "refresh|proj1|managed1".into())
            .unwrap();

        assert_eq!(
            mgr.get_project_for_account("a@test.com"),
            Some("managed1".into())
        );
        assert_eq!(mgr.get_project_for_account("nonexistent@test.com"), None);
    }

    #[test]
    fn test_update_quota() {
        let mgr = make_manager();
        mgr.add_account("a@test.com".into(), "token_a".into())
            .unwrap();

        mgr.update_quota("a@test.com", 50, 100);

        if let Some(entry) = mgr.accounts.get("a@test.com") {
            assert!((entry.quota.fill_ratio() - 0.5).abs() < f64::EPSILON);
        };
    }

    #[test]
    fn test_is_all_rate_limited_empty() {
        let mgr = make_manager();
        assert!(mgr.is_all_rate_limited("any-model"));
    }

    #[tokio::test]
    async fn test_select_account_no_accounts() {
        let mgr = make_manager();
        let result = mgr.select_account("gemini-3-flash").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_select_account_basic() {
        let mgr = make_manager();
        mgr.add_account("a@test.com".into(), "token_a".into())
            .unwrap();

        let (email, _wait) = mgr.select_account("gemini-3-flash").await.unwrap();
        assert_eq!(email, "a@test.com");
    }

    #[test]
    fn test_account_state_is_available() {
        let state = AccountState::new("test@test.com".into(), "token".into());
        assert!(state.is_available("any-model"));
        assert!(!state.is_rate_limited("any-model"));
    }

    #[test]
    fn test_account_state_rate_limited() {
        let mut state = AccountState::new("test@test.com".into(), "token".into());
        state.rate_limits.insert(
            "model-a".to_string(),
            Instant::now() + Duration::from_secs(60),
        );

        assert!(state.is_rate_limited("model-a"));
        assert!(!state.is_rate_limited("model-b"));
        assert!(state.rate_limit_remaining_ms("model-a") > 0);
    }

    #[test]
    fn test_account_state_wildcard_rate_limit() {
        let mut state = AccountState::new("test@test.com".into(), "token".into());
        state
            .rate_limits
            .insert("*".to_string(), Instant::now() + Duration::from_secs(60));

        // Wildcard should block all models
        assert!(state.is_rate_limited("any-model"));
        assert!(state.is_rate_limited("another-model"));
    }
}
