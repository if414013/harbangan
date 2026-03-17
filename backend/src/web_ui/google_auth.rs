use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, Request};
use axum::middleware::Next;
use axum::response::Response;
use axum::Json;
use chrono::Utc;
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::error::ApiError;
use crate::routes::{AppState, OAuthPendingState, SessionInfo};
use crate::web_ui::config_db::ConfigDb;

/// Cached OIDC provider metadata (fetched once on first use).
static OIDC_PROVIDER: OnceCell<CoreProviderMetadata> = OnceCell::const_new();
/// Cached OIDC HTTP client (reused across requests).
static OIDC_HTTP_CLIENT: OnceCell<openidconnect::reqwest::Client> = OnceCell::const_new();

/// Get or initialize the cached OIDC provider metadata.
async fn get_oidc_provider() -> Result<&'static CoreProviderMetadata, ApiError> {
    OIDC_PROVIDER
        .get_or_try_init(|| async {
            let http_client = get_oidc_http_client().await?;
            CoreProviderMetadata::discover_async(
                IssuerUrl::new("https://accounts.google.com".to_string()).map_err(|e| {
                    ApiError::Internal(anyhow::anyhow!("Invalid issuer URL: {}", e))
                })?,
                http_client,
            )
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("OIDC discovery failed: {}", e)))
        })
        .await
}

/// Get or initialize the cached OIDC HTTP client.
async fn get_oidc_http_client() -> Result<&'static openidconnect::reqwest::Client, ApiError> {
    OIDC_HTTP_CLIENT
        .get_or_try_init(|| async {
            openidconnect::reqwest::ClientBuilder::new()
                .build()
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("HTTP client error: {}", e)))
        })
        .await
}

/// Query parameters on the Google OAuth callback.
#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

/// Derive the origin (scheme + host) from the callback URL for CORS and cookie scope.
#[allow(dead_code)]
pub fn derive_origin(callback_url: &str) -> String {
    // e.g. "http://localhost:9001/_ui/api/auth/google/callback" → "http://localhost:9001"
    // Manual parsing to avoid adding the `url` crate dependency.
    if let Some(rest) = callback_url.strip_prefix("https://") {
        let authority = rest.split('/').next().unwrap_or("localhost");
        format!("https://{}", authority)
    } else if let Some(rest) = callback_url.strip_prefix("http://") {
        let authority = rest.split('/').next().unwrap_or("localhost");
        format!("http://{}", authority)
    } else {
        "http://localhost:9001".to_string()
    }
}

/// Whether the callback URL is a local development URL (skip Secure flag on cookies).
fn is_local_dev(callback_url: &str) -> bool {
    callback_url.starts_with("http://localhost") || callback_url.starts_with("http://127.0.0.1")
}

/// Build a session cookie header value.
pub(crate) fn session_cookie(session_id: Uuid, callback_url: &str) -> String {
    let secure = if is_local_dev(callback_url) {
        ""
    } else {
        " Secure;"
    };
    format!(
        "kgw_session={};{} HttpOnly; SameSite=Strict; Path=/_ui; Max-Age=86400",
        session_id, secure
    )
}

/// Build a CSRF cookie header value (non-HttpOnly so JS can read it).
pub(crate) fn csrf_cookie(token: &str, callback_url: &str) -> String {
    let secure = if is_local_dev(callback_url) {
        ""
    } else {
        " Secure;"
    };
    format!(
        "csrf_token={};{} SameSite=Strict; Path=/_ui; Max-Age=86400",
        token, secure
    )
}

/// Build a clear-cookie header to delete the session.
pub(crate) fn clear_session_cookie(callback_url: &str) -> String {
    let secure = if is_local_dev(callback_url) {
        ""
    } else {
        " Secure;"
    };
    format!(
        "kgw_session=deleted;{} HttpOnly; SameSite=Strict; Path=/_ui; Max-Age=0",
        secure
    )
}

pub(crate) fn clear_csrf_cookie(callback_url: &str) -> String {
    let secure = if is_local_dev(callback_url) {
        ""
    } else {
        " Secure;"
    };
    format!(
        "csrf_token=deleted;{} SameSite=Strict; Path=/_ui; Max-Age=0",
        secure
    )
}

/// Extract session_id from the kgw_session cookie.
pub(crate) fn extract_session_id(headers: &axum::http::HeaderMap) -> Option<Uuid> {
    let cookie_header = headers.get("cookie")?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("kgw_session=") {
            return value.parse::<Uuid>().ok();
        }
    }
    None
}

/// Extract CSRF token from the csrf_token cookie.
pub(crate) fn extract_csrf_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    let cookie_header = headers.get("cookie")?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("csrf_token=") {
            return Some(value.to_string());
        }
    }
    None
}

// ── Handlers ──────────────────────────────────────────────────────────

/// GET /_ui/api/auth/google — redirect to Google consent screen.
// TODO: Add rate limiting on OAuth endpoints (tower::limit) as future work.
pub async fn google_auth_redirect(State(state): State<AppState>) -> Result<Response, ApiError> {
    let (client_id, client_secret, callback_url) = {
        let config = state.config.read().unwrap_or_else(|p| p.into_inner());
        (
            config.google_client_id.clone(),
            config.google_client_secret.clone(),
            config.google_callback_url.clone(),
        )
    };

    if client_id.is_empty() || client_secret.is_empty() || callback_url.is_empty() {
        return Err(ApiError::ConfigError(
            "Google OAuth not configured (GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET, GOOGLE_CALLBACK_URL)".to_string(),
        ));
    }

    // OIDC discovery (cached)
    let provider_metadata = get_oidc_provider().await?;

    let oidc_client = CoreClient::from_provider_metadata(
        provider_metadata.clone(),
        ClientId::new(client_id),
        Some(ClientSecret::new(client_secret)),
    )
    .set_redirect_uri(
        RedirectUrl::new(callback_url)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Invalid redirect URL: {}", e)))?,
    );

    // Generate PKCE
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    // Generate authorization URL
    let (auth_url, csrf_token, nonce) = oidc_client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("email".to_string()))
        .add_scope(Scope::new("profile".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    // Cleanup expired entries first
    let now = Utc::now();
    state
        .oauth_pending
        .retain(|_, v| (now - v.created_at).num_minutes() < 10);

    // Cap at 10,000 pending states to prevent unbounded memory growth
    if state.oauth_pending.len() >= 10_000 {
        tracing::warn!("OAuth pending state limit reached (10,000), rejecting new auth request");
        return Err(ApiError::Internal(anyhow::anyhow!(
            "Too many pending OAuth requests. Please try again later."
        )));
    }

    // Store pending state in oauth_pending DashMap (10-min TTL)
    state.oauth_pending.insert(
        csrf_token.secret().clone(),
        OAuthPendingState {
            nonce: nonce.secret().clone(),
            pkce_verifier: pkce_verifier.secret().clone(),
            created_at: Utc::now(),
            linking_user_id: None,
        },
    );

    // Redirect
    Ok(Response::builder()
        .status(302)
        .header("Location", auth_url.to_string())
        .body(Body::empty())
        .unwrap())
}

/// GET /_ui/api/auth/google/link — start Google account linking (session-required).
/// Unlike google_auth_redirect, this stores the user_id in pending state so the
/// callback can distinguish linking from login.
pub async fn google_link_redirect(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Result<Response, ApiError> {
    let session = request
        .extensions()
        .get::<SessionInfo>()
        .cloned()
        .ok_or(ApiError::SessionExpired)?;

    let (client_id, client_secret, callback_url) = {
        let config = state.config.read().unwrap_or_else(|p| p.into_inner());
        (
            config.google_client_id.clone(),
            config.google_client_secret.clone(),
            config.google_callback_url.clone(),
        )
    };

    if client_id.is_empty() || client_secret.is_empty() || callback_url.is_empty() {
        return Err(ApiError::ConfigError(
            "Google OAuth not configured".to_string(),
        ));
    }

    let provider_metadata = get_oidc_provider().await?;
    let oidc_client = CoreClient::from_provider_metadata(
        provider_metadata.clone(),
        ClientId::new(client_id),
        Some(ClientSecret::new(client_secret)),
    )
    .set_redirect_uri(
        RedirectUrl::new(callback_url)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Invalid redirect URL: {}", e)))?,
    );

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let (auth_url, csrf_token, nonce) = oidc_client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("email".to_string()))
        .add_scope(Scope::new("profile".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    // Cleanup expired entries
    let now = Utc::now();
    state
        .oauth_pending
        .retain(|_, v| (now - v.created_at).num_minutes() < 10);

    if state.oauth_pending.len() >= 10_000 {
        return Err(ApiError::Internal(anyhow::anyhow!(
            "Too many pending OAuth requests. Please try again later."
        )));
    }

    state.oauth_pending.insert(
        csrf_token.secret().clone(),
        OAuthPendingState {
            nonce: nonce.secret().clone(),
            pkce_verifier: pkce_verifier.secret().clone(),
            created_at: Utc::now(),
            linking_user_id: Some(session.user_id),
        },
    );

    Ok(Response::builder()
        .status(302)
        .header("Location", auth_url.to_string())
        .body(Body::empty())
        .unwrap())
}

/// GET /_ui/api/auth/google/callback — Google redirects here after consent.
pub async fn google_auth_callback(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<CallbackQuery>,
) -> Result<Response, ApiError> {
    let (client_id, client_secret, callback_url) = {
        let config = state.config.read().unwrap_or_else(|p| p.into_inner());
        (
            config.google_client_id.clone(),
            config.google_client_secret.clone(),
            config.google_callback_url.clone(),
        )
    };

    // Handle user consent denial
    if let Some(ref err) = params.error {
        let error_type = if err == "access_denied" {
            "consent_denied"
        } else {
            "auth_failed"
        };
        return redirect_login_error(error_type);
    }

    // Validate required params
    let code = params
        .code
        .as_deref()
        .ok_or_else(|| ApiError::ValidationError("Missing code parameter".into()))?;
    let state_param = params
        .state
        .as_deref()
        .ok_or_else(|| ApiError::ValidationError("Missing state parameter".into()))?;

    // Look up and remove pending state
    let pending = state.oauth_pending.remove(state_param);
    let (_, pending) = pending
        .ok_or_else(|| {
            tracing::warn!("Invalid or expired OAuth state parameter");
            redirect_login_error_inner("invalid_state")
        })
        .map_err(|_| ApiError::ValidationError("invalid_state".into()))?;

    // Check TTL
    if (Utc::now() - pending.created_at).num_minutes() >= 10 {
        return redirect_login_error("invalid_state");
    }

    // OIDC discovery (cached) + token exchange
    let provider_metadata = get_oidc_provider().await?;
    let http_client = get_oidc_http_client().await?;

    let oidc_client = CoreClient::from_provider_metadata(
        provider_metadata.clone(),
        ClientId::new(client_id),
        Some(ClientSecret::new(client_secret)),
    )
    .set_redirect_uri(
        RedirectUrl::new(callback_url.clone())
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Invalid redirect URL: {}", e)))?,
    );

    // Exchange code for tokens
    let token_response = oidc_client
        .exchange_code(AuthorizationCode::new(code.to_string()))
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Code exchange config error: {}", e)))?
        .set_pkce_verifier(PkceCodeVerifier::new(pending.pkce_verifier))
        .request_async(http_client)
        .await
        .map_err(|e| {
            tracing::error!(error = ?e, "Token exchange failed");
            ApiError::Internal(anyhow::anyhow!("Token exchange failed: {}", e))
        })?;

    // Validate ID token
    let id_token = token_response
        .id_token()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Server did not return an ID token")))?;

    let nonce = Nonce::new(pending.nonce);
    let claims = id_token
        .claims(&oidc_client.id_token_verifier(), &nonce)
        .map_err(|e| {
            tracing::error!(error = ?e, "ID token validation failed");
            ApiError::Internal(anyhow::anyhow!("ID token validation failed: {}", e))
        })?;

    // Check email_verified
    let email_verified = claims.email_verified().unwrap_or(false);
    if !email_verified {
        return redirect_login_error("email_not_verified");
    }

    let email = claims
        .email()
        .map(|e| e.to_string())
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("No email in ID token")))?;

    // Extract name and picture
    let name = claims
        .name()
        .and_then(|n| n.get(None))
        .map(|n| n.to_string())
        .unwrap_or_else(|| email.clone());

    let picture = claims
        .picture()
        .and_then(|p| p.get(None))
        .map(|p| p.to_string());

    // Domain check
    let config_db = require_config_db(&state)?;
    let allowed = config_db
        .is_domain_allowed(&email)
        .await
        .map_err(ApiError::Internal)?;
    if !allowed {
        return redirect_login_error("domain_not_allowed");
    }

    // ── Branch: Google account linking vs. login ──
    if let Some(linking_uid) = pending.linking_user_id {
        // Linking flow: verify the Google email matches the user's email
        let user = config_db
            .get_user_by_email(&email)
            .await
            .map_err(ApiError::Internal)?;

        match user {
            Some((uid, ..)) if uid == linking_uid => {
                // Email matches — link the Google account
                config_db
                    .set_google_linked(linking_uid, true)
                    .await
                    .map_err(ApiError::Internal)?;

                tracing::info!(user_id = %linking_uid, email = %email, "Google account linked");

                return Ok(Response::builder()
                    .status(302)
                    .header("Location", "/_ui/profile")
                    .body(Body::empty())
                    .unwrap());
            }
            _ => {
                // Email mismatch — redirect with error
                return Ok(Response::builder()
                    .status(302)
                    .header("Location", "/_ui/profile?error=google_email_mismatch")
                    .body(Body::empty())
                    .unwrap());
            }
        }
    }

    // ── Normal login flow ──

    // Upsert user (first-user-admin logic is in DB query)
    let (user_id, role) = config_db
        .upsert_user(&email, &name, picture.as_deref())
        .await
        .map_err(ApiError::Internal)?;

    // Check if this is a password-method user trying to log in via Google
    // They must have explicitly linked their Google account first
    let user_auth = config_db
        .get_user_by_email_with_auth(&email)
        .await
        .map_err(ApiError::Internal)?;

    if let Some((_, _, _, _, _, _, _, ref auth_method, _)) = user_auth {
        if auth_method == "password" {
            let google_linked = config_db
                .get_google_linked(user_id)
                .await
                .map_err(ApiError::Internal)?;
            if !google_linked {
                return redirect_login_error("google_not_linked");
            }
        }
    }

    tracing::info!(user_id = %user_id, email = %email, role = %role, "User authenticated via Google SSO");

    // Create session
    let expires_at = Utc::now() + chrono::Duration::hours(24);
    let session_id = config_db
        .create_session(user_id, expires_at)
        .await
        .map_err(ApiError::Internal)?;

    // Cache session
    state.session_cache.insert(
        session_id,
        SessionInfo {
            user_id,
            email: email.clone(),
            role: role.clone(),
            expires_at,
            auth_method: "google".to_string(),
            totp_enabled: false,
            must_change_password: false,
        },
    );

    // Generate CSRF token
    let csrf_token = Uuid::new_v4().to_string();

    // Set cookies and redirect to UI
    Ok(Response::builder()
        .status(302)
        .header("Location", "/_ui/")
        .header("Set-Cookie", session_cookie(session_id, &callback_url))
        .header("Set-Cookie", csrf_cookie(&csrf_token, &callback_url))
        .body(Body::empty())
        .unwrap())
}

/// POST /_ui/api/auth/logout — with access to the raw request for session cleanup.
pub async fn logout_with_session(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Result<Response, ApiError> {
    let callback_url = state
        .config
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .google_callback_url
        .clone();

    // Extract session_id from cookie for cleanup
    if let Some(session_id) = extract_session_id(request.headers()) {
        state.session_cache.remove(&session_id);
        if let Some(ref db) = state.config_db {
            let _ = db.delete_session(session_id).await;
        }
    }

    Ok(Response::builder()
        .status(200)
        .header("Content-Type", "application/json")
        .header("Set-Cookie", clear_session_cookie(&callback_url))
        .header("Set-Cookie", clear_csrf_cookie(&callback_url))
        .body(Body::from(r#"{"ok":true}"#))
        .unwrap())
}

/// GET /_ui/api/auth/me — current user info.
pub async fn auth_me(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Session middleware puts SessionInfo in extensions
    let session = request
        .extensions()
        .get::<SessionInfo>()
        .cloned()
        .ok_or(ApiError::SessionExpired)?;

    // Check if user has Kiro creds configured
    let has_kiro_token = if let Some(ref db) = state.config_db {
        db.get_kiro_token(session.user_id)
            .await
            .map(|t| t.is_some())
            .unwrap_or(false)
    } else {
        false
    };

    // Check if user has linked their Google account
    let google_linked = if let Some(ref db) = state.config_db {
        db.get_google_linked(session.user_id).await.unwrap_or(false)
    } else {
        false
    };

    Ok(Json(json!({
        "user_id": session.user_id,
        "email": session.email,
        "role": session.role,
        "has_kiro_token": has_kiro_token,
        "auth_method": session.auth_method,
        "totp_enabled": session.totp_enabled,
        "must_change_password": session.must_change_password,
        "google_linked": google_linked,
    })))
}

/// GET /_ui/api/status — public endpoint, no auth required.
pub async fn status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let has_users = if let Some(ref db) = state.config_db {
        db.is_setup_complete().await
    } else {
        false
    };

    let google_configured = {
        let config = state.config.read().unwrap_or_else(|p| p.into_inner());
        !config.google_client_id.is_empty()
            && !config.google_client_secret.is_empty()
            && !config.google_callback_url.is_empty()
    };

    let auth_google_enabled = if let Some(ref db) = state.config_db {
        db.get("auth_google_enabled")
            .await
            .unwrap_or(None)
            .map(|v| v == "true")
            .unwrap_or(true)
    } else {
        true
    };
    let auth_password_enabled = if let Some(ref db) = state.config_db {
        db.get("auth_password_enabled")
            .await
            .unwrap_or(None)
            .map(|v| v == "true")
            .unwrap_or(true)
    } else {
        true
    };

    Json(json!({
        "setup_complete": has_users,
        "google_configured": google_configured,
        "auth_google_enabled": auth_google_enabled,
        "auth_password_enabled": auth_password_enabled,
    }))
}

// ── Middleware ─────────────────────────────────────────────────────────

/// Session middleware for web UI routes.
///
/// Validates the `kgw_session` cookie, resolves the session from cache (or DB fallback),
/// and injects `SessionInfo` into request extensions.
pub async fn session_middleware(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let session_id = extract_session_id(request.headers()).ok_or(ApiError::SessionExpired)?;

    // Try in-memory cache first
    if let Some(entry) = state.session_cache.get(&session_id) {
        if entry.expires_at > Utc::now() {
            request.extensions_mut().insert(entry.value().clone());
            return Ok(next.run(request).await);
        } else {
            // Expired — evict
            drop(entry);
            state.session_cache.remove(&session_id);
        }
    }

    // Fallback to DB
    let config_db = require_config_db(&state)?;
    let row = config_db
        .get_session(session_id)
        .await
        .map_err(ApiError::Internal)?;

    let (_session_id, user_id, expires_at) = row.ok_or(ApiError::SessionExpired)?;

    if expires_at <= Utc::now() {
        let _ = config_db.delete_session(session_id).await;
        return Err(ApiError::SessionExpired);
    }

    // Resolve user with auth fields
    let user_auth = config_db
        .get_user_by_email_with_auth_by_id(user_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::SessionExpired)?;

    let session_info = SessionInfo {
        user_id: user_auth.0,
        email: user_auth.1.clone(),
        role: user_auth.4.clone(),
        expires_at,
        auth_method: user_auth.7.clone(),
        totp_enabled: user_auth.6,
        must_change_password: user_auth.8,
    };

    // Cache it
    state.session_cache.insert(session_id, session_info.clone());

    // Sliding expiry: extend session if more than half its lifetime has passed
    let halfway = expires_at - chrono::Duration::hours(12);
    if Utc::now() > halfway {
        let new_expiry = Utc::now() + chrono::Duration::hours(24);
        let _ = config_db.extend_session(session_id, new_expiry).await;
    }

    request.extensions_mut().insert(session_info);
    Ok(next.run(request).await)
}

/// CSRF double-submit middleware.
///
/// On POST/PUT/DELETE, require `X-CSRF-Token` header matching the `csrf_token` cookie.
/// GET/HEAD/OPTIONS pass through.
pub async fn csrf_middleware(request: Request<Body>, next: Next) -> Result<Response, ApiError> {
    let method = request.method().clone();

    // Safe methods pass through
    if method == Method::GET || method == Method::HEAD || method == Method::OPTIONS {
        return Ok(next.run(request).await);
    }

    // Mutating methods require CSRF
    let csrf_cookie_val = extract_csrf_cookie(request.headers());
    let csrf_header = request
        .headers()
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    match (csrf_cookie_val, csrf_header) {
        (Some(cookie), Some(header)) if !cookie.is_empty() && cookie == header => {
            Ok(next.run(request).await)
        }
        _ => Err(ApiError::Forbidden("CSRF token mismatch".to_string())),
    }
}

/// Admin-only guard middleware. Must run AFTER session_middleware.
pub async fn admin_middleware(request: Request<Body>, next: Next) -> Result<Response, ApiError> {
    let session = request
        .extensions()
        .get::<SessionInfo>()
        .ok_or(ApiError::SessionExpired)?;

    if session.role != "admin" {
        return Err(ApiError::Forbidden("Admin access required".to_string()));
    }

    Ok(next.run(request).await)
}

// ── Helpers ───────────────────────────────────────────────────────────

fn require_config_db(state: &AppState) -> Result<Arc<ConfigDb>, ApiError> {
    state.require_config_db()
}

fn redirect_login_error(error: &str) -> Result<Response, ApiError> {
    Ok(redirect_login_error_inner(error))
}

fn redirect_login_error_inner(error: &str) -> Response {
    Response::builder()
        .status(302)
        .header(
            "Location",
            format!("/_ui/login?error={}", urlencoding::encode(error)),
        )
        .body(Body::empty())
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        auth::AuthManager, cache::ModelCache, config::Config, http_client::KiroHttpClient,
        resolver::ModelResolver, routes::SessionInfo,
    };
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        middleware as axum_middleware,
        routing::get,
        Router,
    };
    use std::collections::HashMap;
    use std::sync::atomic::AtomicBool;
    use tower::util::ServiceExt;

    fn create_test_state() -> AppState {
        let cache = ModelCache::new(3600);
        let http_client = Arc::new(KiroHttpClient::new(20, 30, 300, 3).unwrap());
        let auth_manager = Arc::new(tokio::sync::RwLock::new(
            AuthManager::new_for_testing("test-token".to_string(), "us-east-1".to_string(), 300)
                .unwrap(),
        ));
        let resolver = ModelResolver::new(cache.clone(), HashMap::new());
        let config = Config {
            fake_reasoning_max_tokens: 10000,
            ..Config::with_defaults()
        };

        let config_arc = Arc::new(std::sync::RwLock::new(config));

        AppState {
            proxy_api_key_hash: None,
            model_cache: cache,
            auth_manager: Arc::clone(&auth_manager),
            http_client: Arc::clone(&http_client),
            resolver,
            config: Arc::clone(&config_arc),
            setup_complete: Arc::new(AtomicBool::new(true)),
            config_db: None,
            session_cache: Arc::new(dashmap::DashMap::new()),
            api_key_cache: Arc::new(dashmap::DashMap::new()),
            kiro_token_cache: Arc::new(dashmap::DashMap::new()),
            oauth_pending: Arc::new(dashmap::DashMap::new()),
            guardrails_engine: None,
            provider_registry: Arc::new(crate::providers::registry::ProviderRegistry::new()),
            providers: crate::providers::build_provider_map(http_client, auth_manager, config_arc),
            provider_oauth_pending: Arc::new(dashmap::DashMap::new()),
            token_exchanger: Arc::new(crate::web_ui::provider_oauth::HttpTokenExchanger::new()),
            login_rate_limiter: Arc::new(dashmap::DashMap::new()),
            rate_tracker: Arc::new(crate::providers::rate_limiter::RateLimitTracker::new()),
        }
    }

    async fn test_handler() -> &'static str {
        "OK"
    }

    // ── Pure helper tests ────────────────────────────────────────────

    #[test]
    fn test_derive_origin() {
        assert_eq!(
            derive_origin("http://localhost:9001/_ui/api/auth/google/callback"),
            "http://localhost:9001"
        );
        assert_eq!(
            derive_origin("https://myapp.example.com/_ui/api/auth/google/callback"),
            "https://myapp.example.com"
        );
        assert_eq!(
            derive_origin("https://myapp.example.com:8443/_ui/api/auth/google/callback"),
            "https://myapp.example.com:8443"
        );
    }

    #[test]
    fn test_is_local_dev() {
        assert!(is_local_dev("http://localhost:9001/callback"));
        assert!(is_local_dev("http://127.0.0.1:9001/callback"));
        assert!(!is_local_dev("https://myapp.example.com/callback"));
    }

    #[test]
    fn test_session_cookie_local() {
        let cookie = session_cookie(Uuid::nil(), "http://localhost:9001/callback");
        assert!(!cookie.contains("Secure"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
    }

    #[test]
    fn test_session_cookie_production() {
        let cookie = session_cookie(Uuid::nil(), "https://myapp.example.com/callback");
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("HttpOnly"));
    }

    #[test]
    fn test_csrf_cookie_not_httponly() {
        let cookie = csrf_cookie("token123", "http://localhost:9001/callback");
        assert!(!cookie.contains("HttpOnly"));
    }

    #[test]
    fn test_extract_session_id() {
        let mut headers = axum::http::HeaderMap::new();
        let session_id = Uuid::new_v4();
        headers.insert(
            "cookie",
            format!("kgw_session={}; other=value", session_id)
                .parse()
                .unwrap(),
        );
        assert_eq!(extract_session_id(&headers), Some(session_id));
    }

    #[test]
    fn test_extract_session_id_missing() {
        let headers = axum::http::HeaderMap::new();
        assert_eq!(extract_session_id(&headers), None);
    }

    #[test]
    fn test_extract_csrf_cookie() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "cookie",
            "csrf_token=abc123; kgw_session=xxx".parse().unwrap(),
        );
        assert_eq!(extract_csrf_cookie(&headers), Some("abc123".to_string()));
    }

    // ── session_middleware tests ──────────────────────────────────────

    #[tokio::test]
    async fn test_session_middleware_valid_cached_session() {
        let state = create_test_state();
        let session_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let expires_at = Utc::now() + chrono::Duration::hours(12);

        state.session_cache.insert(
            session_id,
            SessionInfo {
                user_id,
                email: "test@example.com".to_string(),
                role: "admin".to_string(),
                expires_at,
                auth_method: "google".to_string(),
                totp_enabled: false,
                must_change_password: false,
            },
        );

        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(axum_middleware::from_fn_with_state(
                state.clone(),
                session_middleware,
            ))
            .with_state(state);

        let request = Request::builder()
            .uri("/test")
            .header("cookie", format!("kgw_session={}", session_id))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_session_middleware_expired_cached_session() {
        let state = create_test_state();
        let session_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let expires_at = Utc::now() - chrono::Duration::hours(1);

        state.session_cache.insert(
            session_id,
            SessionInfo {
                user_id,
                email: "test@example.com".to_string(),
                role: "user".to_string(),
                expires_at,
                auth_method: "google".to_string(),
                totp_enabled: false,
                must_change_password: false,
            },
        );

        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(axum_middleware::from_fn_with_state(
                state.clone(),
                session_middleware,
            ))
            .with_state(state.clone());

        let request = Request::builder()
            .uri("/test")
            .header("cookie", format!("kgw_session={}", session_id))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        // Expired session is evicted from cache, then middleware falls through to DB.
        // Without a DB, it returns 500 (ConfigError). The key assertion is that
        // the request is rejected (not 200) and the session is evicted from cache.
        assert_ne!(response.status(), StatusCode::OK);

        // Verify expired session was evicted from cache
        assert!(state.session_cache.get(&session_id).is_none());
    }

    #[tokio::test]
    async fn test_session_middleware_missing_cookie() {
        let state = create_test_state();

        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(axum_middleware::from_fn_with_state(
                state.clone(),
                session_middleware,
            ))
            .with_state(state);

        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_session_middleware_invalid_session_id() {
        let state = create_test_state();

        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(axum_middleware::from_fn_with_state(
                state.clone(),
                session_middleware,
            ))
            .with_state(state);

        let request = Request::builder()
            .uri("/test")
            .header("cookie", "kgw_session=not-a-valid-uuid")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // ── csrf_middleware tests ─────────────────────────────────────────

    #[tokio::test]
    async fn test_csrf_middleware_get_passes_without_token() {
        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(axum_middleware::from_fn(csrf_middleware));

        let request = Request::builder()
            .method("GET")
            .uri("/test")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_csrf_middleware_post_missing_header() {
        let app = Router::new()
            .route("/test", axum::routing::post(test_handler))
            .layer(axum_middleware::from_fn(csrf_middleware));

        let request = Request::builder()
            .method("POST")
            .uri("/test")
            .header("cookie", "csrf_token=mytoken")
            // No X-CSRF-Token header
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_csrf_middleware_post_mismatched_tokens() {
        let app = Router::new()
            .route("/test", axum::routing::post(test_handler))
            .layer(axum_middleware::from_fn(csrf_middleware));

        let request = Request::builder()
            .method("POST")
            .uri("/test")
            .header("cookie", "csrf_token=cookie-token")
            .header("x-csrf-token", "header-token")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_csrf_middleware_post_matching_tokens() {
        let app = Router::new()
            .route("/test", axum::routing::post(test_handler))
            .layer(axum_middleware::from_fn(csrf_middleware));

        let request = Request::builder()
            .method("POST")
            .uri("/test")
            .header("cookie", "csrf_token=matching-token")
            .header("x-csrf-token", "matching-token")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // ── admin_middleware tests ────────────────────────────────────────

    #[tokio::test]
    async fn test_admin_middleware_admin_passes() {
        // admin_middleware reads SessionInfo from extensions.
        // We need to inject it before admin_middleware runs.
        async fn inject_admin(
            mut request: Request<Body>,
            next: axum::middleware::Next,
        ) -> Response {
            request.extensions_mut().insert(SessionInfo {
                user_id: Uuid::new_v4(),
                email: "admin@example.com".to_string(),
                role: "admin".to_string(),
                expires_at: Utc::now() + chrono::Duration::hours(12),
                auth_method: "google".to_string(),
                totp_enabled: false,
                must_change_password: false,
            });
            next.run(request).await
        }

        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(axum_middleware::from_fn(admin_middleware))
            .layer(axum_middleware::from_fn(inject_admin));

        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_admin_middleware_user_rejected() {
        async fn inject_user(mut request: Request<Body>, next: axum::middleware::Next) -> Response {
            request.extensions_mut().insert(SessionInfo {
                user_id: Uuid::new_v4(),
                email: "user@example.com".to_string(),
                role: "user".to_string(),
                expires_at: Utc::now() + chrono::Duration::hours(12),
                auth_method: "google".to_string(),
                totp_enabled: false,
                must_change_password: false,
            });
            next.run(request).await
        }

        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(axum_middleware::from_fn(admin_middleware))
            .layer(axum_middleware::from_fn(inject_user));

        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_admin_middleware_missing_session() {
        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(axum_middleware::from_fn(admin_middleware));

        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // ── status endpoint test ─────────────────────────────────────────

    #[tokio::test]
    async fn test_status_returns_setup_complete() {
        let state = create_test_state();

        let result = status(State(state)).await;
        let json = result.0;

        // No config_db → setup_complete defaults to false
        assert_eq!(json["setup_complete"], false);
        // No Google SSO configured → google_configured is false
        assert_eq!(json["google_configured"], false);
    }

    // ── auth_me test ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_auth_me_returns_user_info() {
        let state = create_test_state();
        let user_id = Uuid::new_v4();

        // Build a request with SessionInfo in extensions
        let mut request = Request::builder()
            .uri("/_ui/api/auth/me")
            .body(Body::empty())
            .unwrap();

        request.extensions_mut().insert(SessionInfo {
            user_id,
            email: "alice@example.com".to_string(),
            role: "admin".to_string(),
            expires_at: Utc::now() + chrono::Duration::hours(12),
            auth_method: "google".to_string(),
            totp_enabled: false,
            must_change_password: false,
        });

        let result = auth_me(State(state), request).await;
        assert!(result.is_ok());

        let json = result.unwrap().0;
        assert_eq!(json["email"], "alice@example.com");
        assert_eq!(json["role"], "admin");
        assert_eq!(json["user_id"], user_id.to_string());
        assert_eq!(json["has_kiro_token"], false);
    }
}
