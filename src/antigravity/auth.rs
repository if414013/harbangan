//! Antigravity Google OAuth 2.0 authentication module.
//!
//! Implements PKCE-based OAuth flow and per-account token caching
//! for the Cloud Code API.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use dashmap::DashMap;
use sha2::{Digest, Sha256};
use thiserror::Error;

use super::constants::{
    OAUTH_AUTH_URL, OAUTH_CALLBACK_PORT, OAUTH_CLIENT_ID, OAUTH_CLIENT_SECRET, OAUTH_SCOPES,
    OAUTH_TOKEN_URL, OAUTH_USER_INFO_URL,
};

// === Error Types ===

#[derive(Error, Debug)]
pub enum AuthError {
    #[error("Token refresh failed: {0}")]
    RefreshFailed(String),

    #[error("Token exchange failed: {0}")]
    ExchangeFailed(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Account invalid: {email}: {reason}")]
    AccountInvalid { email: String, reason: String },
}

// === PKCE ===

/// PKCE code verifier and challenge pair.
#[derive(Debug, Clone)]
pub struct PkceChallenge {
    pub verifier: String,
    pub challenge: String,
}

/// Generates a PKCE code verifier (32 random bytes, base64url) and
/// its SHA-256 challenge.
pub fn generate_pkce() -> PkceChallenge {
    let mut buf = [0u8; 32];
    random_bytes(&mut buf);
    let verifier = URL_SAFE_NO_PAD.encode(buf);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    PkceChallenge {
        verifier,
        challenge,
    }
}

/// Fill buffer with cryptographically secure random bytes.
fn random_bytes(buf: &mut [u8]) {
    getrandom::getrandom(buf).expect("failed to generate random bytes");
}

// === Authorization URL ===

/// Builds the Google OAuth authorization URL with PKCE.
///
/// Returns `(url, pkce, state)`.
pub fn get_authorization_url(custom_redirect_uri: Option<&str>) -> (String, PkceChallenge, String) {
    let pkce = generate_pkce();
    let state = {
        let mut buf = [0u8; 16];
        random_bytes(&mut buf);
        hex::encode(buf)
    };

    let redirect_uri = custom_redirect_uri
        .map(String::from)
        .unwrap_or_else(|| format!("http://localhost:{}/oauth-callback", OAUTH_CALLBACK_PORT));

    let scopes = OAUTH_SCOPES.join(" ");

    let params = [
        ("client_id", OAUTH_CLIENT_ID.as_str()),
        ("redirect_uri", &redirect_uri),
        ("response_type", "code"),
        ("scope", &scopes),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("code_challenge", &pkce.challenge),
        ("code_challenge_method", "S256"),
        ("state", &state),
    ];

    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let url = format!("{}?{}", OAUTH_AUTH_URL, query);
    (url, pkce, state)
}

// === Composite Refresh Token ===

/// Parsed parts of a composite refresh token.
///
/// Format: `refreshToken|projectId|managedProjectId`
#[derive(Debug, Clone, Default)]
pub struct RefreshParts {
    pub refresh_token: String,
    pub project_id: Option<String>,
    pub managed_project_id: Option<String>,
}

/// Parses a composite refresh token string into its parts.
pub fn parse_refresh_parts(composite: &str) -> RefreshParts {
    let mut parts = composite.splitn(3, '|');
    let refresh_token = parts.next().unwrap_or("").to_string();
    let project_id = parts.next().and_then(|s| {
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    });
    let managed_project_id = parts.next().and_then(|s| {
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    });
    RefreshParts {
        refresh_token,
        project_id,
        managed_project_id,
    }
}

/// Formats refresh parts back into a composite string.
pub fn format_refresh_parts(parts: &RefreshParts) -> String {
    let project_segment = parts.project_id.as_deref().unwrap_or("");
    let base = format!("{}|{}", parts.refresh_token, project_segment);
    match &parts.managed_project_id {
        Some(managed) => format!("{}|{}", base, managed),
        None => base,
    }
}

// === Token Exchange ===

/// Tokens returned from an OAuth token exchange.
#[derive(Clone)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
}

impl std::fmt::Debug for OAuthTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthTokens")
            .field("access_token", &"[REDACTED]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

/// Exchanges an authorization code + PKCE verifier for OAuth tokens.
pub async fn exchange_code(
    client: &reqwest::Client,
    code: &str,
    verifier: &str,
    redirect_uri: Option<&str>,
) -> Result<OAuthTokens, AuthError> {
    let redirect = redirect_uri
        .map(String::from)
        .unwrap_or_else(|| format!("http://localhost:{}/oauth-callback", OAUTH_CALLBACK_PORT));

    let params = [
        ("client_id", OAUTH_CLIENT_ID.as_str()),
        ("client_secret", OAUTH_CLIENT_SECRET.as_str()),
        ("code", code),
        ("code_verifier", verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", &redirect),
    ];

    let response = client
        .post(OAUTH_TOKEN_URL)
        .form(&params)
        .send()
        .await
        .map_err(|e| AuthError::Network(e.to_string()))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AuthError::ExchangeFailed(format!(
            "HTTP {}: {}",
            status.as_u16(),
            body
        )));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AuthError::ExchangeFailed(e.to_string()))?;

    let access_token = json["access_token"]
        .as_str()
        .ok_or_else(|| AuthError::ExchangeFailed("No access_token in response".into()))?
        .to_string();

    let refresh_token = json["refresh_token"].as_str().map(String::from);
    let expires_in = json["expires_in"].as_u64();

    Ok(OAuthTokens {
        access_token,
        refresh_token,
        expires_in,
    })
}

/// Refreshes an access token using a composite refresh token.
///
/// Parses the composite token to extract the actual OAuth refresh token,
/// then exchanges it for a new access token.
pub async fn refresh_access_token(
    client: &reqwest::Client,
    composite_refresh: &str,
) -> Result<OAuthTokens, AuthError> {
    let parts = parse_refresh_parts(composite_refresh);

    let params = [
        ("client_id", OAUTH_CLIENT_ID.as_str()),
        ("client_secret", OAUTH_CLIENT_SECRET.as_str()),
        ("refresh_token", parts.refresh_token.as_str()),
        ("grant_type", "refresh_token"),
    ];

    let response = client
        .post(OAUTH_TOKEN_URL)
        .form(&params)
        .send()
        .await
        .map_err(|e| AuthError::Network(e.to_string()))?;

    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(AuthError::RefreshFailed(body));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AuthError::RefreshFailed(e.to_string()))?;

    let access_token = json["access_token"]
        .as_str()
        .ok_or_else(|| AuthError::RefreshFailed("No access_token in response".into()))?
        .to_string();

    let expires_in = json["expires_in"].as_u64();

    Ok(OAuthTokens {
        access_token,
        refresh_token: None, // Refresh grants don't return a new refresh token
        expires_in,
    })
}

// === User Info ===

/// Fetches the user's email address from the Google userinfo endpoint.
pub async fn get_user_email(client: &reqwest::Client, access_token: &str) -> Result<String> {
    let response = client
        .get(OAUTH_USER_INFO_URL)
        .bearer_auth(access_token)
        .send()
        .await
        .context("Failed to fetch user info")?;

    if !response.status().is_success() {
        anyhow::bail!(
            "Failed to get user info: HTTP {}",
            response.status().as_u16()
        );
    }

    let json: serde_json::Value = response.json().await.context("Failed to parse user info")?;
    json["email"]
        .as_str()
        .map(String::from)
        .context("No email in user info response")
}

// === Token Cache ===

/// A cached access token with its expiry time.
#[derive(Debug, Clone)]
struct CachedToken {
    access_token: String,
    fetched_at: Instant,
}

/// Thread-safe per-account token manager.
///
/// Caches access tokens keyed by account email with a configurable TTL.
/// Uses `DashMap` for lock-free concurrent access.
pub struct AntigravityTokenManager {
    /// Cached tokens keyed by account email.
    cache: DashMap<String, CachedToken>,
    /// Token TTL (default 5 minutes).
    ttl: Duration,
    /// HTTP client for token refresh requests.
    client: reqwest::Client,
}

impl AntigravityTokenManager {
    /// Creates a new token manager with the given TTL.
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: DashMap::new(),
            ttl,
            client: reqwest::Client::new(),
        }
    }

    /// Creates a new token manager with the default 5-minute TTL.
    pub fn with_default_ttl() -> Self {
        Self::new(Duration::from_secs(5 * 60))
    }

    /// Returns a valid access token for the given account, refreshing if needed.
    ///
    /// If a cached token exists and hasn't expired, returns it immediately.
    /// Otherwise, uses the composite refresh token to obtain a new access token.
    pub async fn get_access_token(
        &self,
        email: &str,
        composite_refresh: &str,
    ) -> Result<String, AuthError> {
        // Check cache
        if let Some(entry) = self.cache.get(email) {
            if entry.fetched_at.elapsed() < self.ttl {
                return Ok(entry.access_token.clone());
            }
        }

        // Refresh
        let tokens = refresh_access_token(&self.client, composite_refresh).await?;

        self.cache.insert(
            email.to_string(),
            CachedToken {
                access_token: tokens.access_token.clone(),
                fetched_at: Instant::now(),
            },
        );

        Ok(tokens.access_token)
    }

    /// Inserts a pre-fetched token into the cache (e.g., after initial OAuth exchange).
    pub fn insert_token(&self, email: &str, access_token: &str) {
        self.cache.insert(
            email.to_string(),
            CachedToken {
                access_token: access_token.to_string(),
                fetched_at: Instant::now(),
            },
        );
    }

    /// Invalidates the cached token for an account.
    pub fn invalidate(&self, email: &str) {
        self.cache.remove(email);
    }

    /// Invalidates all cached tokens.
    pub fn invalidate_all(&self) {
        self.cache.clear();
    }

    /// Extracts the project ID from a composite refresh token.
    pub fn get_project_id(composite_refresh: &str) -> Option<String> {
        let parts = parse_refresh_parts(composite_refresh);
        parts.managed_project_id.or(parts.project_id)
    }

    /// Returns the number of cached tokens.
    pub fn cached_count(&self) -> usize {
        self.cache.len()
    }
}

impl std::fmt::Debug for AntigravityTokenManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AntigravityTokenManager")
            .field("cached_accounts", &self.cache.len())
            .field("ttl", &self.ttl)
            .finish()
    }
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_pkce() {
        let pkce = generate_pkce();
        // Verifier should be base64url-encoded 32 bytes
        assert!(!pkce.verifier.is_empty());
        assert!(!pkce.challenge.is_empty());
        assert_ne!(pkce.verifier, pkce.challenge);

        // Verify the challenge is SHA256 of verifier
        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(pkce.verifier.as_bytes()));
        assert_eq!(pkce.challenge, expected);
    }

    #[test]
    fn test_generate_pkce_uniqueness() {
        let a = generate_pkce();
        let b = generate_pkce();
        assert_ne!(a.verifier, b.verifier);
        assert_ne!(a.challenge, b.challenge);
    }

    #[test]
    fn test_parse_refresh_parts_full() {
        let parts = parse_refresh_parts("refresh123|project456|managed789");
        assert_eq!(parts.refresh_token, "refresh123");
        assert_eq!(parts.project_id.as_deref(), Some("project456"));
        assert_eq!(parts.managed_project_id.as_deref(), Some("managed789"));
    }

    #[test]
    fn test_parse_refresh_parts_token_and_project() {
        let parts = parse_refresh_parts("refresh123|project456");
        assert_eq!(parts.refresh_token, "refresh123");
        assert_eq!(parts.project_id.as_deref(), Some("project456"));
        assert!(parts.managed_project_id.is_none());
    }

    #[test]
    fn test_parse_refresh_parts_token_only() {
        let parts = parse_refresh_parts("refresh123");
        assert_eq!(parts.refresh_token, "refresh123");
        assert!(parts.project_id.is_none());
        assert!(parts.managed_project_id.is_none());
    }

    #[test]
    fn test_parse_refresh_parts_empty_segments() {
        let parts = parse_refresh_parts("refresh123||managed789");
        assert_eq!(parts.refresh_token, "refresh123");
        assert!(parts.project_id.is_none());
        assert_eq!(parts.managed_project_id.as_deref(), Some("managed789"));
    }

    #[test]
    fn test_parse_refresh_parts_empty_string() {
        let parts = parse_refresh_parts("");
        assert_eq!(parts.refresh_token, "");
        assert!(parts.project_id.is_none());
        assert!(parts.managed_project_id.is_none());
    }

    #[test]
    fn test_format_refresh_parts_full() {
        let parts = RefreshParts {
            refresh_token: "refresh123".into(),
            project_id: Some("project456".into()),
            managed_project_id: Some("managed789".into()),
        };
        assert_eq!(
            format_refresh_parts(&parts),
            "refresh123|project456|managed789"
        );
    }

    #[test]
    fn test_format_refresh_parts_no_managed() {
        let parts = RefreshParts {
            refresh_token: "refresh123".into(),
            project_id: Some("project456".into()),
            managed_project_id: None,
        };
        assert_eq!(format_refresh_parts(&parts), "refresh123|project456");
    }

    #[test]
    fn test_format_refresh_parts_token_only() {
        let parts = RefreshParts {
            refresh_token: "refresh123".into(),
            project_id: None,
            managed_project_id: None,
        };
        assert_eq!(format_refresh_parts(&parts), "refresh123|");
    }

    #[test]
    fn test_parse_format_roundtrip() {
        let original = "refresh123|project456|managed789";
        let parts = parse_refresh_parts(original);
        let formatted = format_refresh_parts(&parts);
        assert_eq!(formatted, original);
    }

    #[test]
    fn test_get_authorization_url_contains_required_params() {
        let (url, pkce, state) = get_authorization_url(None);
        assert!(url.starts_with(OAUTH_AUTH_URL));
        assert!(url.contains("client_id="));
        assert!(url.contains("redirect_uri="));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("scope="));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("prompt=consent"));
        assert!(url.contains("code_challenge="));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains(&format!("state={}", state)));
        assert!(url.contains(&pkce.challenge));
    }

    #[test]
    fn test_get_authorization_url_custom_redirect() {
        let (url, _, _) = get_authorization_url(Some("http://custom:9999/callback"));
        assert!(url.contains("custom"));
        assert!(url.contains("9999"));
    }

    #[test]
    fn test_get_authorization_url_default_redirect() {
        let (url, _, _) = get_authorization_url(None);
        let expected_port = OAUTH_CALLBACK_PORT.to_string();
        assert!(url.contains(&expected_port));
        assert!(url.contains("oauth-callback"));
    }

    #[test]
    fn test_token_manager_insert_and_get_project_id() {
        let manager = AntigravityTokenManager::with_default_ttl();
        manager.insert_token("user@example.com", "test-token");
        assert_eq!(manager.cached_count(), 1);

        // Test project ID extraction
        assert_eq!(
            AntigravityTokenManager::get_project_id("refresh|proj1|managed1"),
            Some("managed1".into())
        );
        assert_eq!(
            AntigravityTokenManager::get_project_id("refresh|proj1"),
            Some("proj1".into())
        );
        assert_eq!(AntigravityTokenManager::get_project_id("refresh"), None);
    }

    #[test]
    fn test_token_manager_invalidate() {
        let manager = AntigravityTokenManager::with_default_ttl();
        manager.insert_token("a@test.com", "token-a");
        manager.insert_token("b@test.com", "token-b");
        assert_eq!(manager.cached_count(), 2);

        manager.invalidate("a@test.com");
        assert_eq!(manager.cached_count(), 1);

        manager.invalidate_all();
        assert_eq!(manager.cached_count(), 0);
    }

    #[test]
    fn test_token_manager_ttl_expiry() {
        let manager = AntigravityTokenManager::new(Duration::from_millis(0));
        manager.insert_token("user@test.com", "token");

        // With 0ms TTL, the token should be considered expired immediately
        // We can't call get_access_token without a real refresh token,
        // but we can verify the cache entry exists
        assert_eq!(manager.cached_count(), 1);
    }

    #[test]
    fn test_token_manager_debug() {
        let manager = AntigravityTokenManager::with_default_ttl();
        let debug = format!("{:?}", manager);
        assert!(debug.contains("AntigravityTokenManager"));
        assert!(debug.contains("cached_accounts"));
    }
}
