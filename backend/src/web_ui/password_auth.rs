use std::sync::Arc;
use std::time::Instant;

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::body::Body;
use axum::extract::State;
use axum::http::Request;
use axum::response::Response;
use axum::Json;
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;

use crate::error::ApiError;
use crate::routes::{AppState, SessionInfo};
use crate::web_ui::config_db::ConfigDb;
use crate::web_ui::google_auth;

/// Maximum failed login attempts before lockout.
const MAX_LOGIN_ATTEMPTS: u32 = 5;
/// Lockout window in seconds (15 minutes).
const LOCKOUT_WINDOW_SECS: u64 = 900;

/// Hash a plaintext password using Argon2id.
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?;
    Ok(hash.to_string())
}

/// Verify a plaintext password against an Argon2id hash.
pub fn verify_password(password: &str, hash: &str) -> anyhow::Result<bool> {
    let parsed =
        PasswordHash::new(hash).map_err(|e| anyhow::anyhow!("Invalid password hash: {}", e))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

/// Generate a SHA-256 hash of a recovery code for storage.
fn hash_recovery_code(code: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(code.as_bytes());
    hex::encode(hasher.finalize())
}

/// Generate random alphanumeric recovery codes.
fn generate_recovery_codes(count: usize) -> Vec<String> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..count)
        .map(|_| {
            (0..8)
                .map(|_| {
                    let idx = rng.gen_range(0..36);
                    if idx < 10 {
                        (b'0' + idx) as char
                    } else {
                        (b'a' + idx - 10) as char
                    }
                })
                .collect()
        })
        .collect()
}

/// Check the login rate limiter for an email. Returns an error if locked out.
fn check_rate_limit(state: &AppState, email: &str) -> Result<(), ApiError> {
    if let Some(entry) = state.login_rate_limiter.get(email) {
        let (count, first_failure) = entry.value();
        if *count >= MAX_LOGIN_ATTEMPTS {
            let elapsed = first_failure.elapsed().as_secs();
            if elapsed < LOCKOUT_WINDOW_SECS {
                return Err(ApiError::AccountLocked {
                    retry_after_secs: LOCKOUT_WINDOW_SECS - elapsed,
                });
            }
            // Window expired — will be cleared on next success
        }
    }
    Ok(())
}

/// Record a failed login attempt.
fn record_login_failure(state: &AppState, email: &str) {
    state
        .login_rate_limiter
        .entry(email.to_string())
        .and_modify(|(count, first_failure)| {
            if first_failure.elapsed().as_secs() >= LOCKOUT_WINDOW_SECS {
                // Reset if window has expired
                *count = 1;
                *first_failure = Instant::now();
            } else {
                *count += 1;
            }
        })
        .or_insert((1, Instant::now()));
}

/// Clear rate limiter on successful login.
fn clear_rate_limit(state: &AppState, email: &str) {
    state.login_rate_limiter.remove(email);
}

/// Create a session and return response with cookies.
#[allow(clippy::too_many_arguments)]
async fn create_session_response(
    state: &AppState,
    db: &Arc<ConfigDb>,
    user_id: Uuid,
    email: &str,
    role: &str,
    auth_method: &str,
    totp_enabled: bool,
    must_change_password: bool,
) -> Result<Response, ApiError> {
    let callback_url = state
        .config
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .google_callback_url
        .clone();

    let expires_at = Utc::now() + chrono::Duration::hours(24);
    let session_id = db
        .create_session(user_id, expires_at)
        .await
        .map_err(ApiError::Internal)?;

    // Cache session
    state.session_cache.insert(
        session_id,
        SessionInfo {
            user_id,
            email: email.to_string(),
            role: role.to_string(),
            expires_at,
            auth_method: auth_method.to_string(),
            totp_enabled,
            must_change_password,
        },
    );

    // Generate CSRF token
    let csrf_token = Uuid::new_v4().to_string();

    Ok(Response::builder()
        .status(200)
        .header("Content-Type", "application/json")
        .header(
            "Set-Cookie",
            google_auth::session_cookie(session_id, &callback_url),
        )
        .header(
            "Set-Cookie",
            google_auth::csrf_cookie(&csrf_token, &callback_url),
        )
        .body(Body::from(
            serde_json::to_string(&json!({
                "ok": true,
                "user_id": user_id,
                "email": email,
                "role": role,
                "must_change_password": must_change_password,
            }))
            .unwrap_or_default(),
        ))
        .unwrap())
}

// ── Request types ─────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct Login2faRequest {
    pub login_token: String,
    pub code: String,
}

#[derive(Deserialize)]
pub struct Verify2faRequest {
    pub code: String,
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Deserialize)]
pub struct AdminCreateUserRequest {
    pub email: String,
    pub name: String,
    pub password: String,
    pub role: String,
}

#[derive(Deserialize)]
pub struct AdminResetPasswordRequest {
    pub new_password: String,
}

// ── Handlers ──────────────────────────────────────────────────────────

/// POST /_ui/api/auth/login — username/password login.
pub async fn login_handler(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    let db = state.require_config_db()?;

    // Rate limit check
    check_rate_limit(&state, &payload.email)?;

    // Look up user
    let user = db
        .get_user_by_email_with_auth(&payload.email)
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::InvalidCredentials)?;

    let (
        user_id,
        email,
        _name,
        _picture,
        role,
        password_hash,
        totp_enabled,
        auth_method,
        must_change_password,
    ) = user;

    // Must be a password user
    if auth_method != "password" {
        return Err(ApiError::InvalidCredentials);
    }

    // Must have a password hash
    let stored_hash = password_hash.ok_or(ApiError::InvalidCredentials)?;

    // Verify password
    let valid = verify_password(&payload.password, &stored_hash).map_err(ApiError::Internal)?;

    if !valid {
        record_login_failure(&state, &payload.email);
        return Err(ApiError::InvalidCredentials);
    }

    // Password verified — clear rate limiter
    clear_rate_limit(&state, &payload.email);

    // If TOTP is enabled, create pending 2FA login
    if totp_enabled {
        let token = db
            .create_pending_2fa(user_id)
            .await
            .map_err(ApiError::Internal)?;
        return Err(ApiError::TwoFactorRequired {
            login_token: token.to_string(),
        });
    }

    // No 2FA — create session directly
    create_session_response(
        &state,
        &db,
        user_id,
        &email,
        &role,
        "password",
        false,
        must_change_password,
    )
    .await
}

/// POST /_ui/api/auth/login/2fa — complete login with TOTP code.
pub async fn login_2fa_handler(
    State(state): State<AppState>,
    Json(payload): Json<Login2faRequest>,
) -> Result<Response, ApiError> {
    let db = state.require_config_db()?;

    // Parse login token
    let token = payload
        .login_token
        .parse::<Uuid>()
        .map_err(|_| ApiError::InvalidCredentials)?;

    // Look up pending 2FA
    let pending = db
        .get_pending_2fa(token)
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::InvalidCredentials)?;

    let (_token_id, user_id, _expires_at) = pending;

    // Delete the pending entry (single use)
    let _ = db.delete_pending_2fa(token).await;

    // Get user data
    let user = db
        .get_user_by_email_with_auth_by_id(user_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::InvalidCredentials)?;

    let (
        _id,
        email,
        _name,
        _picture,
        role,
        _pw_hash,
        _totp_enabled,
        _auth_method,
        must_change_password,
    ) = user;

    // Try TOTP verification first
    let totp_secret = db
        .get_totp_secret(user_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::InvalidCredentials)?;

    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        Secret::Encoded(totp_secret)
            .to_bytes()
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Invalid TOTP secret: {}", e)))?,
        Some("KiroGateway".to_string()),
        email.clone(),
    )
    .map_err(|e| ApiError::Internal(anyhow::anyhow!("TOTP init error: {}", e)))?;

    let code_valid = totp
        .check_current(&payload.code)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("TOTP check error: {}", e)))?;

    if !code_valid {
        // Try recovery code
        let code_hash = hash_recovery_code(&payload.code);
        let recovery_used = db
            .use_recovery_code(user_id, &code_hash)
            .await
            .map_err(ApiError::Internal)?;

        if !recovery_used {
            return Err(ApiError::InvalidCredentials);
        }
    }

    // Code valid — create session
    create_session_response(
        &state,
        &db,
        user_id,
        &email,
        &role,
        "password",
        true,
        must_change_password,
    )
    .await
}

/// GET /_ui/api/auth/2fa/setup — generate TOTP secret and QR code.
pub async fn setup_2fa_handler(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = request
        .extensions()
        .get::<SessionInfo>()
        .cloned()
        .ok_or(ApiError::SessionExpired)?;

    let db = state.require_config_db()?;

    // Generate TOTP secret
    let secret = Secret::generate_secret();
    let secret_base32 = secret.to_encoded().to_string();

    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret
            .to_bytes()
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Secret conversion error: {}", e)))?,
        Some("KiroGateway".to_string()),
        session.email.clone(),
    )
    .map_err(|e| ApiError::Internal(anyhow::anyhow!("TOTP init error: {}", e)))?;

    // Generate QR code data URL
    let qr_code_data_url = totp
        .get_qr_base64()
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("QR code generation error: {}", e)))?;

    let qr_code_data_url = format!("data:image/png;base64,{}", qr_code_data_url);

    // Store secret temporarily (will be committed on verify)
    // We store it immediately so verify_2fa_handler can find it
    db.enable_totp(session.user_id, &secret_base32)
        .await
        .map_err(ApiError::Internal)?;

    // But mark TOTP as not yet enabled — we only set it above to store the secret.
    // Re-disable it until verification succeeds.
    db.disable_totp(session.user_id)
        .await
        .map_err(ApiError::Internal)?;

    // Store just the secret without enabling
    sqlx::query("UPDATE users SET totp_secret = $1 WHERE id = $2")
        .bind(&secret_base32)
        .bind(session.user_id)
        .execute(db.pool())
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to store TOTP secret: {}", e)))?;

    // Generate recovery codes
    let recovery_codes = generate_recovery_codes(8);
    let code_hashes: Vec<String> = recovery_codes
        .iter()
        .map(|c| hash_recovery_code(c))
        .collect();

    db.store_recovery_codes(session.user_id, &code_hashes)
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(json!({
        "secret": secret_base32,
        "qr_code_data_url": qr_code_data_url,
        "recovery_codes": recovery_codes,
    })))
}

/// POST /_ui/api/auth/2fa/verify — verify TOTP code and enable 2FA.
pub async fn verify_2fa_handler(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Extract session before consuming the request body
    let session = request
        .extensions()
        .get::<SessionInfo>()
        .cloned()
        .ok_or(ApiError::SessionExpired)?;

    let body = axum::body::to_bytes(request.into_body(), 1024)
        .await
        .map_err(|e| ApiError::ValidationError(format!("Invalid request body: {}", e)))?;

    let payload: Verify2faRequest = serde_json::from_slice(&body)
        .map_err(|e| ApiError::ValidationError(format!("Invalid JSON: {}", e)))?;

    let db = state.require_config_db()?;

    // Get the stored secret
    let secret = db
        .get_totp_secret(session.user_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| {
            ApiError::ValidationError("No TOTP secret found. Call setup first.".to_string())
        })?;

    // Verify the code
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        Secret::Encoded(secret.clone())
            .to_bytes()
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Invalid TOTP secret: {}", e)))?,
        Some("KiroGateway".to_string()),
        session.email.clone(),
    )
    .map_err(|e| ApiError::Internal(anyhow::anyhow!("TOTP init error: {}", e)))?;

    let valid = totp
        .check_current(&payload.code)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("TOTP check error: {}", e)))?;

    if !valid {
        return Err(ApiError::ValidationError("Invalid TOTP code".to_string()));
    }

    // Enable TOTP
    db.enable_totp(session.user_id, &secret)
        .await
        .map_err(ApiError::Internal)?;

    // Update session cache — iterate all sessions since cache is keyed by session_id
    for mut entry in state.session_cache.iter_mut() {
        if entry.value().user_id == session.user_id {
            entry.value_mut().totp_enabled = true;
        }
    }

    Ok(Json(json!({ "ok": true })))
}

/// POST /_ui/api/auth/password/change — change password.
pub async fn change_password_handler(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = request
        .extensions()
        .get::<SessionInfo>()
        .cloned()
        .ok_or(ApiError::SessionExpired)?;

    let body = axum::body::to_bytes(request.into_body(), 4096)
        .await
        .map_err(|e| ApiError::ValidationError(format!("Invalid request body: {}", e)))?;

    let payload: ChangePasswordRequest = serde_json::from_slice(&body)
        .map_err(|e| ApiError::ValidationError(format!("Invalid JSON: {}", e)))?;

    let db = state.require_config_db()?;

    // Get current user auth info
    let user = db
        .get_user_by_email_with_auth_by_id(session.user_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::SessionExpired)?;

    let stored_hash: Option<String> = user.5;

    if let Some(ref hash) = stored_hash {
        // Existing user with password — verify current password
        let valid = verify_password(&payload.current_password, hash).map_err(ApiError::Internal)?;
        if !valid {
            return Err(ApiError::InvalidCredentials);
        }
    }
    // else: SSO user setting initial password — no verification needed

    // Hash new password
    let new_hash = hash_password(&payload.new_password).map_err(ApiError::Internal)?;

    // Update in DB
    db.update_password(session.user_id, &new_hash)
        .await
        .map_err(ApiError::Internal)?;

    // Update session cache
    for mut entry in state.session_cache.iter_mut() {
        if entry.value().user_id == session.user_id {
            entry.value_mut().must_change_password = false;
            entry.value_mut().auth_method = "password".to_string();
        }
    }

    Ok(Json(json!({ "ok": true })))
}

/// POST /_ui/api/admin/users/create — admin creates a password user.
pub async fn admin_create_user_handler(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = request
        .extensions()
        .get::<SessionInfo>()
        .cloned()
        .ok_or(ApiError::SessionExpired)?;

    if session.role != "admin" {
        return Err(ApiError::Forbidden("Admin access required".to_string()));
    }

    let body = axum::body::to_bytes(request.into_body(), 4096)
        .await
        .map_err(|e| ApiError::ValidationError(format!("Invalid request body: {}", e)))?;

    let payload: AdminCreateUserRequest = serde_json::from_slice(&body)
        .map_err(|e| ApiError::ValidationError(format!("Invalid JSON: {}", e)))?;

    let db = state.require_config_db()?;

    // Validate role
    if payload.role != "admin" && payload.role != "user" {
        return Err(ApiError::ValidationError(
            "Role must be 'admin' or 'user'".to_string(),
        ));
    }

    // Hash password
    let password_hash = hash_password(&payload.password).map_err(ApiError::Internal)?;

    // Create user
    let user_id = db
        .create_password_user(&payload.email, &payload.name, &password_hash, &payload.role)
        .await
        .map_err(ApiError::Internal)?;

    tracing::info!(
        user_id = %user_id,
        email = %payload.email,
        role = %payload.role,
        "Admin created password user"
    );

    Ok(Json(json!({
        "ok": true,
        "user_id": user_id,
        "email": payload.email,
        "name": payload.name,
        "role": payload.role,
        "must_change_password": true,
    })))
}

/// POST /_ui/api/admin/users/:id/reset-password — admin resets user password.
pub async fn admin_reset_password_handler(
    State(state): State<AppState>,
    axum::extract::Path(user_id): axum::extract::Path<Uuid>,
    request: Request<Body>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = request
        .extensions()
        .get::<SessionInfo>()
        .cloned()
        .ok_or(ApiError::SessionExpired)?;

    if session.role != "admin" {
        return Err(ApiError::Forbidden("Admin access required".to_string()));
    }

    let body = axum::body::to_bytes(request.into_body(), 4096)
        .await
        .map_err(|e| ApiError::ValidationError(format!("Invalid request body: {}", e)))?;

    let payload: AdminResetPasswordRequest = serde_json::from_slice(&body)
        .map_err(|e| ApiError::ValidationError(format!("Invalid JSON: {}", e)))?;

    let db = state.require_config_db()?;

    // Hash new password
    let password_hash = hash_password(&payload.new_password).map_err(ApiError::Internal)?;

    // Update password and set must_change_password=true
    sqlx::query("UPDATE users SET password_hash = $1, must_change_password = true WHERE id = $2")
        .bind(&password_hash)
        .bind(user_id)
        .execute(db.pool())
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to reset password: {}", e)))?;

    // Reset 2FA
    db.reset_user_2fa(user_id)
        .await
        .map_err(ApiError::Internal)?;

    // Evict user caches to force re-auth
    state.evict_user_caches(user_id);

    tracing::info!(
        admin = %session.email,
        target_user_id = %user_id,
        "Admin reset user password and 2FA"
    );

    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_verify_password() {
        let password = "test_password_123!";
        let hash = hash_password(password).unwrap();
        assert!(verify_password(password, &hash).unwrap());
        assert!(!verify_password("wrong_password", &hash).unwrap());
    }

    #[test]
    fn test_hash_password_produces_different_hashes() {
        let password = "same_password";
        let hash1 = hash_password(password).unwrap();
        let hash2 = hash_password(password).unwrap();
        // Different salts should produce different hashes
        assert_ne!(hash1, hash2);
        // Both should verify correctly
        assert!(verify_password(password, &hash1).unwrap());
        assert!(verify_password(password, &hash2).unwrap());
    }

    #[test]
    fn test_generate_recovery_codes() {
        let codes = generate_recovery_codes(8);
        assert_eq!(codes.len(), 8);
        for code in &codes {
            assert_eq!(code.len(), 8);
            assert!(code.chars().all(|c| c.is_ascii_alphanumeric()));
        }
        // All codes should be unique
        let unique: std::collections::HashSet<_> = codes.iter().collect();
        assert_eq!(unique.len(), 8);
    }

    #[test]
    fn test_hash_recovery_code() {
        let code = "abc12345";
        let hash = hash_recovery_code(code);
        assert_eq!(hash.len(), 64); // SHA-256 hex = 64 chars
                                    // Same input produces same hash
        assert_eq!(hash, hash_recovery_code(code));
        // Different input produces different hash
        assert_ne!(hash, hash_recovery_code("different"));
    }

    #[test]
    fn test_verify_password_invalid_hash() {
        let result = verify_password("password", "not-a-valid-hash");
        assert!(result.is_err());
    }
}
