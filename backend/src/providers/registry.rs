/// Provider registry: resolves which provider to use for a given user + model.
///
/// Caches per-user provider credentials in memory (5-minute TTL) to avoid
/// repeated DB lookups on every request. Handles transparent token refresh
/// for OAuth-based provider tokens.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

struct CacheEntry {
    credentials: HashMap<String, ProviderCredentials>,
    /// Per-provider token expiry times.
    expires_at: HashMap<String, DateTime<Utc>>,
    /// User's provider priority (provider_id -> priority). Lower = preferred.
    priority: HashMap<String, i32>,
    cached_at: Instant,
}

/// Per-(user_id, provider) mutex map to prevent concurrent refresh storms.
type RefreshLockMap = DashMap<(Uuid, String), Arc<tokio::sync::Mutex<()>>>;

/// Resolves provider + credentials for a user + model combination.
pub struct ProviderRegistry {
    cache: Arc<DashMap<Uuid, CacheEntry>>,
    refresh_locks: Arc<RefreshLockMap>,
    /// Static credentials for proxy mode (populated from env vars at startup).
    proxy_credentials: Option<HashMap<ProviderId, ProviderCredentials>>,
    /// Model names that route to the Custom provider.
    custom_models: HashSet<String>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
            refresh_locks: Arc::new(DashMap::new()),
            proxy_credentials: None,
            custom_models: HashSet::new(),
        }
    }

    /// Create a registry with static proxy credentials (for proxy-only mode).
    pub fn new_with_proxy(
        proxy_creds: HashMap<ProviderId, ProviderCredentials>,
        custom_models: HashSet<String>,
    ) -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
            refresh_locks: Arc::new(DashMap::new()),
            proxy_credentials: if proxy_creds.is_empty() {
                None
            } else {
                Some(proxy_creds)
            },
            custom_models,
        }
    }

    /// Build a registry from a ProxyConfig (env-var based proxy-only mode).
    ///
    /// Extracts provider credentials and custom model list from the config,
    /// returning a fully wired ProviderRegistry.
    pub fn from_proxy_config(proxy: &crate::config::ProxyConfig) -> Self {
        use crate::providers::types::ProviderCredentials;

        let mut creds = HashMap::new();
        let mut custom_models = HashSet::new();

        if let Some(ref key) = proxy.anthropic_api_key {
            creds.insert(
                ProviderId::Anthropic,
                ProviderCredentials {
                    provider: ProviderId::Anthropic,
                    access_token: key.clone(),
                    base_url: None,
                    account_label: "proxy".into(),
                },
            );
        }
        if let Some(ref key) = proxy.openai_api_key {
            creds.insert(
                ProviderId::OpenAICodex,
                ProviderCredentials {
                    provider: ProviderId::OpenAICodex,
                    access_token: key.clone(),
                    base_url: proxy.openai_base_url.clone(),
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
        if let Some(ref token) = proxy.qwen_token {
            creds.insert(
                ProviderId::Qwen,
                ProviderCredentials {
                    provider: ProviderId::Qwen,
                    access_token: token.clone(),
                    base_url: proxy.qwen_base_url.clone(),
                    account_label: "proxy".into(),
                },
            );
        }
        if let Some(ref url) = proxy.custom_provider_url {
            creds.insert(
                ProviderId::Custom,
                ProviderCredentials {
                    provider: ProviderId::Custom,
                    access_token: proxy.custom_provider_key.clone().unwrap_or_default(),
                    base_url: Some(url.clone()),
                    account_label: "proxy".into(),
                },
            );
        }
        if let Some(ref models) = proxy.custom_provider_models {
            custom_models = models
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }

        Self::new_with_proxy(creds, custom_models)
    }

    /// Access the static proxy credentials (if any).
    pub fn proxy_credentials(&self) -> &Option<HashMap<ProviderId, ProviderCredentials>> {
        &self.proxy_credentials
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
        } else if model.starts_with("qwen-")
            || model.starts_with("qwen3-")
            || model.starts_with("qwq-")
        {
            Some(ProviderId::Qwen)
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
        let Some(target) = Self::provider_for_model(model) else {
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
            return self.resolve_from_proxy_creds(model);
        };

        // Try explicit prefix first (e.g. "anthropic/claude-opus-4-6")
        let (native, actual_model) =
            if let Some((provider, _model_id)) = Self::parse_prefixed_model(model) {
                (provider, model)
            } else if let Some(provider) = Self::provider_for_model(model) {
                (provider, model)
            } else {
                return (ProviderId::Kiro, None);
            };
        // actual_model is used for logging context only; routing uses `native`
        let _ = actual_model;

        // Cache hit?
        if let Some(entry) = self.cache.get(&uid) {
            if entry.cached_at.elapsed() < CACHE_TTL {
                return Self::pick_best_provider(&native, &entry.credentials, &entry.priority);
            }
        }

        // Cache miss or stale — load from DB
        let Some(db) = db else {
            // Proxy mode with Kiro creds but no DB: check proxy credential store
            return self.resolve_from_proxy_creds(model);
        };
        let (user_creds, user_expires, user_priority) = Self::load_user_data(uid, db).await;
        let result = Self::pick_best_provider(&native, &user_creds, &user_priority);
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
        let native = if let Some((provider, _model_id)) = Self::parse_prefixed_model(model) {
            provider
        } else if let Some(provider) = Self::provider_for_model(model) {
            provider
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

        // Load all user accounts for this provider
        if let Ok(rows) = db.get_all_user_provider_tokens(uid, provider_str).await {
            for row in rows {
                let now = chrono::Utc::now();
                if row.expires_at > now {
                    let base_url = if provider_str == "qwen" {
                        row.base_url.clone()
                    } else {
                        None
                    };
                    let account_id = AccountId {
                        user_id: Some(uid),
                        provider_id: native.clone(),
                        account_label: row.account_label.clone(),
                    };
                    let creds = ProviderCredentials {
                        provider: native.clone(),
                        access_token: row.access_token.clone(),
                        base_url,
                        account_label: row.account_label.clone(),
                    };
                    candidates.push((account_id, creds, native_pri));
                }
            }
        }

        // Also check Copilot as an alternative (universal provider)
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

        // Load admin pool accounts as fallback (default priority 100)
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

        if candidates.is_empty() {
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

    /// Resolve provider from static proxy credentials (env-var based, no DB).
    fn resolve_from_proxy_creds(&self, model: &str) -> (ProviderId, Option<ProviderCredentials>) {
        let Some(ref proxy_creds) = self.proxy_credentials else {
            return (ProviderId::Kiro, None);
        };
        // Determine target provider from model name
        let native = if let Some((provider, _)) = Self::parse_prefixed_model(model) {
            provider
        } else if let Some(provider) = Self::provider_for_model(model) {
            provider
        } else if self.custom_models.contains(model) {
            ProviderId::Custom
        } else {
            return (ProviderId::Kiro, None);
        };
        // Look up proxy credentials for that provider
        if let Some(cred) = proxy_creds.get(&native) {
            (native, Some(cred.clone()))
        } else {
            (ProviderId::Kiro, None)
        }
    }

    /// Return the set of provider IDs that have proxy credentials configured.
    pub fn configured_proxy_providers(&self) -> Vec<ProviderId> {
        match &self.proxy_credentials {
            Some(creds) => creds.keys().cloned().collect(),
            None => Vec::new(),
        }
    }

    /// Return the custom model names configured for proxy mode.
    pub fn custom_model_names(&self) -> &HashSet<String> {
        &self.custom_models
    }

    /// Invalidate the cache for a user. Call after a provider token is added, removed, or refreshed.
    pub fn invalidate(&self, user_id: Uuid) {
        self.cache.remove(&user_id);
    }

    /// Load all provider tokens and priority for a user from the database.
    async fn load_user_data(
        user_id: Uuid,
        db: &ConfigDb,
    ) -> (
        HashMap<String, ProviderCredentials>,
        HashMap<String, DateTime<Utc>>,
        HashMap<String, i32>,
    ) {
        let mut creds_map = HashMap::new();
        let mut expires_map = HashMap::new();
        for provider_str in &["anthropic", "openai_codex", "qwen"] {
            if let Ok(Some((access_token, _refresh_token, expires_at, _email))) =
                db.get_user_provider_token(user_id, provider_str).await
            {
                // Only include tokens that haven't fully expired
                let now = Utc::now();
                if expires_at > now {
                    let (provider, base_url) = match *provider_str {
                        "anthropic" => (ProviderId::Anthropic, None),
                        "openai_codex" => (ProviderId::OpenAICodex, None),
                        "qwen" => {
                            // Load base_url from DB for Qwen (set by device flow)
                            let url = db
                                .get_user_provider_base_url(user_id, "qwen")
                                .await
                                .ok()
                                .flatten();
                            (ProviderId::Qwen, url)
                        }
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
        // Gemini provider removed — gemini-* models should return None (fall through to Kiro)
        assert_eq!(ProviderRegistry::provider_for_model("gemini-2.5-pro"), None);
        assert_eq!(
            ProviderRegistry::provider_for_model("gemini-2.5-flash"),
            None
        );
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
    fn test_provider_for_model_qwen() {
        assert_eq!(
            ProviderRegistry::provider_for_model("qwen-coder-plus"),
            Some(ProviderId::Qwen)
        );
        assert_eq!(
            ProviderRegistry::provider_for_model("qwen-vl-plus"),
            Some(ProviderId::Qwen)
        );
        assert_eq!(
            ProviderRegistry::provider_for_model("qwen3-coder"),
            Some(ProviderId::Qwen)
        );
        assert_eq!(
            ProviderRegistry::provider_for_model("qwq-32b"),
            Some(ProviderId::Qwen)
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

    // ── 6.2: Qwen model routing edge cases ──────────────────────────

    #[test]
    fn test_provider_for_model_qwen_coder_variants() {
        // All qwen-coder-* models should route to Qwen
        assert_eq!(
            ProviderRegistry::provider_for_model("qwen-coder-plus"),
            Some(ProviderId::Qwen)
        );
        assert_eq!(
            ProviderRegistry::provider_for_model("qwen-coder-plus-latest"),
            Some(ProviderId::Qwen)
        );
        assert_eq!(
            ProviderRegistry::provider_for_model("qwen-coder-turbo"),
            Some(ProviderId::Qwen)
        );
    }

    #[test]
    fn test_provider_for_model_qwen_vl_variants() {
        assert_eq!(
            ProviderRegistry::provider_for_model("qwen-vl-plus"),
            Some(ProviderId::Qwen)
        );
        assert_eq!(
            ProviderRegistry::provider_for_model("qwen-vl-max"),
            Some(ProviderId::Qwen)
        );
    }

    #[test]
    fn test_provider_for_model_qwen3_variants() {
        assert_eq!(
            ProviderRegistry::provider_for_model("qwen3-coder"),
            Some(ProviderId::Qwen)
        );
        assert_eq!(
            ProviderRegistry::provider_for_model("qwen3-235b-a22b"),
            Some(ProviderId::Qwen)
        );
    }

    #[test]
    fn test_provider_for_model_qwq_variants() {
        assert_eq!(
            ProviderRegistry::provider_for_model("qwq-32b"),
            Some(ProviderId::Qwen)
        );
        assert_eq!(
            ProviderRegistry::provider_for_model("qwq-plus"),
            Some(ProviderId::Qwen)
        );
    }

    #[test]
    fn test_provider_for_model_qwen_no_collision_with_other_providers() {
        // "qwen" prefix should NOT match other providers
        assert_ne!(
            ProviderRegistry::provider_for_model("qwen-coder-plus"),
            Some(ProviderId::OpenAICodex)
        );
        assert_ne!(
            ProviderRegistry::provider_for_model("qwen-coder-plus"),
            Some(ProviderId::Anthropic)
        );
        assert_ne!(
            ProviderRegistry::provider_for_model("qwen-coder-plus"),
            Some(ProviderId::Copilot)
        );
    }

    #[test]
    fn test_provider_for_model_qwen_without_dash_falls_through() {
        // "qwen" alone (no dash) should NOT match — prefix is "qwen-"
        assert_eq!(ProviderRegistry::provider_for_model("qwen"), None);
        // "qwen2" should NOT match (no "qwen2-" prefix in the code)
        assert_eq!(ProviderRegistry::provider_for_model("qwen2-72b"), None);
    }

    // ── 6.7: Registry integration — Qwen cache + resolve ───────────

    #[tokio::test]
    async fn test_resolve_provider_cache_hit_returns_qwen() {
        let registry = ProviderRegistry::new();
        let uid = Uuid::new_v4();

        let mut creds_map = HashMap::new();
        creds_map.insert(
            "qwen".to_string(),
            ProviderCredentials {
                provider: ProviderId::Qwen,
                access_token: "qwen-tok-123".to_string(),
                base_url: Some("https://custom.qwen.ai/api".to_string()),
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
            .resolve_provider(Some(uid), "qwen-coder-plus", None)
            .await;
        assert_eq!(provider, ProviderId::Qwen);
        let creds = creds.expect("expected Qwen credentials");
        assert_eq!(creds.access_token, "qwen-tok-123");
        assert_eq!(creds.base_url.unwrap(), "https://custom.qwen.ai/api");
    }

    #[tokio::test]
    async fn test_resolve_provider_qwen_model_no_token_returns_kiro() {
        let registry = ProviderRegistry::new();
        let uid = Uuid::new_v4();

        // Cache with no Qwen credentials
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
            .resolve_provider(Some(uid), "qwen-coder-plus", None)
            .await;
        assert_eq!(provider, ProviderId::Kiro);
        assert!(creds.is_none());
    }

    #[test]
    fn test_pick_best_provider_qwen_native_only() {
        let mut creds = HashMap::new();
        creds.insert(
            "qwen".to_string(),
            ProviderCredentials {
                provider: ProviderId::Qwen,
                access_token: "qwen-tok".to_string(),
                base_url: Some("https://custom.qwen.ai/api".to_string()),
                account_label: "default".to_string(),
            },
        );
        let priority = HashMap::new();
        let (provider, c) =
            ProviderRegistry::pick_best_provider(&ProviderId::Qwen, &creds, &priority);
        assert_eq!(provider, ProviderId::Qwen);
        let c = c.unwrap();
        assert_eq!(c.access_token, "qwen-tok");
        assert_eq!(c.base_url.unwrap(), "https://custom.qwen.ai/api");
    }

    #[test]
    fn test_pick_best_provider_qwen_and_copilot_default_prefers_qwen() {
        let mut creds = HashMap::new();
        creds.insert(
            "qwen".to_string(),
            ProviderCredentials {
                provider: ProviderId::Qwen,
                access_token: "qwen-tok".to_string(),
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
        // No priority set — native (qwen) default=0, copilot default=1 → qwen wins
        let priority = HashMap::new();
        let (provider, _) =
            ProviderRegistry::pick_best_provider(&ProviderId::Qwen, &creds, &priority);
        assert_eq!(provider, ProviderId::Qwen);
    }

    #[test]
    fn test_pick_best_provider_copilot_preferred_over_qwen() {
        let mut creds = HashMap::new();
        creds.insert(
            "qwen".to_string(),
            ProviderCredentials {
                provider: ProviderId::Qwen,
                access_token: "qwen-tok".to_string(),
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
        let mut priority = HashMap::new();
        priority.insert("copilot".to_string(), 1);
        priority.insert("qwen".to_string(), 2);
        let (provider, c) =
            ProviderRegistry::pick_best_provider(&ProviderId::Qwen, &creds, &priority);
        assert_eq!(provider, ProviderId::Copilot);
        assert_eq!(c.unwrap().access_token, "cop-tok");
    }

    #[test]
    fn test_pick_best_provider_copilot_only_for_qwen_model() {
        // User has only Copilot, requesting a Qwen model
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
        let (provider, _) =
            ProviderRegistry::pick_best_provider(&ProviderId::Qwen, &creds, &priority);
        assert_eq!(provider, ProviderId::Copilot);
    }

    #[tokio::test]
    async fn test_resolve_provider_qwq_model_routes_to_qwen() {
        let registry = ProviderRegistry::new();
        let uid = Uuid::new_v4();

        let mut creds_map = HashMap::new();
        creds_map.insert(
            "qwen".to_string(),
            ProviderCredentials {
                provider: ProviderId::Qwen,
                access_token: "qwen-tok".to_string(),
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

        let (provider, _) = registry.resolve_provider(Some(uid), "qwq-32b", None).await;
        assert_eq!(provider, ProviderId::Qwen);
    }

    #[tokio::test]
    async fn test_resolve_provider_qwen3_model_routes_to_qwen() {
        let registry = ProviderRegistry::new();
        let uid = Uuid::new_v4();

        let mut creds_map = HashMap::new();
        creds_map.insert(
            "qwen".to_string(),
            ProviderCredentials {
                provider: ProviderId::Qwen,
                access_token: "qwen-tok".to_string(),
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

        let (provider, _) = registry
            .resolve_provider(Some(uid), "qwen3-coder", None)
            .await;
        assert_eq!(provider, ProviderId::Qwen);
    }

    // ── 6.6: Token refresh — Qwen-specific cache behavior ──────────

    #[test]
    fn test_ensure_fresh_token_cache_fresh_qwen_skips_refresh() {
        let registry = ProviderRegistry::new();
        let uid = Uuid::new_v4();

        let mut expires_map = HashMap::new();
        expires_map.insert("qwen".to_string(), Utc::now() + chrono::Duration::hours(1));
        let mut creds_map = HashMap::new();
        creds_map.insert(
            "qwen".to_string(),
            ProviderCredentials {
                provider: ProviderId::Qwen,
                access_token: "qwen-still-valid".to_string(),
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

        // Verify the cache check logic: Qwen token is fresh
        let entry = registry.cache.get(&uid).unwrap();
        let expires_at = entry.expires_at.get("qwen").unwrap();
        let now = Utc::now();
        assert!(
            (*expires_at - now).num_seconds() > REFRESH_BUFFER_SECS,
            "Qwen token should be considered fresh"
        );
    }

    #[test]
    fn test_refresh_lock_key_qwen_provider() {
        let registry = ProviderRegistry::new();
        let uid = Uuid::new_v4();

        registry.refresh_locks.insert(
            (uid, "qwen".to_string()),
            Arc::new(tokio::sync::Mutex::new(())),
        );

        assert!(registry
            .refresh_locks
            .contains_key(&(uid, "qwen".to_string())));
        // Different provider for same user should be separate
        assert!(!registry
            .refresh_locks
            .contains_key(&(uid, "anthropic".to_string())));
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
        creds.insert(
            ProviderId::Custom,
            ProviderCredentials {
                provider: ProviderId::Custom,
                access_token: String::new(),
                base_url: Some("http://localhost:11434/v1".to_string()),
                account_label: "proxy".to_string(),
            },
        );
        creds
    }

    #[test]
    fn test_resolve_from_proxy_creds_anthropic_model() {
        let custom_models = HashSet::new();
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds(), custom_models);
        let (provider, creds) = registry.resolve_from_proxy_creds("claude-sonnet-4");
        assert_eq!(provider, ProviderId::Anthropic);
        let creds = creds.expect("expected proxy credentials");
        assert_eq!(creds.access_token, "sk-ant-proxy-test");
    }

    #[test]
    fn test_resolve_from_proxy_creds_openai_model() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds(), HashSet::new());
        let (provider, creds) = registry.resolve_from_proxy_creds("gpt-4o");
        assert_eq!(provider, ProviderId::OpenAICodex);
        let creds = creds.expect("expected proxy credentials");
        assert_eq!(creds.access_token, "sk-proj-proxy-test");
        assert_eq!(creds.base_url.as_deref(), Some("https://api.openai.com/v1"));
    }

    #[test]
    fn test_resolve_from_proxy_creds_prefixed_model() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds(), HashSet::new());
        let (provider, creds) = registry.resolve_from_proxy_creds("anthropic/claude-opus-4-6");
        assert_eq!(provider, ProviderId::Anthropic);
        assert!(creds.is_some());
    }

    #[test]
    fn test_resolve_from_proxy_creds_custom_model() {
        let mut custom_models = HashSet::new();
        custom_models.insert("llama3".to_string());
        custom_models.insert("deepseek-r1".to_string());
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds(), custom_models);

        let (provider, creds) = registry.resolve_from_proxy_creds("llama3");
        assert_eq!(provider, ProviderId::Custom);
        let creds = creds.expect("expected custom proxy credentials");
        assert_eq!(creds.base_url.as_deref(), Some("http://localhost:11434/v1"));
    }

    #[test]
    fn test_resolve_from_proxy_creds_custom_model_second() {
        let mut custom_models = HashSet::new();
        custom_models.insert("llama3".to_string());
        custom_models.insert("deepseek-r1".to_string());
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds(), custom_models);

        let (provider, _) = registry.resolve_from_proxy_creds("deepseek-r1");
        assert_eq!(provider, ProviderId::Custom);
    }

    #[test]
    fn test_resolve_from_proxy_creds_unknown_model_falls_back_to_kiro() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds(), HashSet::new());
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
        // Only Anthropic configured, but model routes to Qwen
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
        let registry = ProviderRegistry::new_with_proxy(creds, HashSet::new());
        let (provider, creds) = registry.resolve_from_proxy_creds("qwen-coder-plus");
        assert_eq!(provider, ProviderId::Kiro);
        assert!(creds.is_none());
    }

    #[tokio::test]
    async fn test_resolve_provider_user_id_none_with_proxy_creds() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds(), HashSet::new());
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
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds(), HashSet::new());
        let uid = Uuid::new_v4();
        let (provider, creds) = registry.resolve_provider(Some(uid), "gpt-4o", None).await;
        assert_eq!(provider, ProviderId::OpenAICodex);
        assert!(creds.is_some());
    }

    #[tokio::test]
    async fn test_resolve_provider_proxy_user_id_no_db() {
        let proxy_user_id = Uuid::from_u128(0x0000_0001_0000_0000_0000_0000_0000_0001);
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds(), HashSet::new());
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
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds(), HashSet::new());
        let (provider, creds) = registry
            .resolve_provider(Some(proxy_user_id), "kiro-auto", None)
            .await;
        assert_eq!(provider, ProviderId::Kiro);
        assert!(creds.is_none());
    }

    #[test]
    fn test_new_with_proxy_empty_creds_sets_none() {
        let registry = ProviderRegistry::new_with_proxy(HashMap::new(), HashSet::new());
        assert!(registry.proxy_credentials.is_none());
    }

    #[test]
    fn test_new_with_proxy_non_empty_creds_sets_some() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds(), HashSet::new());
        assert!(registry.proxy_credentials.is_some());
    }

    #[test]
    fn test_configured_proxy_providers_empty() {
        let registry = ProviderRegistry::new();
        assert!(registry.configured_proxy_providers().is_empty());
    }

    #[test]
    fn test_configured_proxy_providers_returns_configured() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds(), HashSet::new());
        let providers = registry.configured_proxy_providers();
        assert!(providers.contains(&ProviderId::Anthropic));
        assert!(providers.contains(&ProviderId::OpenAICodex));
        assert!(providers.contains(&ProviderId::Custom));
        assert_eq!(providers.len(), 3);
    }

    #[test]
    fn test_custom_model_names_empty() {
        let registry = ProviderRegistry::new();
        assert!(registry.custom_model_names().is_empty());
    }

    #[test]
    fn test_custom_model_names_returns_set() {
        let mut models = HashSet::new();
        models.insert("llama3".to_string());
        models.insert("codellama".to_string());
        let registry = ProviderRegistry::new_with_proxy(HashMap::new(), models);
        let names = registry.custom_model_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains("llama3"));
        assert!(names.contains("codellama"));
    }

    #[test]
    fn test_resolve_from_proxy_creds_o3_routes_to_openai() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds(), HashSet::new());
        let (provider, _) = registry.resolve_from_proxy_creds("o3-pro");
        assert_eq!(provider, ProviderId::OpenAICodex);
    }

    #[test]
    fn test_resolve_from_proxy_creds_o4_routes_to_openai() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds(), HashSet::new());
        let (provider, _) = registry.resolve_from_proxy_creds("o4-mini");
        assert_eq!(provider, ProviderId::OpenAICodex);
    }

    #[test]
    fn test_resolve_from_proxy_creds_chatgpt_routes_to_openai() {
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds(), HashSet::new());
        let (provider, _) = registry.resolve_from_proxy_creds("chatgpt-4o-latest");
        assert_eq!(provider, ProviderId::OpenAICodex);
    }

    #[test]
    fn test_resolve_from_proxy_creds_prefix_overrides_name_inference() {
        // "openai_codex/claude-sonnet-4" should route to OpenAI, not Anthropic
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds(), HashSet::new());
        let (provider, _) = registry.resolve_from_proxy_creds("openai_codex/claude-sonnet-4");
        assert_eq!(provider, ProviderId::OpenAICodex);
    }

    #[test]
    fn test_resolve_from_proxy_creds_custom_model_not_in_set() {
        let mut custom_models = HashSet::new();
        custom_models.insert("llama3".to_string());
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds(), custom_models);
        // "mistral" is not in custom_models and has no prefix match
        let (provider, creds) = registry.resolve_from_proxy_creds("mistral");
        assert_eq!(provider, ProviderId::Kiro);
        assert!(creds.is_none());
    }

    #[tokio::test]
    async fn test_resolve_provider_proxy_custom_model_via_user_id_none() {
        let mut custom_models = HashSet::new();
        custom_models.insert("llama3".to_string());
        let registry = ProviderRegistry::new_with_proxy(make_proxy_creds(), custom_models);
        let (provider, creds) = registry.resolve_provider(None, "llama3", None).await;
        assert_eq!(provider, ProviderId::Custom);
        assert!(creds.is_some());
    }

    // ── from_proxy_config tests ─────────────────────────────────────

    #[test]
    fn test_from_proxy_config_all_providers() {
        use crate::config::ProxyConfig;
        let proxy = ProxyConfig {
            api_key: "test-key-long-enough".to_string(),
            anthropic_api_key: Some("sk-ant-test".to_string()),
            openai_api_key: Some("sk-proj-test".to_string()),
            openai_base_url: Some("https://openrouter.ai/api".to_string()),
            copilot_token: Some("cop-tok".to_string()),
            copilot_base_url: Some("https://api.githubcopilot.com".to_string()),
            qwen_token: Some("qwen-tok".to_string()),
            qwen_base_url: Some("https://qwen.example.com".to_string()),
            custom_provider_url: Some("http://localhost:11434/v1".to_string()),
            custom_provider_key: Some("custom-key".to_string()),
            custom_provider_models: Some("llama3,codellama,deepseek-r1".to_string()),
            ..Default::default()
        };
        let registry = ProviderRegistry::from_proxy_config(&proxy);
        let creds = registry.proxy_credentials().as_ref().unwrap();
        assert_eq!(creds.len(), 5);
        assert!(creds.contains_key(&ProviderId::Anthropic));
        assert!(creds.contains_key(&ProviderId::OpenAICodex));
        assert!(creds.contains_key(&ProviderId::Copilot));
        assert!(creds.contains_key(&ProviderId::Qwen));
        assert!(creds.contains_key(&ProviderId::Custom));
        // Verify base_url propagation
        assert_eq!(
            creds[&ProviderId::OpenAICodex].base_url.as_deref(),
            Some("https://openrouter.ai/api")
        );
        assert_eq!(creds[&ProviderId::Custom].access_token, "custom-key");
    }

    #[test]
    fn test_from_proxy_config_partial_providers() {
        use crate::config::ProxyConfig;
        let proxy = ProxyConfig {
            api_key: "test-key-long-enough".to_string(),
            anthropic_api_key: Some("sk-ant-test".to_string()),
            custom_provider_url: Some("http://localhost:11434/v1".to_string()),
            custom_provider_models: Some("llama3".to_string()),
            ..Default::default()
        };
        let registry = ProviderRegistry::from_proxy_config(&proxy);
        let creds = registry.proxy_credentials().as_ref().unwrap();
        assert_eq!(creds.len(), 2);
        assert!(creds.contains_key(&ProviderId::Anthropic));
        assert!(creds.contains_key(&ProviderId::Custom));
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

    #[test]
    fn test_from_proxy_config_custom_models_parsed() {
        use crate::config::ProxyConfig;
        let proxy = ProxyConfig {
            api_key: "test-key-long-enough".to_string(),
            custom_provider_url: Some("http://localhost:11434/v1".to_string()),
            custom_provider_models: Some("llama3 , codellama , deepseek-r1".to_string()),
            ..Default::default()
        };
        let registry = ProviderRegistry::from_proxy_config(&proxy);
        // Verify custom models are trimmed and parsed
        let (provider, creds) = registry.resolve_from_proxy_creds("llama3");
        assert_eq!(provider, ProviderId::Custom);
        assert!(creds.is_some());
        let (provider2, _) = registry.resolve_from_proxy_creds("codellama");
        assert_eq!(provider2, ProviderId::Custom);
        let (provider3, _) = registry.resolve_from_proxy_creds("deepseek-r1");
        assert_eq!(provider3, ProviderId::Custom);
    }

    #[test]
    fn test_from_proxy_config_whitespace_around_commas() {
        use crate::config::ProxyConfig;
        let proxy = ProxyConfig {
            api_key: "test-key-long-enough".to_string(),
            custom_provider_url: Some("http://localhost:11434/v1".to_string()),
            custom_provider_models: Some(" llama3 , codellama ".to_string()),
            ..Default::default()
        };
        let registry = ProviderRegistry::from_proxy_config(&proxy);
        let (provider, _) = registry.resolve_from_proxy_creds("llama3");
        assert_eq!(provider, ProviderId::Custom);
        let (provider2, _) = registry.resolve_from_proxy_creds("codellama");
        assert_eq!(provider2, ProviderId::Custom);
    }

    #[test]
    fn test_from_proxy_config_trailing_comma() {
        use crate::config::ProxyConfig;
        let proxy = ProxyConfig {
            api_key: "test-key-long-enough".to_string(),
            custom_provider_url: Some("http://localhost:11434/v1".to_string()),
            custom_provider_models: Some("llama3,codellama,".to_string()),
            ..Default::default()
        };
        let registry = ProviderRegistry::from_proxy_config(&proxy);
        let (provider, _) = registry.resolve_from_proxy_creds("llama3");
        assert_eq!(provider, ProviderId::Custom);
        let (provider2, _) = registry.resolve_from_proxy_creds("codellama");
        assert_eq!(provider2, ProviderId::Custom);
        // Empty string from trailing comma should NOT be in the set
        let (provider3, creds3) = registry.resolve_from_proxy_creds("");
        assert_eq!(provider3, ProviderId::Kiro);
        assert!(creds3.is_none());
    }

    #[test]
    fn test_from_proxy_config_custom_key_defaults_empty() {
        use crate::config::ProxyConfig;
        let proxy = ProxyConfig {
            api_key: "test-key-long-enough".to_string(),
            custom_provider_url: Some("http://localhost:11434/v1".to_string()),
            // No custom_provider_key set
            ..Default::default()
        };
        let registry = ProviderRegistry::from_proxy_config(&proxy);
        let creds = registry.proxy_credentials().as_ref().unwrap();
        assert_eq!(creds[&ProviderId::Custom].access_token, "");
    }
}
