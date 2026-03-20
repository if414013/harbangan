use axum::extract::{Path, State};
use axum::routing::{delete, get, patch};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use std::str::FromStr;

use crate::error::ApiError;
use crate::providers::types::ProviderId;
use crate::routes::{AppState, SessionInfo};

// ── Types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct PoolAccountResponse {
    accounts: Vec<crate::web_ui::config_db::AdminPoolRow>,
}

#[derive(Deserialize)]
struct AddPoolAccountRequest {
    provider_id: String,
    #[serde(default = "default_label")]
    account_label: String,
    api_key: String,
    #[serde(default)]
    key_prefix: String,
    base_url: Option<String>,
}

fn default_label() -> String {
    "pool-1".to_string()
}

#[derive(Deserialize)]
struct ToggleRequest {
    enabled: bool,
}

#[derive(Serialize)]
struct UserAccountResponse {
    accounts: Vec<UserAccountInfo>,
}

#[derive(Serialize)]
struct UserAccountInfo {
    provider_id: String,
    account_label: String,
    email: Option<String>,
    base_url: Option<String>,
    created_at: String,
}

#[derive(Serialize)]
struct RateLimitResponse {
    accounts: Vec<RateLimitAccountInfo>,
}

#[derive(Serialize)]
struct RateLimitAccountInfo {
    provider_id: String,
    account_label: String,
    is_user_account: bool,
    requests_remaining: Option<u64>,
    tokens_remaining: Option<u64>,
    is_limited: bool,
}

// ── Admin pool handlers ──────────────────────────────────────────────

/// GET /_ui/api/admin/pool — List all pool accounts
async fn list_pool_accounts(
    State(state): State<AppState>,
) -> Result<Json<PoolAccountResponse>, ApiError> {
    let db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("No database configured")))?;

    let accounts = db
        .get_all_admin_pool_accounts_include_disabled()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to list pool accounts: {}", e)))?;

    Ok(Json(PoolAccountResponse { accounts }))
}

/// POST /_ui/api/admin/pool — Add pool account
async fn add_pool_account(
    State(state): State<AppState>,
    Json(payload): Json<AddPoolAccountRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("No database configured")))?;

    // Validate provider_id via enum
    let pid = ProviderId::from_str(&payload.provider_id).map_err(ApiError::ValidationError)?;
    if !pid.supports_pool() {
        return Err(ApiError::ValidationError(format!(
            "Provider '{}' does not support pool accounts",
            payload.provider_id
        )));
    }

    let key_prefix = if payload.key_prefix.is_empty() {
        // Auto-generate prefix from first 8 chars of API key
        let chars: Vec<char> = payload.api_key.chars().collect();
        if chars.len() > 8 {
            chars[..8].iter().collect::<String>() + "..."
        } else {
            "****".to_string()
        }
    } else {
        payload.key_prefix
    };

    db.upsert_admin_pool_account(
        &payload.provider_id,
        &payload.account_label,
        &payload.api_key,
        &key_prefix,
        payload.base_url.as_deref(),
    )
    .await
    .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to add pool account: {}", e)))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

/// DELETE /_ui/api/admin/pool/:id — Remove pool account
async fn remove_pool_account(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("No database configured")))?;

    db.delete_admin_pool_account(id)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to remove pool account: {}", e)))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

/// PATCH /_ui/api/admin/pool/:id/toggle — Enable/disable pool account
async fn toggle_pool_account(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<ToggleRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("No database configured")))?;

    db.set_admin_pool_account_enabled(id, payload.enabled)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to toggle pool account: {}", e)))?;

    Ok(Json(
        serde_json::json!({ "ok": true, "enabled": payload.enabled }),
    ))
}

// ── User account handlers ────────────────────────────────────────────

/// GET /_ui/api/providers/:provider/accounts — List user's accounts for a provider
async fn list_user_accounts(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
    Path(provider): Path<String>,
) -> Result<Json<UserAccountResponse>, ApiError> {
    let db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("No database configured")))?;

    let rows = db
        .get_all_user_provider_tokens(session.user_id, &provider)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to list user accounts: {}", e)))?;

    let accounts = rows
        .into_iter()
        .map(|r| UserAccountInfo {
            provider_id: r.provider_id,
            account_label: r.account_label,
            email: if r.email.is_empty() {
                None
            } else {
                Some(r.email)
            },
            base_url: r.base_url,
            created_at: r.created_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(UserAccountResponse { accounts }))
}

/// DELETE /_ui/api/providers/:provider/accounts/:label — Delete specific account
async fn delete_user_account(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
    Path((provider, label)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("No database configured")))?;

    db.delete_user_provider_token_labeled(session.user_id, &provider, &label)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to delete account: {}", e)))?;

    // Invalidate registry cache for this user
    state.provider_registry.invalidate(session.user_id);

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Rate limit monitoring ────────────────────────────────────────────

/// GET /_ui/api/providers/rate-limits — Current rate limit state
async fn get_rate_limits(State(state): State<AppState>) -> Json<RateLimitResponse> {
    let accounts = state.rate_tracker.get_all_states();
    let now = std::time::Instant::now();

    let infos = accounts
        .into_iter()
        .map(|(account_id, rate_state)| RateLimitAccountInfo {
            provider_id: account_id.provider_id.as_str().to_string(),
            account_label: account_id.account_label,
            is_user_account: account_id.user_id.is_some(),
            requests_remaining: rate_state.requests_remaining,
            tokens_remaining: rate_state.tokens_remaining,
            is_limited: rate_state.limited_until.map(|t| t > now).unwrap_or(false),
        })
        .collect();

    Json(RateLimitResponse { accounts: infos })
}

// ── Route builders ───────────────────────────────────────────────────

/// Admin pool routes (admin-only, CSRF protected).
pub fn admin_pool_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/admin/pool",
            get(list_pool_accounts).post(add_pool_account),
        )
        .route("/admin/pool/:id", delete(remove_pool_account))
        .route("/admin/pool/:id/toggle", patch(toggle_pool_account))
}

/// User provider account routes (session-authenticated, CSRF protected).
pub fn user_account_routes() -> Router<AppState> {
    Router::new()
        .route("/providers/:provider/accounts", get(list_user_accounts))
        .route(
            "/providers/:provider/accounts/:label",
            delete(delete_user_account),
        )
}

/// Rate limit monitoring route (session-authenticated).
pub fn rate_limit_routes() -> Router<AppState> {
    Router::new().route("/providers/rate-limits", get(get_rate_limits))
}
