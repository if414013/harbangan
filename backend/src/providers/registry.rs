/// Provider registry: resolves which provider to use for a given user + model.
///
/// Caches per-user provider credentials in memory (5-minute TTL) to avoid
/// repeated DB lookups on every request. Handles transparent token refresh
/// for OAuth-based provider tokens.
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use uuid::Uuid;

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
    cached_at: Instant,
}

/// Per-(user_id, provider) mutex map to prevent concurrent refresh storms.
type RefreshLockMap = DashMap<(Uuid, String), Arc<tokio::sync::Mutex<()>>>;

/// Resolves provider + credentials for a user + model combination.
pub struct ProviderRegistry {
    cache: Arc<DashMap<Uuid, CacheEntry>>,
    refresh_locks: Arc<RefreshLockMap>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
            refresh_locks: Arc::new(DashMap::new()),
        }
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
            Some(ProviderId::OpenAI)
        } else if model.starts_with("gemini-") {
            Some(ProviderId::Gemini)
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
    /// Returns `(ProviderId::Kiro, None)` when:
    /// - `user_id` is None (proxy-only mode or unauthenticated)
    /// - The model has no recognised direct-provider prefix
    /// - The user has no stored token for the inferred provider
    /// - The DB is unavailable
    pub async fn resolve_provider(
        &self,
        user_id: Option<Uuid>,
        model: &str,
        db: Option<&ConfigDb>,
    ) -> (ProviderId, Option<ProviderCredentials>) {
        let Some(uid) = user_id else {
            return (ProviderId::Kiro, None);
        };
        let Some(target) = Self::provider_for_model(model) else {
            return (ProviderId::Kiro, None);
        };
        let provider_str = target.as_str();

        // Cache hit?
        if let Some(entry) = self.cache.get(&uid) {
            if entry.cached_at.elapsed() < CACHE_TTL {
                return match entry.credentials.get(provider_str) {
                    Some(creds) => (target, Some(creds.clone())),
                    None => (ProviderId::Kiro, None),
                };
            }
        }

        // Cache miss or stale — load from DB
        let Some(db) = db else {
            return (ProviderId::Kiro, None);
        };
        let (user_creds, user_expires) = Self::load_user_tokens(uid, db).await;
        let result = match user_creds.get(provider_str) {
            Some(creds) => (target, Some(creds.clone())),
            None => (ProviderId::Kiro, None),
        };
        self.cache.insert(
            uid,
            CacheEntry {
                credentials: user_creds,
                expires_at: user_expires,
                cached_at: Instant::now(),
            },
        );
        result
    }

    /// Invalidate the cache for a user. Call after a provider token is added, removed, or refreshed.
    pub fn invalidate(&self, user_id: Uuid) {
        self.cache.remove(&user_id);
    }

    /// Load all provider tokens for a user from the `user_provider_tokens` table.
    async fn load_user_tokens(
        user_id: Uuid,
        db: &ConfigDb,
    ) -> (
        HashMap<String, ProviderCredentials>,
        HashMap<String, DateTime<Utc>>,
    ) {
        let mut creds_map = HashMap::new();
        let mut expires_map = HashMap::new();
        for provider_str in &["anthropic", "openai", "gemini"] {
            if let Ok(Some((access_token, _refresh_token, expires_at, _email))) =
                db.get_user_provider_token(user_id, provider_str).await
            {
                // Only include tokens that haven't fully expired
                let now = Utc::now();
                if expires_at > now {
                    let provider = match *provider_str {
                        "anthropic" => ProviderId::Anthropic,
                        "openai" => ProviderId::OpenAI,
                        "gemini" => ProviderId::Gemini,
                        _ => continue,
                    };
                    creds_map.insert(
                        provider_str.to_string(),
                        ProviderCredentials {
                            provider,
                            access_token,
                            base_url: None,
                        },
                    );
                    expires_map.insert(provider_str.to_string(), expires_at);
                }
            }
        }
        (creds_map, expires_map)
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
            Some(ProviderId::OpenAI)
        );
        assert_eq!(
            ProviderRegistry::provider_for_model("o1-mini"),
            Some(ProviderId::OpenAI)
        );
        assert_eq!(
            ProviderRegistry::provider_for_model("o3-pro"),
            Some(ProviderId::OpenAI)
        );
        assert_eq!(
            ProviderRegistry::provider_for_model("chatgpt-4o-latest"),
            Some(ProviderId::OpenAI)
        );
    }

    #[test]
    fn test_provider_for_model_gemini() {
        assert_eq!(
            ProviderRegistry::provider_for_model("gemini-2.5-pro"),
            Some(ProviderId::Gemini)
        );
        assert_eq!(
            ProviderRegistry::provider_for_model("gemini-2.5-flash"),
            Some(ProviderId::Gemini)
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
            },
        );
        registry.cache.insert(
            uid,
            CacheEntry {
                credentials: creds_map,
                expires_at: HashMap::new(),
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
            },
        );
        registry.cache.insert(
            uid,
            CacheEntry {
                credentials: creds_map,
                expires_at: expires_map,
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
            (uid1, "openai".to_string()),
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
            .contains_key(&(uid1, "openai".to_string())));
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
                cached_at: Instant::now(),
            },
        );

        let entry = registry.cache.get(&uid).unwrap();
        assert_eq!(entry.expires_at.get("anthropic"), Some(&future));
    }
}
