use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use chrono::Utc;
use dashmap::DashMap;
use uuid::Uuid;

use std::sync::RwLock;

use crate::auth::AuthManager;
use crate::cache::ModelCache;
use crate::config::Config;
use crate::error::ApiError;
use crate::http_client::KiroHttpClient;
use crate::providers::registry::ProviderRegistry;
use crate::providers::ProviderMap;
use crate::resolver::ModelResolver;
use crate::web_ui::config_db::ConfigDb;
use crate::web_ui::provider_oauth::{ProviderOAuthPendingState, TokenExchanger};

/// Per-user Kiro credentials, injected into request extensions by auth middleware.
#[derive(Debug, Clone)]
pub struct UserKiroCreds {
    pub user_id: Uuid,
    pub access_token: String,
    #[allow(dead_code)]
    pub refresh_token: String,
    pub region: String,
}

/// Cached session information (in-memory, backed by DB).
// TODO: Replace `role: String` with a `Role` enum (Admin, User) with serde support.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub user_id: Uuid,
    pub email: String,
    pub role: String,
    pub expires_at: chrono::DateTime<Utc>,
    /// How the user authenticated: "google" or "password"
    pub auth_method: String,
    /// Whether TOTP two-factor authentication is enabled for this user
    pub totp_enabled: bool,
    /// Whether the user must change their password on next login
    pub must_change_password: bool,
}

/// Pending OAuth state for PKCE validation.
#[derive(Debug, Clone)]
pub struct OAuthPendingState {
    pub nonce: String,
    pub pkce_verifier: String,
    pub created_at: chrono::DateTime<Utc>,
    /// When set, this OAuth flow is for linking a Google account to an existing user,
    /// not for login. The callback will update google_linked instead of creating a session.
    pub linking_user_id: Option<Uuid>,
}

/// Sentinel user ID for proxy-only mode (all requests share this identity).
pub const PROXY_USER_ID: Uuid = Uuid::from_u128(0x0000_0001_0000_0000_0000_0000_0000_0001);

/// Application state shared across handlers.
///
/// Future refactoring: consider grouping related fields into sub-structs
/// (e.g., AuthState, CacheState, FeatureState) to keep AppState focused.
#[derive(Clone)]
pub struct AppState {
    // Proxy-only mode: pre-hashed API key for constant-time comparison
    pub proxy_api_key_hash: Option<[u8; 32]>,
    // Core services
    pub model_cache: ModelCache,
    pub auth_manager: Arc<tokio::sync::RwLock<AuthManager>>,
    pub http_client: Arc<KiroHttpClient>,
    #[allow(dead_code)]
    pub resolver: ModelResolver,
    pub config: Arc<RwLock<Config>>,
    pub setup_complete: Arc<AtomicBool>,
    pub config_db: Option<Arc<ConfigDb>>,
    // In-memory caches
    /// session_id → SessionInfo
    pub session_cache: Arc<DashMap<Uuid, SessionInfo>>,
    /// key_hash → (user_id, key_id)
    pub api_key_cache: Arc<DashMap<String, (Uuid, Uuid)>>,
    /// user_id → (access_token, region, cached_at)
    pub kiro_token_cache: Arc<DashMap<Uuid, (String, String, std::time::Instant)>>,
    /// state_param → OAuthPendingState (10-min TTL)
    pub oauth_pending: Arc<DashMap<String, OAuthPendingState>>,
    // Feature subsystems
    /// Guardrails engine for input/output validation (None when guardrails disabled or no DB)
    pub guardrails_engine: Option<Arc<crate::guardrails::engine::GuardrailsEngine>>,
    // Multi-provider support
    /// Routes requests to the right provider based on user API keys
    pub provider_registry: Arc<ProviderRegistry>,
    /// All non-Kiro providers, keyed by ProviderId
    pub providers: ProviderMap,
    // Provider OAuth relay
    /// Pending provider OAuth relay states (separate from Google SSO oauth_pending)
    pub provider_oauth_pending: Arc<DashMap<String, ProviderOAuthPendingState>>,
    /// Token exchanger for provider OAuth (mockable for tests)
    pub token_exchanger: Arc<dyn TokenExchanger>,
    /// Rate limiter for password login: email → (failure_count, first_failure_at)
    pub login_rate_limiter: Arc<DashMap<String, (u32, std::time::Instant)>>,
}

impl AppState {
    /// Get the config database or return an error.
    pub fn require_config_db(&self) -> Result<Arc<ConfigDb>, ApiError> {
        self.config_db
            .as_ref()
            .cloned()
            .ok_or_else(|| ApiError::ConfigError("Config database not available".to_string()))
    }

    /// Evict all cached data for a user (sessions, API keys, Kiro tokens).
    /// Call after role change or user deletion.
    #[allow(dead_code)]
    pub fn evict_user_caches(&self, user_id: Uuid) {
        self.session_cache.retain(|_, info| info.user_id != user_id);
        self.api_key_cache.retain(|_, (uid, _)| *uid != user_id);
        self.kiro_token_cache.remove(&user_id);
    }
}
