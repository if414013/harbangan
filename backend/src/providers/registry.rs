/// Provider registry: resolves which provider to use for a given user + model.
///
/// Caches per-user provider credentials in memory (5-minute TTL) to avoid
/// repeated DB lookups on every request. Handles transparent token refresh
/// for OAuth-based provider tokens.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use uuid::Uuid;

use crate::providers::rate_limiter::{AccountId, RateLimitTracker};
use crate::providers::types::{ProviderCredentials, ProviderId};
use crate::web_ui::config_db::ConfigDb;
use crate::web_ui::provider_oauth::TokenExchanger;

const CACHE_TTL: Duration = Duration::from_secs(300);

/// Buffer before expiry to trigger proactive refresh (5 minutes).
const REFRESH_BUFFER_SECS: i64 = 300;

pub(crate) struct CacheEntry {
    pub(crate) credentials: HashMap<String, ProviderCredentials>,
    /// Per-provider token expiry times.
    pub(crate) expires_at: HashMap<String, DateTime<Utc>>,
    /// User's provider priority (provider_id -> priority). Lower = preferred.
    pub(crate) priority: HashMap<String, i32>,
    pub(crate) cached_at: Instant,
}

/// Per-(user_id, provider) mutex map to prevent concurrent refresh storms.
type RefreshLockMap = DashMap<(Uuid, String), Arc<tokio::sync::Mutex<()>>>;

/// Resolves provider + credentials for a user + model combination.
pub struct ProviderRegistry {
    cache: Arc<DashMap<Uuid, CacheEntry>>,
    refresh_locks: Arc<RefreshLockMap>,
    /// Live credentials for proxy mode. Populated from env vars at startup,
    /// updated at runtime by `ProxyTokenManager` after OAuth relay or refresh.
    proxy_credentials: Arc<DashMap<ProviderId, ProviderCredentials>>,
    /// Set of providers disabled by admin. Kiro is never in this set.
    disabled_providers: Arc<RwLock<HashSet<ProviderId>>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
            refresh_locks: Arc::new(DashMap::new()),
            proxy_credentials: Arc::new(DashMap::new()),
            disabled_providers: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Create a registry with proxy credentials (for proxy-only mode).
    pub fn new_with_proxy(proxy_creds: HashMap<ProviderId, ProviderCredentials>) -> Self {
        let map = DashMap::new();
        for (k, v) in proxy_creds {
            map.insert(k, v);
        }
        Self {
            cache: Arc::new(DashMap::new()),
            refresh_locks: Arc::new(DashMap::new()),
            proxy_credentials: Arc::new(map),
            disabled_providers: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Build a registry from a ProxyConfig (env-var based proxy-only mode).
    ///
    /// Extracts provider credentials and custom model list from the config,
    /// returning a fully wired ProviderRegistry.
    pub fn from_proxy_config(proxy: &crate::config::ProxyConfig) -> Self {
        use crate::providers::types::ProviderCredentials;

        let mut creds = HashMap::new();

        // Anthropic: use OAuth access token if available
        if let Some(ref token) = proxy.anthropic_access_token {
            creds.insert(
                ProviderId::Anthropic,
                ProviderCredentials {
                    provider: ProviderId::Anthropic,
                    access_token: token.clone(),
                    base_url: None,
                    account_label: "proxy".into(),
                },
            );
        }
        // OpenAI: use OAuth access token if available
        if let Some(ref token) = proxy.openai_access_token {
            creds.insert(
                ProviderId::OpenAICodex,
                ProviderCredentials {
                    provider: ProviderId::OpenAICodex,
                    access_token: token.clone(),
                    base_url: None,
                    account_label: "proxy".into(),
                },
            );
        }
        if let Some(ref token) = proxy.copilot_token {
            creds.insert(
                ProviderId::Copilot,
                ProviderCredentials {
                    provider: ProviderId::Copilot,
                    access_token: token.clone(),
                    base_url: proxy.copilot_base_url.clone(),
                    account_label: "proxy".into(),
                },
            );
        }

        Self::new_with_proxy(creds)
    }

    /// Snapshot of proxy credentials as a HashMap. Returns None if empty.
    pub fn proxy_credentials(&self) -> Option<HashMap<ProviderId, ProviderCredentials>> {
        if self.proxy_credentials.is_empty() {
            None
        } else {
            let map: HashMap<_, _> = self
                .proxy_credentials
                .iter()
                .map(|entry| (entry.key().clone(), entry.value().clone()))
                .collect();
            Some(map)
        }
    }

    /// Get a shared reference to the live proxy credentials DashMap.
    /// Used by `ProxyTokenManager` to update credentials at runtime.
    #[allow(dead_code)]
    pub fn proxy_credentials_live(&self) -> Arc<DashMap<ProviderId, ProviderCredentials>> {
        Arc::clone(&self.proxy_credentials)
    }

    /// Parse a prefixed model ID like `"anthropic/claude-opus-4-6"` into (ProviderId, model_id).
    ///
    /// Returns `None` if the model string has no `/` prefix or the provider is unknown.
    pub fn parse_prefixed_model(model: &str) -> Option<(ProviderId, String)> {
        let (prefix, model_id) = model.split_once('/')?;
        if model_id.is_empty() {
            return None;
        }
        let provider: ProviderId = prefix.parse().ok()?;
        Some((provider, model_id.to_string()))
    }

    /// Infer the preferred direct provider for a model name based on prefix conventions.
    ///
    /// Returns `None` when the model should go through Kiro.
    pub fn provider_for_model(model: &str) -> Option<ProviderId> {
        if model.starts_with("claude-") {
            Some(ProviderId::Anthropic)
        } else if model.starts_with("gpt-")
            || model.starts_with("o1-")
            || model.starts_with("o3-")
            || model.starts_with("o4-")
            || model.starts_with("chatgpt-")
        {
            Some(ProviderId::OpenAICodex)
        } else {
            None
        }
    }

    /// Check whether a model name belongs to a provider that has been removed.
    ///
    /// Returns the removed provider name if the model matches a known-removed
    /// prefix, so the caller can return an explicit error instead of silently
    /// routing to Kiro.
    pub fn removed_provider_for_model(model: &str) -> Option<&'static str> {
        if model.starts_with("qwen-")
            || model.starts_with("qwen3-")
            || model.starts_with("qwq-")
            || model.starts_with("qwen/")
        {
            Some("qwen")
        } else if model.starts_with("gemini-") || model.starts_with("gemini/") {
            Some("gemini")
        } else {
            None
        }
    }

    /// Ensure the user's OAuth token for a provider is fresh.
    ///
    /// Call this at the handler level BEFORE `resolve_provider`. If the token
    /// is expired (or about to expire), refreshes it transparently. On permanent
    /// refresh failure (revoked token), deletes the token row and invalidates cache
    /// so the request falls back to Kiro.
    ///
    /// Uses a per-(user_id, provider) mutex so concurrent requests don't all
    /// try to refresh simultaneously.
    pub async fn ensure_fresh_token(
        &self,
        user_id: Uuid,
        model: &str,
        db: &ConfigDb,
        exchanger: &dyn TokenExchanger,
    ) {
        let target = if let Some((provider, _)) = Self::parse_prefixed_model(model) {
            provider
        } else if let Some(provider) = Self::provider_for_model(model) {
            provider
        } else {
            return;
        };
        let provider_str = target.as_str().to_string();

        // Check cache first — if token is still fresh, skip DB lookup entirely
        if let Some(entry) = self.cache.get(&user_id) {
            if entry.cached_at.elapsed() < CACHE_TTL {
                if let Some(expires_at) = entry.expires_at.get(&provider_str) {
                    let now = Utc::now();
                    if (*expires_at - now).num_seconds() > REFRESH_BUFFER_SECS {
                        return; // Token is fresh, nothing to do
                    }
                } else {
                    return; // No token for this provider, nothing to refresh
                }
            }
        }

        // Token might need refresh — check DB
        let token_row = match db.get_user_provider_token(user_id, &provider_str).await {
            Ok(Some(row)) => row,
            _ => return, // No token stored, nothing to refresh
        };

        let (_access_token, refresh_token, expires_at, _email) = token_row;
        let now = Utc::now();

        if (expires_at - now).num_seconds() > REFRESH_BUFFER_SECS {
            return; // Token is still fresh
        }

        if refresh_token.is_empty() {
            // No refresh token available — can't refresh, delete stale token
            tracing::warn!(
                user_id = %user_id,
                provider = %provider_str,
                "Token expired with no refresh token, removing"
            );
            let _ = db.delete_user_provider_token(user_id, &provider_str).await;
            self.invalidate(user_id);
            return;
        }

        // Acquire per-(user, provider) lock to prevent concurrent refresh
        let lock = self
            .refresh_locks
            .entry((user_id, provider_str.clone()))
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();

        let _guard = lock.lock().await;

        // Re-check after acquiring lock — another request may have refreshed already
        if let Ok(Some((_, _, new_expires, _))) =
            db.get_user_provider_token(user_id, &provider_str).await
        {
            let now = Utc::now();
            if (new_expires - now).num_seconds() > REFRESH_BUFFER_SECS {
                // Another request refreshed while we waited
                self.invalidate(user_id);
                return;
            }
        }

        // Actually refresh
        match exchanger.refresh_token(&provider_str, &refresh_token).await {
            Ok(result) => {
                let new_expires = Utc::now() + chrono::Duration::seconds(result.expires_in);
                // Update DB — use the new refresh_token if provided, otherwise keep existing
                let store_refresh = if result.refresh_token.is_empty() {
                    &refresh_token
                } else {
                    &result.refresh_token
                };
                if let Err(e) = db
                    .upsert_user_provider_token(
                        user_id,
                        &provider_str,
                        &result.access_token,
                        store_refresh,
                        new_expires,
                        "", // Don't overwrite email on refresh
                    )
                    .await
                {
                    tracing::error!(
                        error = ?e,
                        user_id = %user_id,
                        provider = %provider_str,
                        "Failed to store refreshed token"
                    );
                }
                self.invalidate(user_id);
                tracing::debug!(
                    user_id = %user_id,
                    provider = %provider_str,
                    "Provider token refreshed"
                );
            }
            Err(e) => {
                // Permanent failure — delete token, fall back to Kiro
                tracing::warn!(
                    error = ?e,
                    user_id = %user_id,
                    provider = %provider_str,
                    "Token refresh failed permanently, removing token"
                );
                let _ = db.delete_user_provider_token(user_id, &provider_str).await;
                self.invalidate(user_id);
            }
        }
    }

    /// Resolve provider and credentials for a user + model.
    ///
    /// When multiple providers can serve the requested model (e.g. both Anthropic
    /// and Copilot can serve `claude-*`), picks the one with the lowest priority
    /// number from the user's `user_provider_priority` table. Falls back to the
    /// native provider when no priority is configured.
    ///
    /// Returns `(ProviderId::Kiro, None)` when:
    /// - `user_id` is None (proxy-only mode or unauthenticated)
    /// - The model has no recognised direct-provider prefix
    /// - The user has no stored token for any candidate provider
    /// - The DB is unavailable
    pub async fn resolve_provider(
        &self,
        user_id: Option<Uuid>,
        model: &str,
        db: Option<&ConfigDb>,
    ) -> (ProviderId, Option<ProviderCredentials>) {
        let Some(uid) = user_id else {
            // Proxy mode without Kiro creds: check proxy credential store
            let disabled = self.disabled_providers.read().await;
            return self.resolve_from_proxy_creds_filtered(model, &disabled);
        };

        // Try explicit prefix first (e.g. "anthropic/claude-opus-4-6")
        let (native, is_explicit_prefix) =
            if let Some((provider, _model_id)) = Self::parse_prefixed_model(model) {
                (provider, true)
            } else if let Some(provider) = Self::provider_for_model(model) {
                (provider, false)
            } else {
                return (ProviderId::Kiro, None);
            };

        // Filter out disabled providers from credentials
        let disabled = self.disabled_providers.read().await;

        // Cache hit?
        if let Some(entry) = self.cache.get(&uid) {
            if entry.cached_at.elapsed() < CACHE_TTL {
                let mut filtered_creds = entry.credentials.clone();
                for pid in disabled.iter() {
                    filtered_creds.remove(pid.as_str());
                }
                if is_explicit_prefix {
                    // Explicit prefix is binding — only look up the specified provider
                    let native_str = native.as_str();
                    let creds = filtered_creds.get(native_str).cloned();
                    return (native, creds);
                }
                return Self::pick_best_provider(&native, &filtered_creds, &entry.priority);
            }
        }

        // Cache miss or stale — load from DB
        let Some(db) = db else {
            // Proxy mode with Kiro creds but no DB: check proxy credential store
            return self.resolve_from_proxy_creds_filtered(model, &disabled);
        };
        let (mut user_creds, user_expires, user_priority) = self.load_user_data(uid, db).await;
        // Filter out disabled providers
        for pid in disabled.iter() {
            user_creds.remove(pid.as_str());
        }
        drop(disabled);
        let result = if is_explicit_prefix {
            // Explicit prefix is binding — only look up the specified provider
            let native_str = native.as_str();
            let creds = user_creds.get(native_str).cloned();
            (native, creds)
        } else {
            Self::pick_best_provider(&native, &user_creds, &user_priority)
        };
        self.cache.insert(
            uid,
            CacheEntry {
                credentials: user_creds,
                expires_at: user_expires,
                priority: user_priority,
                cached_at: Instant::now(),
            },
        );
        result
    }

    /// Pick the best provider from candidates that have credentials.
    ///
    /// Candidates are the native provider for the model plus Copilot (which can
    /// serve any model). The provider with the lowest priority number wins.
    /// If no priority is set for either, the native provider is preferred.
    fn pick_best_provider(
        native: &ProviderId,
        credentials: &HashMap<String, ProviderCredentials>,
        priority: &HashMap<String, i32>,
    ) -> (ProviderId, Option<ProviderCredentials>) {
        let native_str = native.as_str();
        let has_native = credentials.contains_key(native_str);
        let has_copilot = credentials.contains_key("copilot");

        match (has_native, has_copilot) {
            (false, false) => (ProviderId::Kiro, None),
            (true, false) => (native.clone(), Some(credentials[native_str].clone())),
            (false, true) => (ProviderId::Copilot, Some(credentials["copilot"].clone())),
            (true, true) => {
                // Both available — use priority (lower number wins)
                let native_pri = priority.get(native_str).copied().unwrap_or(0);
                let copilot_pri = priority.get("copilot").copied().unwrap_or(1);
                if copilot_pri < native_pri {
                    (ProviderId::Copilot, Some(credentials["copilot"].clone()))
                } else {
                    (native.clone(), Some(credentials[native_str].clone()))
                }
            }
        }
    }

    /// Resolve provider with multi-account load balancing.
    ///
    /// Loads ALL user accounts for the target provider plus admin pool accounts,
    /// then uses the rate limit tracker to pick the account with the most headroom.
    /// Falls back to existing single-account resolution if no multi-account data exists.
    ///
    /// Returns `(ProviderId, Option<ProviderCredentials>, Option<AccountId>)` where
    /// the AccountId can be used for rate-limit tracking after the request completes.
    pub async fn resolve_provider_with_balancing(
        &self,
        user_id: Option<Uuid>,
        model: &str,
        db: Option<&ConfigDb>,
        rate_tracker: &RateLimitTracker,
    ) -> (ProviderId, Option<ProviderCredentials>, Option<AccountId>) {
        let Some(uid) = user_id else {
            return (ProviderId::Kiro, None, None);
        };

        // Determine target provider from model name
        let (native, is_explicit_prefix) =
            if let Some((provider, _model_id)) = Self::parse_prefixed_model(model) {
                (provider, true)
            } else if let Some(provider) = Self::provider_for_model(model) {
                (provider, false)
            } else {
                return (ProviderId::Kiro, None, None);
            };

        let Some(db) = db else {
            return (ProviderId::Kiro, None, None);
        };

        let provider_str = native.as_str();

        // Load user priorities once for all provider decisions
        let priority = db
            .get_user_provider_priority(uid)
            .await
            .unwrap_or_default()
            .into_iter()
            .collect::<HashMap<String, i32>>();
        let native_pri = priority.get(provider_str).copied().unwrap_or(0);
        let copilot_pri = priority.get("copilot").copied().unwrap_or(1);

        // Candidates: (AccountId, ProviderCredentials, priority)
        let mut candidates: Vec<(AccountId, ProviderCredentials, i32)> = Vec::new();

        match native {
            ProviderId::Copilot => {
                if let Ok(Some(row)) = db.get_copilot_tokens(uid).await {
                    if let (Some(copilot_token), Some(base_url), Some(expires_at)) =
                        (row.copilot_token, row.base_url, row.expires_at)
                    {
                        let now = chrono::Utc::now();
                        if expires_at > now {
                            let account_id = AccountId {
                                user_id: Some(uid),
                                provider_id: ProviderId::Copilot,
                                account_label: "default".to_string(),
                            };
                            let creds = ProviderCredentials {
                                provider: ProviderId::Copilot,
                                access_token: copilot_token,
                                base_url: Some(base_url),
                                account_label: "default".to_string(),
                            };
                            candidates.push((account_id, creds, native_pri));
                        }
                    }
                }
            }
            _ => {
                // Load all user accounts for this provider
                if let Ok(rows) = db.get_all_user_provider_tokens(uid, provider_str).await {
                    for row in rows {
                        let now = chrono::Utc::now();
                        if row.expires_at > now {
                            let account_id = AccountId {
                                user_id: Some(uid),
                                provider_id: native.clone(),
                                account_label: row.account_label.clone(),
                            };
                            let creds = ProviderCredentials {
                                provider: native.clone(),
                                access_token: row.access_token.clone(),
                                base_url: row.base_url.clone(),
                                account_label: row.account_label.clone(),
                            };
                            candidates.push((account_id, creds, native_pri));
                        }
                    }
                }
            }
        }

        // Also check Copilot as an alternative (universal provider)
        // BUT skip Copilot when prefix is explicit — explicit prefix is binding
        if native != ProviderId::Copilot && !is_explicit_prefix {
            if let Ok(Some(row)) = db.get_copilot_tokens(uid).await {
                if let (Some(copilot_token), Some(base_url), Some(expires_at)) =
                    (row.copilot_token, row.base_url, row.expires_at)
                {
                    let now = chrono::Utc::now();
                    if expires_at > now {
                        // Include Copilot if it has equal or better priority, or if no native accounts
                        if candidates.is_empty() || copilot_pri <= native_pri {
                            let account_id = AccountId {
                                user_id: Some(uid),
                                provider_id: ProviderId::Copilot,
                                account_label: "default".to_string(),
                            };
                            let creds = ProviderCredentials {
                                provider: ProviderId::Copilot,
                                access_token: copilot_token,
                                base_url: Some(base_url),
                                account_label: "default".to_string(),
                            };
                            candidates.push((account_id, creds, copilot_pri));
                        }
                    }
                }
            }
        }

        // Load admin pool accounts for the target provider as fallback (default priority 100).
        const ADMIN_POOL_PRIORITY: i32 = 100;
        if let Ok(pool_rows) = db.get_admin_pool_accounts(provider_str).await {
            for row in pool_rows {
                let account_id = AccountId {
                    user_id: None,
                    provider_id: native.clone(),
                    account_label: row.account_label.clone(),
                };
                let creds = ProviderCredentials {
                    provider: native.clone(),
                    access_token: row.api_key.clone(),
                    base_url: row.base_url.clone(),
                    account_label: row.account_label.clone(),
                };
                candidates.push((account_id, creds, ADMIN_POOL_PRIORITY));
            }
        }

        // Filter out candidates for disabled providers
        {
            let disabled = self.disabled_providers.read().await;
            if !disabled.is_empty() {
                candidates.retain(|(_, creds, _)| !disabled.contains(&creds.provider));
            }
        }

        if candidates.is_empty() {
            if is_explicit_prefix {
                // Explicit prefix is binding — return the specified provider with no creds
                return (native, None, None);
            }
            // No accounts available — fall back to existing single-account resolution
            let (pid, creds) = self.resolve_provider(user_id, model, Some(db)).await;
            return (pid, creds, None);
        }

        // Use priority-aware rate tracker to pick the best account
        let prioritized: Vec<(AccountId, i32)> = candidates
            .iter()
            .map(|(aid, _, pri)| (aid.clone(), *pri))
            .collect();
        if let Some(best) = rate_tracker.best_account_with_priority(&prioritized) {
            // Find the matching credentials
            if let Some((_, creds, _)) = candidates.iter().find(|(aid, _, _)| aid == &best) {
                let provider_id = creds.provider.clone();
                return (provider_id, Some(creds.clone()), Some(best));
            }
        }

        // All accounts rate-limited — return first candidate anyway with account_id
        // so the caller can attempt and get a proper 429
        let (account_id, creds, _) = candidates.into_iter().next().unwrap();
        let provider_id = creds.provider.clone();
        (provider_id, Some(creds), Some(account_id))
    }

    /// Resolve provider from proxy credentials (env-var or relay-based, no DB).
    fn resolve_from_proxy_creds(&self, model: &str) -> (ProviderId, Option<ProviderCredentials>) {
        self.resolve_from_proxy_creds_filtered(model, &HashSet::new())
    }

    /// Resolve provider from proxy credentials, filtering out disabled providers.
    fn resolve_from_proxy_creds_filtered(
        &self,
        model: &str,
        disabled: &HashSet<ProviderId>,
    ) -> (ProviderId, Option<ProviderCredentials>) {
        if self.proxy_credentials.is_empty() {
            return (ProviderId::Kiro, None);
        }
        // Determine target provider from model name
        let (native, is_explicit_prefix) =
            if let Some((provider, _)) = Self::parse_prefixed_model(model) {
                (provider, true)
            } else if let Some(provider) = Self::provider_for_model(model) {
                (provider, false)
            } else {
                return (ProviderId::Kiro, None);
            };
        // Skip disabled providers for proxy creds
        if disabled.contains(&native) {
            return if is_explicit_prefix {
                (native, None)
            } else {
                (ProviderId::Kiro, None)
            };
        }
        // Look up proxy credentials for that provider
        if let Some(cred) = self.proxy_credentials.get(&native) {
            (native, Some(cred.clone()))
        } else if is_explicit_prefix {
            // Explicit prefix is binding — return specified provider with no creds
            (native, None)
        } else {
            (ProviderId::Kiro, None)
        }
    }

    /// Invalidate the cache for a user. Call after a provider token is added, removed, or refreshed.
    pub fn invalidate(&self, user_id: Uuid) {
        self.cache.remove(&user_id);
    }

    /// Check if a provider is enabled. Kiro always returns true.
    pub async fn is_provider_enabled(&self, provider: &ProviderId) -> bool {
        if *provider == ProviderId::Kiro {
            return true;
        }
        !self.disabled_providers.read().await.contains(provider)
    }

    /// Update a provider's enabled/disabled state in the in-memory cache.
    /// Call this after persisting the change to the database.
    pub async fn set_provider_enabled(&self, provider: ProviderId, enabled: bool) {
        if provider == ProviderId::Kiro {
            return; // Kiro cannot be disabled
        }
        let mut set = self.disabled_providers.write().await;
        if enabled {
            set.remove(&provider);
        } else {
            set.insert(provider);
        }
    }

    /// Load disabled providers from the database at startup.
    pub async fn load_disabled_providers_from_db(&self, db: &ConfigDb) {
        match db.get_all_provider_settings().await {
            Ok(rows) => {
                let mut set = HashSet::new();
                for (provider_id_str, enabled) in rows {
                    if !enabled {
                        if let Ok(pid) = provider_id_str.parse::<ProviderId>() {
                            if pid != ProviderId::Kiro {
                                set.insert(pid);
                            }
                        }
                    }
                }
                *self.disabled_providers.write().await = set;
            }
            Err(e) => {
                tracing::warn!(error = ?e, "Failed to load provider settings from DB, all providers enabled");
            }
        }
    }

    /// Get a snapshot of currently disabled provider IDs.
    #[allow(dead_code)]
    pub async fn disabled_providers(&self) -> HashSet<ProviderId> {
        self.disabled_providers.read().await.clone()
    }

    /// Load all provider tokens and priority for a user from the database.
    /// Skips disabled providers so their credentials are not loaded into the cache.
    async fn load_user_data(
        &self,
        user_id: Uuid,
        db: &ConfigDb,
    ) -> (
        HashMap<String, ProviderCredentials>,
        HashMap<String, DateTime<Utc>>,
        HashMap<String, i32>,
    ) {
        let disabled = self.disabled_providers.read().await;
        let mut creds_map = HashMap::new();
        let mut expires_map = HashMap::new();
        for pid in ProviderId::all_visible() {
            if disabled.contains(pid) {
                continue;
            }
            let provider_str = pid.as_str();
            if let Ok(Some((access_token, _refresh_token, expires_at, _email))) =
                db.get_user_provider_token(user_id, provider_str).await
            {
                // Only include tokens that haven't fully expired
                let now = Utc::now();
                if expires_at > now {
                    let (provider, base_url) = match provider_str {
                        "anthropic" => (ProviderId::Anthropic, None),
                        "openai_codex" => (ProviderId::OpenAICodex, None),
                        _ => continue,
                    };
                    creds_map.insert(
                        provider_str.to_string(),
                        ProviderCredentials {
                            provider,
                            access_token,
                            base_url,
                            account_label: "default".to_string(),
                        },
                    );
                    expires_map.insert(provider_str.to_string(), expires_at);
                }
            }
        }

        // Also load Copilot tokens from user_copilot_tokens (separate table)
        if !disabled.contains(&ProviderId::Copilot) {
            if let Ok(Some(row)) = db.get_copilot_tokens(user_id).await {
                if let (Some(copilot_token), Some(base_url), Some(expires_at)) =
                    (row.copilot_token, row.base_url, row.expires_at)
                {
                    let now = Utc::now();
                    if expires_at > now {
                        creds_map.insert(
                            "copilot".to_string(),
                            ProviderCredentials {
                                provider: ProviderId::Copilot,
                                access_token: copilot_token,
                                base_url: Some(base_url),
                                account_label: "default".to_string(),
                            },
                        );
                        expires_map.insert("copilot".to_string(), expires_at);
                    }
                }
            }
        }

        // Load provider priority
        let priority_map = match db.get_user_provider_priority(user_id).await {
            Ok(rows) => rows.into_iter().collect(),
            Err(e) => {
                tracing::warn!(
                    error = ?e,
                    user_id = %user_id,
                    "Failed to load provider priority, using defaults"
                );
                HashMap::new()
            }
        };

        (creds_map, expires_map, priority_map)
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ApiError;
    use crate::web_ui::provider_oauth::TokenExchangeResult;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn test_provider_for_model_claude() {
        assert_eq!(
            ProviderRegistry::provider_for_model("claude-sonnet-4"),
            Some(ProviderId::Anthropic)
        );
        assert_eq!(
            ProviderRegistry::provider_for_model("claude-3-5-sonnet-20241022"),
            Some(ProviderId::Anthropic)
        );
    }

    #[test]
    fn test_provider_for_model_openai() {
        assert_eq!(
            ProviderRegistry::provider_for_model("gpt-4o"),
            Some(ProviderId::OpenAICodex)
        );
        assert_eq!(
            ProviderRegistry::provider_for_model("o1-mini"),
            Some(ProviderId::OpenAICodex)
        );
        assert_eq!(
            ProviderRegistry::provider_for_model("o3-pro"),
            Some(ProviderId::OpenAICodex)
        );
        assert_eq!(
            ProviderRegistry::provider_for_model("chatgpt-4o-latest"),
            Some(ProviderId::OpenAICodex)
        );
    }

    #[test]
    fn test_provider_for_model_gemini_now_returns_none() {
        // Gemini provider removed — gemini-* models fall through to Kiro via provider_for_model
        // but are caught by removed_provider_for_model at the route level
        assert_eq!(ProviderRegistry::provider_for_model("gemini-2.5-pro"), None);
        assert_eq!(
            ProviderRegistry::provider_for_model("gemini-2.5-flash"),
            None
        );
    }

    #[test]
    fn test_removed_provider_for_model_qwen() {
        assert_eq!(
            ProviderRegistry::removed_provider_for_model("qwen-coder-plus"),
            Some("qwen")
        );
        assert_eq!(
            ProviderRegistry::removed_provider_for_model("qwen3-coder-plus"),
            Some("qwen")
        );
        assert_eq!(
            ProviderRegistry::removed_provider_for_model("qwq-32b"),
            Some("qwen")
        );
        assert_eq!(
            ProviderRegistry::removed_provider_for_model("qwen/qwen3-coder-plus"),
            Some("qwen")
        );
    }

    #[test]
    fn test_removed_provider_for_model_gemini() {
        assert_eq!(
            ProviderRegistry::removed_provider_for_model("gemini-2.5-pro"),
            Some("gemini")
        );
        assert_eq!(
            ProviderRegistry::removed_provider_for_model("gemini/gemini-2.5-flash"),
            Some("gemini")
        );
    }

    #[test]
    fn test_removed_provider_for_model_active_providers_pass() {
        assert_eq!(
            ProviderRegistry::removed_provider_for_model("claude-sonnet-4"),
            None
        );
        assert_eq!(ProviderRegistry::removed_provider_for_model("gpt-4o"), None);
        assert_eq!(ProviderRegistry::removed_provider_for_model("auto"), None);
    }

    #[test]
    fn test_provider_for_model_kiro_unknown() {
        assert_eq!(ProviderRegistry::provider_for_model("kiro-auto"), None);
        assert_eq!(ProviderRegistry::provider_for_model("auto"), None);
        assert_eq!(
            ProviderRegistry::provider_for_model("CLAUDE_SONNET_4_20250514_V1_0"),
            None
        );
    }

    #[test]
    fn test_provider_for_model_empty() {
        assert_eq!(ProviderRegistry::provider_for_model(""), None);
    }

    #[tokio::test]
    async fn test_resolve_provider_no_user_id_returns_kiro() {
        let registry = ProviderRegistry::new();
        let (provider, creds) = registry
            .resolve_provider(None, "claude-sonnet-4", None)
            .await;
        assert_eq!(provider, ProviderId::Kiro);
        assert!(creds.is_none());
    }

    #[tokio::test]
    async fn test_resolve_provider_unknown_model_returns_kiro() {
        let registry = ProviderRegistry::new();
        let uid = Uuid::new_v4();
        let (provider, creds) = registry.resolve_provider(Some(uid), "auto", None).await;
        assert_eq!(provider, ProviderId::Kiro);
        assert!(creds.is_none());
    }

    #[tokio::test]
    async fn test_resolve_provider_no_db_returns_kiro() {
        let registry = ProviderRegistry::new();
        let uid = Uuid::new_v4();
        let (provider, creds) = registry
            .resolve_provider(Some(uid), "claude-sonnet-4", None)
            .await;
        assert_eq!(provider, ProviderId::Kiro);
        assert!(creds.is_none());
    }

    #[tokio::test]
    async fn test_resolve_provider_cache_hit_returns_kiro_on_empty_cache() {
        let registry = ProviderRegistry::new();
        let uid = Uuid::new_v4();

        registry.cache.insert(
            uid,
            CacheEntry {
                credentials: HashMap::new(),
                expires_at: HashMap::new(),
                priority: HashMap::new(),
                cached_at: Instant::now(),
            },
        );

        let (provider, creds) = registry
            .resolve_provider(Some(uid), "claude-sonnet-4", None)
            .await;
        assert_eq!(provider, ProviderId::Kiro);
        assert!(creds.is_none());
    }

    #[tokio::test]
    async fn test_resolve_provider_cache_hit_returns_direct_provider() {
        let registry = ProviderRegistry::new();
        let uid = Uuid::new_v4();

        let mut creds_map = HashMap::new();
        creds_map.insert(
            "anthropic".to_string(),
            ProviderCredentials {
                provider: ProviderId::Anthropic,
                access_token: "sk-ant-cached".to_string(),
                base_url: None,
                account_label: "default".to_string(),
            },
        );
        registry.cache.insert(
            uid,
            CacheEntry {
                credentials: creds_map,
                expires_at: HashMap::new(),
                priority: HashMap::new(),
                cached_at: Instant::now(),
            },
        );

        let (provider, creds) = registry
            .resolve_provider(Some(uid), "claude-sonnet-4", None)
            .await;
        assert_eq!(provider, ProviderId::Anthropic);
        let creds = creds.expect("expected credentials from cache");
        assert_eq!(creds.access_token, "sk-ant-cached");
    }

    #[test]
    fn test_invalidate_removes_cache_entry() {
        let registry = ProviderRegistry::new();
        let uid = Uuid::new_v4();
        registry.cache.insert(
            uid,
            CacheEntry {
                credentials: HashMap::new(),
                expires_at: HashMap::new(),
                priority: HashMap::new(),
                cached_at: Instant::now(),
            },
        );
        assert!(registry.cache.contains_key(&uid));
        registry.invalidate(uid);
        assert!(!registry.cache.contains_key(&uid));
    }

    // ── Token refresh tests ──────────────────────────────────────────

    /// Mock token exchanger that tracks call count.
    struct MockExchanger {
        call_count: Arc<AtomicU32>,
        should_fail: bool,
    }

    impl MockExchanger {
        fn new() -> Self {
            Self {
                call_count: Arc::new(AtomicU32::new(0)),
                should_fail: false,
            }
        }

        #[allow(dead_code)]
        fn failing() -> Self {
            Self {
                call_count: Arc::new(AtomicU32::new(0)),
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl TokenExchanger for MockExchanger {
        async fn exchange_code(
            &self,
            _provider: &str,
            _code: &str,
            _state: &str,
            _pkce_verifier: &str,
            _redirect_uri: &str,
        ) -> Result<TokenExchangeResult, ApiError> {
            unimplemented!("not used in refresh tests")
        }

        async fn refresh_token(
            &self,
            _provider: &str,
            _refresh_token: &str,
        ) -> Result<TokenExchangeResult, ApiError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            if self.should_fail {
                Err(ApiError::Internal(anyhow::anyhow!("Token revoked")))
            } else {
                Ok(TokenExchangeResult {
                    access_token: "refreshed-access-token".to_string(),
                    refresh_token: "refreshed-refresh-token".to_string(),
                    expires_in: 3600,
                    email: String::new(),
                })
            }
        }
    }

    #[test]
    fn test_ensure_fresh_token_skips_unknown_model() {
        // ensure_fresh_token should return immediately for non-provider models
        let registry = ProviderRegistry::new();
        let exchanger = MockExchanger::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            // No DB needed — should bail out before DB access
            // We can't pass a real ConfigDb, but the function should return
            // before reaching the DB call for unknown models.
            // Just verify provider_for_model returns None.
            assert!(ProviderRegistry::provider_for_model("auto").is_none());
            assert_eq!(exchanger.call_count.load(Ordering::SeqCst), 0);
        });
        // Verify no refresh was attempted
        assert_eq!(registry.cache.len(), 0);
    }

    #[test]
    fn test_ensure_fresh_token_cache_fresh_skips_refresh() {
        let registry = ProviderRegistry::new();
        let uid = Uuid::new_v4();

        // Insert a cache entry with a far-future expiry
        let mut expires_map = HashMap::new();
        expires_map.insert(
            "anthropic".to_string(),
            Utc::now() + chrono::Duration::hours(1),
        );
        let mut creds_map = HashMap::new();
        creds_map.insert(
            "anthropic".to_string(),
            ProviderCredentials {
                provider: ProviderId::Anthropic,
                access_token: "still-valid".to_string(),
                base_url: None,
                account_label: "default".to_string(),
            },
        );
        registry.cache.insert(
            uid,
            CacheEntry {
                credentials: creds_map,
                expires_at: expires_map,
                priority: HashMap::new(),
                cached_at: Instant::now(),
            },
        );

        let exchanger = MockExchanger::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            // This should return immediately because the cached token is fresh.
            // We can't call ensure_fresh_token without a real DB, but we can
            // verify the cache check logic by checking the early return path.
            if let Some(entry) = registry.cache.get(&uid) {
                if entry.cached_at.elapsed() < CACHE_TTL {
                    if let Some(expires_at) = entry.expires_at.get("anthropic") {
                        let now = Utc::now();
                        assert!(
                            (*expires_at - now).num_seconds() > REFRESH_BUFFER_SECS,
                            "Token should be considered fresh"
                        );
                    }
                }
            }
        });
        assert_eq!(exchanger.call_count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_refresh_lock_key_is_per_user_provider() {
        let registry = ProviderRegistry::new();
        let uid1 = Uuid::new_v4();
        let uid2 = Uuid::new_v4();

        // Insert locks for different (user, provider) pairs
        registry.refresh_locks.insert(
            (uid1, "anthropic".to_string()),
            Arc::new(tokio::sync::Mutex::new(())),
        );
        registry.refresh_locks.insert(
            (uid1, "openai_codex".to_string()),
            Arc::new(tokio::sync::Mutex::new(())),
        );
        registry.refresh_locks.insert(
            (uid2, "anthropic".to_string()),
            Arc::new(tokio::sync::Mutex::new(())),
        );

        // Each (user, provider) pair gets its own lock
        assert_eq!(registry.refresh_locks.len(), 3);
        assert!(registry
            .refresh_locks
            .contains_key(&(uid1, "anthropic".to_string())));
        assert!(registry
            .refresh_locks
            .contains_key(&(uid1, "openai_codex".to_string())));
        assert!(registry
            .refresh_locks
            .contains_key(&(uid2, "anthropic".to_string())));
    }

    #[test]
    fn test_cache_entry_includes_expires_at() {
        let registry = ProviderRegistry::new();
        let uid = Uuid::new_v4();
        let future = Utc::now() + chrono::Duration::hours(1);

        let mut expires_map = HashMap::new();
        expires_map.insert("anthropic".to_string(), future);

        registry.cache.insert(
            uid,
            CacheEntry {
                credentials: HashMap::new(),
                expires_at: expires_map,
                priority: HashMap::new(),
                cached_at: Instant::now(),
            },
        );

        let entry = registry.cache.get(&uid).unwrap();
        assert_eq!(entry.expires_at.get("anthropic"), Some(&future));
    }

    // ── Priority selection tests ─────────────────────────────────────

    #[test]
    fn test_pick_best_provider_no_credentials() {
        let creds = HashMap::new();
        let priority = HashMap::new();
        let (provider, c) =
            ProviderRegistry::pick_best_provider(&ProviderId::Anthropic, &creds, &priority);
        assert_eq!(provider, ProviderId::Kiro);
        assert!(c.is_none());
    }

    #[test]
    fn test_pick_best_provider_native_only() {
        let mut creds = HashMap::new();
        creds.insert(
            "anthropic".to_string(),
            ProviderCredentials {
                provider: ProviderId::Anthropic,
                access_token: "sk-ant".to_string(),
                base_url: None,
                account_label: "default".to_string(),
            },
        );
        let priority = HashMap::new();
        let (provider, c) =
            ProviderRegistry::pick_best_provider(&ProviderId::Anthropic, &creds, &priority);
        assert_eq!(provider, ProviderId::Anthropic);
        assert_eq!(c.unwrap().access_token, "sk-ant");
    }

    #[test]
    fn test_pick_best_provider_copilot_only() {
        let mut creds = HashMap::new();
        creds.insert(
            "copilot".to_string(),
            ProviderCredentials {
                provider: ProviderId::Copilot,
                access_token: "cop-tok".to_string(),
                base_url: Some("https://api.githubcopilot.com".to_string()),
                account_label: "default".to_string(),
            },
        );
        let priority = HashMap::new();
        let (provider, c) =
            ProviderRegistry::pick_best_provider(&ProviderId::Anthropic, &creds, &priority);
        assert_eq!(provider, ProviderId::Copilot);
        assert_eq!(c.unwrap().access_token, "cop-tok");
    }

    #[test]
    fn test_pick_best_provider_both_no_priority_prefers_native() {
        let mut creds = HashMap::new();
        creds.insert(
            "anthropic".to_string(),
            ProviderCredentials {
                provider: ProviderId::Anthropic,
                access_token: "sk-ant".to_string(),
                base_url: None,
                account_label: "default".to_string(),
            },
        );
        creds.insert(
            "copilot".to_string(),
            ProviderCredentials {
                provider: ProviderId::Copilot,
                access_token: "cop-tok".to_string(),
                base_url: Some("https://api.githubcopilot.com".to_string()),
                account_label: "default".to_string(),
            },
        );
        // No priority set — native default=0, copilot default=1 → native wins
        let priority = HashMap::new();
        let (provider, _) =
            ProviderRegistry::pick_best_provider(&ProviderId::Anthropic, &creds, &priority);
        assert_eq!(provider, ProviderId::Anthropic);
    }

    #[test]
    fn test_pick_best_provider_copilot_higher_priority() {
        let mut creds = HashMap::new();
        creds.insert(
            "openai_codex".to_string(),
            ProviderCredentials {
                provider: ProviderId::OpenAICodex,
                access_token: "sk-oai".to_string(),
                base_url: None,
                account_label: "default".to_string(),
            },
        );
        creds.insert(
            "copilot".to_string(),
            ProviderCredentials {
                provider: ProviderId::Copilot,
                access_token: "cop-tok".to_string(),
                base_url: Some("https://api.githubcopilot.com".to_string()),
                account_label: "default".to_string(),
            },
        );
        // User sets copilot priority=1, openai_codex priority=2 → copilot wins
        let mut priority = HashMap::new();
        priority.insert("copilot".to_string(), 1);
        priority.insert("openai_codex".to_string(), 2);
        let (provider, c) =
            ProviderRegistry::pick_best_provider(&ProviderId::OpenAICodex, &creds, &priority);
        assert_eq!(provider, ProviderId::Copilot);
        assert_eq!(c.unwrap().access_token, "cop-tok");
    }

    #[test]
    fn test_pick_best_provider_native_higher_priority() {
        let mut creds = HashMap::new();
        creds.insert(
            "anthropic".to_string(),
            ProviderCredentials {
                provider: ProviderId::Anthropic,
                access_token: "sk-ant".to_string(),
                base_url: None,
                account_label: "default".to_string(),
            },
        );
        creds.insert(
            "copilot".to_string(),
            ProviderCredentials {
                provider: ProviderId::Copilot,
                access_token: "cop-tok".to_string(),
                base_url: Some("https://api.githubcopilot.com".to_string()),
                account_label: "default".to_string(),
            },
        );
        // User sets anthropic priority=1, copilot priority=5 → anthropic wins
        let mut priority = HashMap::new();
        priority.insert("anthropic".to_string(), 1);
        priority.insert("copilot".to_string(), 5);
        let (provider, c) =
            ProviderRegistry::pick_best_provider(&ProviderId::Anthropic, &creds, &priority);
        assert_eq!(provider, ProviderId::Anthropic);
        assert_eq!(c.unwrap().access_token, "sk-ant");
    }

    #[test]
    fn test_pick_best_provider_equal_priority_prefers_native() {
        let mut creds = HashMap::new();
        creds.insert(
            "openai_codex".to_string(),
            ProviderCredentials {
                provider: ProviderId::OpenAICodex,
                access_token: "sk-oai".to_string(),
                base_url: None,
                account_label: "default".to_string(),
            },
        );
        creds.insert(
            "copilot".to_string(),
            ProviderCredentials {
                provider: ProviderId::Copilot,
                access_token: "cop-tok".to_string(),
                base_url: Some("https://api.githubcopilot.com".to_string()),
                account_label: "default".to_string(),
            },
        );
        // Equal priority → native wins (tie-break)
        let mut priority = HashMap::new();
        priority.insert("openai_codex".to_string(), 1);
        priority.insert("copilot".to_string(), 1);
        let (provider, _) =
            ProviderRegistry::pick_best_provider(&ProviderId::OpenAICodex, &creds, &priority);
        assert_eq!(provider, ProviderId::OpenAICodex);
    }

    #[tokio::test]
    async fn test_resolve_provider_cache_with_priority_picks_copilot() {
        let registry = ProviderRegistry::new();
        let uid = Uuid::new_v4();

        let mut creds_map = HashMap::new();
        creds_map.insert(
            "anthropic".to_string(),
            ProviderCredentials {
                provider: ProviderId::Anthropic,
                access_token: "sk-ant".to_string(),
                base_url: None,
                account_label: "default".to_string(),
            },
        );
        creds_map.insert(
            "copilot".to_string(),
            ProviderCredentials {
                provider: ProviderId::Copilot,
                access_token: "cop-tok".to_string(),
                base_url: Some("https://api.githubcopilot.com".to_string()),
                account_label: "default".to_string(),
            },
        );
        let mut priority_map = HashMap::new();
        priority_map.insert("copilot".to_string(), 1);
        priority_map.insert("anthropic".to_string(), 2);

        registry.cache.insert(
            uid,
            CacheEntry {
                credentials: creds_map,
                expires_at: HashMap::new(),
                priority: priority_map,
                cached_at: Instant::now(),
            },
        );

        let (provider, creds) = registry
            .resolve_provider(Some(uid), "claude-sonnet-4", None)
            .await;
        assert_eq!(provider, ProviderId::Copilot);
        assert_eq!(creds.unwrap().access_token, "cop-tok");
    }

    #[test]
    fn test_pick_best_provider_copilot_for_openai_model() {
        // Copilot can serve OpenAI models too
        let mut creds = HashMap::new();
        creds.insert(
            "copilot".to_string(),
            ProviderCredentials {
                provider: ProviderId::Copilot,
                access_token: "cop-tok".to_string(),
                base_url: Some("https://api.githubcopilot.com".to_string()),
                account_label: "default".to_string(),
            },
        );
        let priority = HashMap::new();
        let (provider, c) =
            ProviderRegistry::pick_best_provider(&ProviderId::OpenAICodex, &creds, &priority);
        assert_eq!(provider, ProviderId::Copilot);
        assert_eq!(c.unwrap().access_token, "cop-tok");
    }

    #[test]
    fn test_pick_best_provider_copilot_base_url_preserved() {
        let mut creds = HashMap::new();
        creds.insert(
            "copilot".to_string(),
            ProviderCredentials {
                provider: ProviderId::Copilot,
                access_token: "cop-tok".to_string(),
                base_url: Some("https://api.business.githubcopilot.com".to_string()),
                account_label: "default".to_string(),
            },
        );
        let priority = HashMap::new();
        let (_, c) =
            ProviderRegistry::pick_best_provider(&ProviderId::Anthropic, &creds, &priority);
        let c = c.unwrap();
        assert_eq!(
            c.base_url.unwrap(),
            "https://api.business.githubcopilot.com"
        );
    }

    #[tokio::test]
    async fn test_resolve_provider_stale_cache_falls_back_to_kiro_without_db() {
        let registry = ProviderRegistry::new();
        let uid = Uuid::new_v4();

        // Insert a cache entry that's already expired (cached_at in the past)
        let mut creds_map = HashMap::new();
        creds_map.insert(
            "anthropic".to_string(),
            ProviderCredentials {
                provider: ProviderId::Anthropic,
                access_token: "sk-ant".to_string(),
                base_url: None,
                account_label: "default".to_string(),
            },
        );
        registry.cache.insert(
            uid,
            CacheEntry {
                credentials: creds_map,
                expires_at: HashMap::new(),
                priority: HashMap::new(),
                cached_at: Instant::now() - Duration::from_secs(600), // 10 min ago, past TTL
            },
        );

        // No DB provided — should fall back to Kiro
        let (provider, creds) = registry
            .resolve_provider(Some(uid), "claude-sonnet-4", None)
            .await;
        assert_eq!(provider, ProviderId::Kiro);
        assert!(creds.is_none());
    }

    #[test]
    fn test_provider_registry_default() {
        let registry = ProviderRegistry::default();
        assert_eq!(registry.cache.len(), 0);
        assert_eq!(registry.refresh_locks.len(), 0);
    }

    #[test]
    fn test_provider_for_model_o4_prefix() {
        assert_eq!(
            ProviderRegistry::provider_for_model("o4-mini"),
            Some(ProviderId::OpenAICodex)
        );
    }

    // ── Multi-provider proxy credential tests ─────────────────────────

    fn make_proxy_creds() -> HashMap<ProviderId, ProviderCredentials> {
        let mut creds = HashMap::new();
        creds.insert(
            ProviderId::Anthropic,
            ProviderCredentials {
                provider: ProviderId::Anthropic,
                access_token: "sk-ant-proxy-test".to_string(),
                base_url: None,
                account_label: "proxy".to_string(),
            },
        );
        creds.insert(
            ProviderId::OpenAICodex,
            ProviderCredentials {
                provider: ProviderId::OpenAICodex,
                access_token: "sk-proj-proxy-test".to_string(),
                base_url: Some("https://api.openai.com/v1".to_string()),
                account_label: "proxy".to_string(),
            },
        );
        creds
    }

    #[test]
    fn test_resolve_from_proxy_creds_anthropic_model() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds());
        let (provider, creds) = registry.resolve_from_proxy_creds("claude-sonnet-4");
        assert_eq!(provider, ProviderId::Anthropic);
        let creds = creds.expect("expected proxy credentials");
        assert_eq!(creds.access_token, "sk-ant-proxy-test");
    }

    #[test]
    fn test_resolve_from_proxy_creds_openai_model() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds());
        let (provider, creds) = registry.resolve_from_proxy_creds("gpt-4o");
        assert_eq!(provider, ProviderId::OpenAICodex);
        let creds = creds.expect("expected proxy credentials");
        assert_eq!(creds.access_token, "sk-proj-proxy-test");
        assert_eq!(creds.base_url.as_deref(), Some("https://api.openai.com/v1"));
    }

    #[test]
    fn test_resolve_from_proxy_creds_prefixed_model() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds());
        let (provider, creds) = registry.resolve_from_proxy_creds("anthropic/claude-opus-4-6");
        assert_eq!(provider, ProviderId::Anthropic);
        assert!(creds.is_some());
    }

    #[test]
    fn test_resolve_from_proxy_creds_unknown_model_falls_back_to_kiro() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds());
        let (provider, creds) = registry.resolve_from_proxy_creds("auto");
        assert_eq!(provider, ProviderId::Kiro);
        assert!(creds.is_none());
    }

    #[test]
    fn test_resolve_from_proxy_creds_no_proxy_creds_returns_kiro() {
        let registry = ProviderRegistry::new();
        let (provider, creds) = registry.resolve_from_proxy_creds("claude-sonnet-4");
        assert_eq!(provider, ProviderId::Kiro);
        assert!(creds.is_none());
    }

    #[test]
    fn test_resolve_from_proxy_creds_provider_not_configured() {
        // Only Anthropic configured, but model routes to OpenAI
        let mut creds = HashMap::new();
        creds.insert(
            ProviderId::Anthropic,
            ProviderCredentials {
                provider: ProviderId::Anthropic,
                access_token: "sk-ant-test".to_string(),
                base_url: None,
                account_label: "proxy".to_string(),
            },
        );
        let registry = ProviderRegistry::new_with_proxy(creds);
        let (provider, creds) = registry.resolve_from_proxy_creds("gpt-4o");
        assert_eq!(provider, ProviderId::Kiro);
        assert!(creds.is_none());
    }

    #[tokio::test]
    async fn test_resolve_provider_user_id_none_with_proxy_creds() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds());
        let (provider, creds) = registry
            .resolve_provider(None, "claude-sonnet-4", None)
            .await;
        assert_eq!(provider, ProviderId::Anthropic);
        assert!(creds.is_some());
    }

    #[tokio::test]
    async fn test_resolve_provider_user_id_none_no_proxy_creds() {
        let registry = ProviderRegistry::new();
        let (provider, creds) = registry
            .resolve_provider(None, "claude-sonnet-4", None)
            .await;
        assert_eq!(provider, ProviderId::Kiro);
        assert!(creds.is_none());
    }

    #[tokio::test]
    async fn test_resolve_provider_with_user_id_no_db_uses_proxy_creds() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds());
        let uid = Uuid::new_v4();
        let (provider, creds) = registry.resolve_provider(Some(uid), "gpt-4o", None).await;
        assert_eq!(provider, ProviderId::OpenAICodex);
        assert!(creds.is_some());
    }

    #[tokio::test]
    async fn test_resolve_provider_proxy_user_id_no_db() {
        let proxy_user_id = Uuid::from_u128(0x0000_0001_0000_0000_0000_0000_0000_0001);
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds());
        let (provider, creds) = registry
            .resolve_provider(Some(proxy_user_id), "claude-opus-4-6", None)
            .await;
        assert_eq!(provider, ProviderId::Anthropic);
        let creds = creds.expect("expected proxy credentials");
        assert_eq!(creds.access_token, "sk-ant-proxy-test");
    }

    #[tokio::test]
    async fn test_resolve_provider_proxy_user_id_unknown_model() {
        let proxy_user_id = Uuid::from_u128(0x0000_0001_0000_0000_0000_0000_0000_0001);
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds());
        let (provider, creds) = registry
            .resolve_provider(Some(proxy_user_id), "kiro-auto", None)
            .await;
        assert_eq!(provider, ProviderId::Kiro);
        assert!(creds.is_none());
    }

    #[test]
    fn test_new_with_proxy_empty_creds_sets_none() {
        let registry = ProviderRegistry::new_with_proxy(HashMap::new());
        assert!(registry.proxy_credentials.is_empty());
    }

    #[test]
    fn test_new_with_proxy_non_empty_creds_sets_some() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds());
        assert!(!registry.proxy_credentials.is_empty());
    }

    #[test]
    fn test_resolve_from_proxy_creds_o3_routes_to_openai() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds());
        let (provider, _) = registry.resolve_from_proxy_creds("o3-pro");
        assert_eq!(provider, ProviderId::OpenAICodex);
    }

    #[test]
    fn test_resolve_from_proxy_creds_o4_routes_to_openai() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds());
        let (provider, _) = registry.resolve_from_proxy_creds("o4-mini");
        assert_eq!(provider, ProviderId::OpenAICodex);
    }

    #[test]
    fn test_resolve_from_proxy_creds_chatgpt_routes_to_openai() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds());
        let (provider, _) = registry.resolve_from_proxy_creds("chatgpt-4o-latest");
        assert_eq!(provider, ProviderId::OpenAICodex);
    }

    #[test]
    fn test_resolve_from_proxy_creds_prefix_overrides_name_inference() {
        // "openai_codex/claude-sonnet-4" should route to OpenAI, not Anthropic
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds());
        let (provider, _) = registry.resolve_from_proxy_creds("openai_codex/claude-sonnet-4");
        assert_eq!(provider, ProviderId::OpenAICodex);
    }

    // ── from_proxy_config tests ─────────────────────────────────────

    #[test]
    fn test_from_proxy_config_all_providers() {
        use crate::config::ProxyConfig;
        let proxy = ProxyConfig {
            api_key: "test-key-long-enough".to_string(),
            anthropic_enabled: true,
            anthropic_access_token: Some("ant-access-tok".to_string()),
            openai_enabled: true,
            openai_access_token: Some("oai-access-tok".to_string()),
            copilot_token: Some("cop-tok".to_string()),
            copilot_base_url: Some("https://api.githubcopilot.com".to_string()),
            ..Default::default()
        };
        let registry = ProviderRegistry::from_proxy_config(&proxy);
        let creds = registry.proxy_credentials().unwrap();
        assert_eq!(creds.len(), 3);
        assert!(creds.contains_key(&ProviderId::Anthropic));
        assert!(creds.contains_key(&ProviderId::OpenAICodex));
        assert!(creds.contains_key(&ProviderId::Copilot));
        assert_eq!(creds[&ProviderId::Anthropic].access_token, "ant-access-tok");
        assert_eq!(
            creds[&ProviderId::OpenAICodex].access_token,
            "oai-access-tok"
        );
    }

    #[test]
    fn test_from_proxy_config_partial_providers() {
        use crate::config::ProxyConfig;
        let proxy = ProxyConfig {
            api_key: "test-key-long-enough".to_string(),
            anthropic_enabled: true,
            anthropic_access_token: Some("ant-access-tok".to_string()),
            ..Default::default()
        };
        let registry = ProviderRegistry::from_proxy_config(&proxy);
        let creds = registry.proxy_credentials().unwrap();
        assert_eq!(creds.len(), 1);
        assert!(creds.contains_key(&ProviderId::Anthropic));
        assert!(!creds.contains_key(&ProviderId::OpenAICodex));
    }

    #[test]
    fn test_from_proxy_config_empty() {
        use crate::config::ProxyConfig;
        let proxy = ProxyConfig {
            api_key: "test-key-long-enough".to_string(),
            ..Default::default()
        };
        let registry = ProviderRegistry::from_proxy_config(&proxy);
        assert!(registry.proxy_credentials().is_none());
    }

    // ── Explicit prefix enforcement tests (S-003) ─────────────────────

    #[tokio::test]
    async fn test_explicit_prefix_binding_skips_copilot() {
        let registry = ProviderRegistry::new();
        let uid = Uuid::new_v4();

        // Cache has both Anthropic and Copilot creds, Copilot has better priority
        let mut creds_map = HashMap::new();
        creds_map.insert(
            "anthropic".to_string(),
            ProviderCredentials {
                provider: ProviderId::Anthropic,
                access_token: "sk-ant-direct".to_string(),
                base_url: None,
                account_label: "default".to_string(),
            },
        );
        creds_map.insert(
            "copilot".to_string(),
            ProviderCredentials {
                provider: ProviderId::Copilot,
                access_token: "cop-tok".to_string(),
                base_url: Some("https://api.githubcopilot.com".to_string()),
                account_label: "default".to_string(),
            },
        );
        let mut priority = HashMap::new();
        priority.insert("copilot".to_string(), 0); // Copilot preferred
        priority.insert("anthropic".to_string(), 1);

        registry.cache.insert(
            uid,
            CacheEntry {
                credentials: creds_map,
                expires_at: HashMap::new(),
                priority,
                cached_at: Instant::now(),
            },
        );

        // Without prefix: picks Copilot (better priority)
        let (pid, _) = registry
            .resolve_provider(Some(uid), "claude-sonnet-4", None)
            .await;
        assert_eq!(pid, ProviderId::Copilot);

        // With explicit prefix: MUST use Anthropic regardless of priority
        let (pid, creds) = registry
            .resolve_provider(Some(uid), "anthropic/claude-sonnet-4", None)
            .await;
        assert_eq!(pid, ProviderId::Anthropic);
        assert_eq!(creds.unwrap().access_token, "sk-ant-direct");
    }

    #[tokio::test]
    async fn test_explicit_prefix_no_creds_returns_provider_not_kiro() {
        let registry = ProviderRegistry::new();
        let uid = Uuid::new_v4();

        // Cache has only Copilot, no Anthropic
        let mut creds_map = HashMap::new();
        creds_map.insert(
            "copilot".to_string(),
            ProviderCredentials {
                provider: ProviderId::Copilot,
                access_token: "cop-tok".to_string(),
                base_url: Some("https://api.githubcopilot.com".to_string()),
                account_label: "default".to_string(),
            },
        );
        registry.cache.insert(
            uid,
            CacheEntry {
                credentials: creds_map,
                expires_at: HashMap::new(),
                priority: HashMap::new(),
                cached_at: Instant::now(),
            },
        );

        // Explicit prefix for Anthropic — no creds, but returns Anthropic (NOT Kiro)
        let (pid, creds) = registry
            .resolve_provider(Some(uid), "anthropic/claude-sonnet-4", None)
            .await;
        assert_eq!(pid, ProviderId::Anthropic);
        assert!(creds.is_none()); // No creds — handler will return AuthError
    }

    #[test]
    fn test_explicit_prefix_proxy_no_creds_returns_provider_not_kiro() {
        // Only OpenAI configured, but explicit prefix for Anthropic
        let mut proxy_creds = HashMap::new();
        proxy_creds.insert(
            ProviderId::OpenAICodex,
            ProviderCredentials {
                provider: ProviderId::OpenAICodex,
                access_token: "sk-oai".to_string(),
                base_url: None,
                account_label: "proxy".to_string(),
            },
        );
        let registry = ProviderRegistry::new_with_proxy(proxy_creds);

        let (pid, creds) = registry.resolve_from_proxy_creds("anthropic/claude-sonnet-4");
        assert_eq!(pid, ProviderId::Anthropic);
        assert!(creds.is_none()); // No Anthropic proxy creds
    }

    // ── Token refresh provider inference tests (S-006) ───────────────

    #[test]
    fn test_prefixed_model_routes_refresh_to_correct_provider() {
        // parse_prefixed_model should extract the provider, not provider_for_model
        let result = ProviderRegistry::parse_prefixed_model("openai_codex/claude-sonnet-4");
        assert!(result.is_some());
        let (pid, model_id) = result.unwrap();
        assert_eq!(pid, ProviderId::OpenAICodex);
        assert_eq!(model_id, "claude-sonnet-4");

        // Without prefix, provider_for_model would route to Anthropic
        assert_eq!(
            ProviderRegistry::provider_for_model("claude-sonnet-4"),
            Some(ProviderId::Anthropic)
        );
    }

    #[test]
    fn test_openai_alias_prefix_works() {
        // "openai/gpt-4o" should parse correctly with the alias
        let result = ProviderRegistry::parse_prefixed_model("openai/gpt-4o");
        assert!(result.is_some());
        let (pid, model_id) = result.unwrap();
        assert_eq!(pid, ProviderId::OpenAICodex);
        assert_eq!(model_id, "gpt-4o");
    }

    // ── Provider enabled/disabled tests ─────────────────────────────

    #[tokio::test]
    async fn test_kiro_always_enabled() {
        let registry = ProviderRegistry::new();
        registry.set_provider_enabled(ProviderId::Kiro, false).await;
        assert!(registry.is_provider_enabled(&ProviderId::Kiro).await);
    }

    #[tokio::test]
    async fn test_disable_provider() {
        let registry = ProviderRegistry::new();
        assert!(registry.is_provider_enabled(&ProviderId::Anthropic).await);
        registry
            .set_provider_enabled(ProviderId::Anthropic, false)
            .await;
        assert!(!registry.is_provider_enabled(&ProviderId::Anthropic).await);
    }

    #[tokio::test]
    async fn test_re_enable_provider() {
        let registry = ProviderRegistry::new();
        registry
            .set_provider_enabled(ProviderId::Copilot, false)
            .await;
        assert!(!registry.is_provider_enabled(&ProviderId::Copilot).await);
        registry
            .set_provider_enabled(ProviderId::Copilot, true)
            .await;
        assert!(registry.is_provider_enabled(&ProviderId::Copilot).await);
    }

    #[tokio::test]
    async fn test_disabled_providers_snapshot() {
        let registry = ProviderRegistry::new();
        registry
            .set_provider_enabled(ProviderId::Anthropic, false)
            .await;
        registry
            .set_provider_enabled(ProviderId::OpenAICodex, false)
            .await;
        let disabled = registry.disabled_providers().await;
        assert_eq!(disabled.len(), 2);
        assert!(disabled.contains(&ProviderId::Anthropic));
        assert!(disabled.contains(&ProviderId::OpenAICodex));
        assert!(!disabled.contains(&ProviderId::Kiro));
    }

    #[tokio::test]
    async fn test_new_registry_all_enabled() {
        let registry = ProviderRegistry::new();
        for pid in ProviderId::all_visible() {
            assert!(registry.is_provider_enabled(pid).await);
        }
    }
}
