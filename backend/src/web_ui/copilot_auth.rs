use std::sync::Arc;
use std::time::Instant;

use axum::extract::State;
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use chrono::Utc;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::error::ApiError;
use crate::routes::{AppState, SessionInfo};
use crate::web_ui::config_db::ConfigDb;

// ── Constants ─────────────────────────────────────────────────────

const GITHUB_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
const GITHUB_DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const GITHUB_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const GITHUB_SCOPE: &str = "read:user";

const GITHUB_USER_URL: &str = "https://api.github.com/user";
const COPILOT_TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";
const COPILOT_USER_URL: &str = "https://api.github.com/copilot_internal/user";

const EDITOR_VERSION: &str = "vscode/1.97.2";
const EDITOR_PLUGIN_VERSION: &str = "copilot-chat/0.26.7";
const USER_AGENT_VALUE: &str = "GitHubCopilotChat/0.26.7";
const GITHUB_API_VERSION: &str = "2025-04-01";

// ── Pending State ─────────────────────────────────────────────────

/// In-memory pending state for a Copilot device flow.
#[derive(Debug, Clone)]
pub struct CopilotDevicePending {
    pub device_code: String,
    pub user_id: Uuid,
    pub created_at: chrono::DateTime<Utc>,
}

/// Shared map of pending Copilot device flows: device_code → CopilotDevicePending.
pub type CopilotDevicePendingMap = Arc<DashMap<String, CopilotDevicePending>>;

// ── Response types ────────────────────────────────────────────────

#[derive(Serialize)]
pub struct CopilotStatusResponse {
    pub connected: bool,
    pub github_username: Option<String>,
    pub copilot_plan: Option<String>,
    pub expired: bool,
}

#[derive(Serialize)]
struct CopilotDeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

#[derive(Deserialize)]
struct CopilotDevicePollQuery {
    device_code: String,
}

#[derive(Serialize)]
struct CopilotDevicePollResponse {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

// ── GitHub API response types ────────────────────────────────────

#[derive(Deserialize)]
struct GitHubDeviceCodeApiResponse {
    device_code: Option<String>,
    user_code: Option<String>,
    verification_uri: Option<String>,
    expires_in: Option<u64>,
    interval: Option<u64>,
    // Error fields
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct GitHubTokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    #[allow(dead_code)]
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct GitHubUserResponse {
    login: Option<String>,
}

#[derive(Deserialize)]
struct CopilotTokenResponse {
    token: Option<String>,
    expires_at: Option<i64>,
    refresh_in: Option<i64>,
}

#[derive(Deserialize)]
struct CopilotUserResponse {
    copilot_plan: Option<String>,
}

// ── Routes ────────────────────────────────────────────────────────

pub fn copilot_routes() -> Router<AppState> {
    Router::new()
        .route("/copilot/device-code", post(copilot_device_code))
        .route("/copilot/device-poll", get(copilot_device_poll))
        .route("/copilot/status", get(copilot_status))
        .route("/copilot/disconnect", delete(copilot_disconnect))
}

// ── POST /copilot/device-code ────────────────────────────────────

async fn copilot_device_code(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
) -> Result<Json<CopilotDeviceCodeResponse>, ApiError> {
    let pending_map = &state.copilot_device_pending;

    // Cleanup expired entries (10-min TTL)
    let now = Utc::now();
    pending_map.retain(|_, v| (now - v.created_at).num_minutes() < 10);

    if pending_map.len() >= 10_000 {
        return Err(ApiError::Internal(anyhow::anyhow!(
            "Too many pending Copilot device flows. Please try again later."
        )));
    }

    let http = reqwest::Client::new();
    let resp = http
        .post(GITHUB_DEVICE_CODE_URL)
        .header("accept", "application/json")
        .json(&json!({
            "client_id": GITHUB_CLIENT_ID,
            "scope": GITHUB_SCOPE,
        }))
        .send()
        .await
        .map_err(|e| {
            ApiError::CopilotAuthError(format!("GitHub device code request failed: {}", e))
        })?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(ApiError::CopilotAuthError(format!(
            "GitHub device code API returned {}: {}",
            status, body
        )));
    }

    let api_resp: GitHubDeviceCodeApiResponse = resp.json().await.map_err(|e| {
        ApiError::CopilotAuthError(format!(
            "Failed to parse GitHub device code response: {}",
            e
        ))
    })?;

    if let Some(err) = api_resp.error {
        let desc = api_resp.error_description.unwrap_or_default();
        return Err(ApiError::CopilotAuthError(format!(
            "GitHub device code error: {} - {}",
            err, desc
        )));
    }

    let device_code = api_resp.device_code.ok_or_else(|| {
        ApiError::CopilotAuthError("GitHub device code response missing device_code".to_string())
    })?;
    let user_code = api_resp.user_code.ok_or_else(|| {
        ApiError::CopilotAuthError("GitHub device code response missing user_code".to_string())
    })?;
    let verification_uri = api_resp.verification_uri.ok_or_else(|| {
        ApiError::CopilotAuthError(
            "GitHub device code response missing verification_uri".to_string(),
        )
    })?;

    // Store pending state
    pending_map.insert(
        device_code.clone(),
        CopilotDevicePending {
            device_code: device_code.clone(),
            user_id: session.user_id,
            created_at: Utc::now(),
        },
    );

    tracing::info!(
        user_id = %session.user_id,
        "Copilot device flow initiated"
    );

    Ok(Json(CopilotDeviceCodeResponse {
        device_code,
        user_code,
        verification_uri,
        expires_in: api_resp.expires_in.unwrap_or(900),
        interval: api_resp.interval.unwrap_or(5),
    }))
}

// ── GET /copilot/device-poll ─────────────────────────────────────

async fn copilot_device_poll(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
    axum::extract::Query(params): axum::extract::Query<CopilotDevicePollQuery>,
) -> Result<Json<CopilotDevicePollResponse>, ApiError> {
    let pending_map = &state.copilot_device_pending;

    // Look up pending state (don't remove yet — only remove on success/expiry)
    let pending = pending_map
        .get(&params.device_code)
        .ok_or_else(|| {
            ApiError::ValidationError(
                "Unknown or expired device code. Start a new device flow.".to_string(),
            )
        })?
        .clone();

    // Verify this device code belongs to the requesting user
    if pending.user_id != session.user_id {
        return Err(ApiError::ValidationError(
            "Device code does not belong to this user".to_string(),
        ));
    }

    // Check TTL (10 minutes)
    let age = Utc::now() - pending.created_at;
    if age.num_minutes() >= 10 {
        pending_map.remove(&params.device_code);
        return Ok(Json(CopilotDevicePollResponse {
            status: "expired".to_string(),
            message: Some("Device code expired. Start a new device flow.".to_string()),
        }));
    }

    let http = reqwest::Client::new();

    let resp = http
        .post(GITHUB_TOKEN_URL)
        .header("accept", "application/json")
        .json(&json!({
            "client_id": GITHUB_CLIENT_ID,
            "device_code": params.device_code,
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
        }))
        .send()
        .await
        .map_err(|e| ApiError::CopilotAuthError(format!("GitHub token poll failed: {}", e)))?;

    // Parse response body regardless of status (RFC 8628 uses 400 for pending states)
    let token_resp: GitHubTokenResponse = resp.json().await.map_err(|e| {
        ApiError::CopilotAuthError(format!("Failed to parse GitHub token response: {}", e))
    })?;

    // Handle RFC 8628 error codes
    if let Some(ref error_code) = token_resp.error {
        return match error_code.as_str() {
            "authorization_pending" => Ok(Json(CopilotDevicePollResponse {
                status: "pending".to_string(),
                message: Some("Waiting for user authorization".to_string()),
            })),
            "slow_down" => Ok(Json(CopilotDevicePollResponse {
                status: "slow_down".to_string(),
                message: Some("Polling too fast, please slow down".to_string()),
            })),
            "expired_token" => {
                pending_map.remove(&params.device_code);
                Ok(Json(CopilotDevicePollResponse {
                    status: "expired".to_string(),
                    message: Some("Device code expired. Start a new device flow.".to_string()),
                }))
            }
            "access_denied" => {
                pending_map.remove(&params.device_code);
                Ok(Json(CopilotDevicePollResponse {
                    status: "denied".to_string(),
                    message: Some("Authorization was denied by the user.".to_string()),
                }))
            }
            _ => {
                pending_map.remove(&params.device_code);
                Err(ApiError::CopilotAuthError(format!(
                    "GitHub token error: {}",
                    error_code
                )))
            }
        };
    }

    // Success — extract GitHub access token
    let github_token = token_resp.access_token.ok_or_else(|| {
        ApiError::CopilotAuthError("GitHub token response missing access_token".to_string())
    })?;

    // Fetch GitHub username
    let github_username = fetch_github_username(&http, &github_token)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Copilot device poll: failed to fetch GitHub username");
            e
        })?;

    // Fetch Copilot bearer token
    let copilot_resp = fetch_copilot_token(&http, &github_token)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Copilot device poll: failed to fetch Copilot token");
            e
        })?;

    // Detect account type (ok to fail)
    let copilot_plan = fetch_copilot_plan(&http, &github_token).await.ok();

    // Compute base_url from plan
    let base_url = copilot_plan
        .as_deref()
        .map(base_url_for_plan)
        .unwrap_or("https://api.githubcopilot.com");

    // Compute expires_at from epoch timestamp
    let expires_at = copilot_resp
        .expires_at
        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0));

    // Store in DB
    let db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::ConfigError("Database not configured".to_string()))?;
    db.upsert_copilot_tokens(
        session.user_id,
        &github_token,
        Some(&github_username),
        copilot_resp.token.as_deref(),
        copilot_plan.as_deref(),
        Some(base_url),
        expires_at,
        copilot_resp.refresh_in,
    )
    .await
    .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to store copilot tokens: {}", e)))?;

    // Invalidate cache
    state.copilot_token_cache.remove(&session.user_id);

    // Clean up pending state
    pending_map.remove(&params.device_code);

    tracing::info!(
        user_id = %session.user_id,
        github_username = %github_username,
        copilot_plan = ?copilot_plan,
        "Copilot connected via device flow"
    );

    Ok(Json(CopilotDevicePollResponse {
        status: "success".to_string(),
        message: Some("Copilot connected successfully".to_string()),
    }))
}

// ── GET /copilot/status ───────────────────────────────────────────

async fn copilot_status(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
) -> Result<Json<CopilotStatusResponse>, ApiError> {
    let db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::ConfigError("Database not configured".to_string()))?;
    let row = db
        .get_copilot_tokens(session.user_id)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to get copilot tokens: {}", e)))?;

    match row {
        Some(r) => {
            let expired = r.expires_at.map(|exp| exp < Utc::now()).unwrap_or(true);

            Ok(Json(CopilotStatusResponse {
                connected: true,
                github_username: r.github_username,
                copilot_plan: r.copilot_plan,
                expired,
            }))
        }
        None => Ok(Json(CopilotStatusResponse {
            connected: false,
            github_username: None,
            copilot_plan: None,
            expired: false,
        })),
    }
}

// ── DELETE /copilot/disconnect ────────────────────────────────────

async fn copilot_disconnect(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::ConfigError("Database not configured".to_string()))?;
    db.delete_copilot_tokens(session.user_id)
        .await
        .map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("Failed to delete copilot tokens: {}", e))
        })?;

    state.copilot_token_cache.remove(&session.user_id);

    tracing::info!(user_id = %session.user_id, "Copilot disconnected");

    Ok(Json(json!({ "status": "disconnected" })))
}

// ── Background token refresh ──────────────────────────────────────

pub fn spawn_copilot_token_refresh_task(
    config_db: Arc<ConfigDb>,
    copilot_token_cache: Arc<DashMap<Uuid, (String, String, Instant)>>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(120));
        let http = reqwest::Client::new();

        loop {
            interval.tick().await;

            let expiring = match config_db.get_expiring_copilot_tokens().await {
                Ok(tokens) => tokens,
                Err(e) => {
                    tracing::error!(error = ?e, "Failed to query expiring Copilot tokens");
                    continue;
                }
            };

            if expiring.is_empty() {
                continue;
            }

            tracing::debug!(count = expiring.len(), "Refreshing expiring Copilot tokens");

            for row in &expiring {
                match fetch_copilot_token(&http, &row.github_token).await {
                    Ok(resp) => {
                        let expires_at = resp
                            .expires_at
                            .map(|ts| chrono::DateTime::from_timestamp(ts, 0))
                            .flatten();

                        let plan = row.copilot_plan.as_deref();
                        let base_url = row.base_url.as_deref().unwrap_or_else(|| {
                            plan.map(base_url_for_plan)
                                .unwrap_or("https://api.githubcopilot.com")
                        });

                        if let Err(e) = config_db
                            .upsert_copilot_tokens(
                                row.user_id,
                                &row.github_token,
                                row.github_username.as_deref(),
                                resp.token.as_deref(),
                                plan,
                                Some(base_url),
                                expires_at,
                                resp.refresh_in,
                            )
                            .await
                        {
                            tracing::error!(
                                user_id = %row.user_id,
                                error = ?e,
                                "Failed to store refreshed Copilot token"
                            );
                        } else {
                            // Invalidate cache so next request picks up fresh token
                            copilot_token_cache.remove(&row.user_id);
                            tracing::info!(
                                user_id = %row.user_id,
                                "Copilot token refreshed"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            user_id = %row.user_id,
                            error = ?e,
                            "Copilot token refresh failed, marking disconnected"
                        );
                        // Null out copilot_token + expires_at to mark as disconnected
                        if let Err(e2) = config_db
                            .upsert_copilot_tokens(
                                row.user_id,
                                &row.github_token,
                                row.github_username.as_deref(),
                                None,
                                row.copilot_plan.as_deref(),
                                row.base_url.as_deref(),
                                None,
                                None,
                            )
                            .await
                        {
                            tracing::error!(
                                user_id = %row.user_id,
                                error = ?e2,
                                "Failed to mark Copilot token as expired"
                            );
                        }
                        copilot_token_cache.remove(&row.user_id);
                    }
                }
            }
        }
    });
}

// ── GitHub API helpers ────────────────────────────────────────────

fn github_api_headers(github_token: &str) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "authorization",
        format!("token {}", github_token).parse().unwrap(),
    );
    headers.insert("editor-version", EDITOR_VERSION.parse().unwrap());
    headers.insert(
        "editor-plugin-version",
        EDITOR_PLUGIN_VERSION.parse().unwrap(),
    );
    headers.insert("user-agent", USER_AGENT_VALUE.parse().unwrap());
    headers.insert("x-github-api-version", GITHUB_API_VERSION.parse().unwrap());
    headers
}

async fn fetch_github_username(
    http: &reqwest::Client,
    github_token: &str,
) -> Result<String, ApiError> {
    let resp = http
        .get(GITHUB_USER_URL)
        .header("authorization", format!("token {}", github_token))
        .header("user-agent", USER_AGENT_VALUE)
        .send()
        .await
        .map_err(|e| ApiError::CopilotAuthError(format!("Failed to fetch GitHub user: {}", e)))?;

    let body: GitHubUserResponse = resp.json().await.map_err(|e| {
        ApiError::CopilotAuthError(format!("Failed to parse GitHub user response: {}", e))
    })?;

    body.login
        .ok_or_else(|| ApiError::CopilotAuthError("GitHub user response missing login".to_string()))
}

async fn fetch_copilot_token(
    http: &reqwest::Client,
    github_token: &str,
) -> Result<CopilotTokenResponse, ApiError> {
    let headers = github_api_headers(github_token);

    let resp = http
        .get(COPILOT_TOKEN_URL)
        .headers(headers)
        .send()
        .await
        .map_err(|e| ApiError::CopilotAuthError(format!("Failed to fetch Copilot token: {}", e)))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(ApiError::CopilotAuthError(format!(
            "Copilot token API returned {}: {}",
            status, body
        )));
    }

    resp.json().await.map_err(|e| {
        ApiError::CopilotAuthError(format!("Failed to parse Copilot token response: {}", e))
    })
}

async fn fetch_copilot_plan(
    http: &reqwest::Client,
    github_token: &str,
) -> Result<String, ApiError> {
    let headers = github_api_headers(github_token);

    let resp = http
        .get(COPILOT_USER_URL)
        .headers(headers)
        .send()
        .await
        .map_err(|e| {
            ApiError::CopilotAuthError(format!("Failed to fetch Copilot user info: {}", e))
        })?;

    if !resp.status().is_success() {
        return Err(ApiError::CopilotAuthError(
            "Copilot user API returned non-success status".to_string(),
        ));
    }

    let body: CopilotUserResponse = resp.json().await.map_err(|e| {
        ApiError::CopilotAuthError(format!("Failed to parse Copilot user response: {}", e))
    })?;

    body.copilot_plan.ok_or_else(|| {
        ApiError::CopilotAuthError("Copilot user response missing copilot_plan".to_string())
    })
}

fn base_url_for_plan(plan: &str) -> &'static str {
    match plan {
        "business" => "https://api.business.githubcopilot.com",
        "enterprise" => "https://api.enterprise.githubcopilot.com",
        _ => "https://api.githubcopilot.com",
    }
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_url_for_plan() {
        assert_eq!(
            base_url_for_plan("individual"),
            "https://api.githubcopilot.com"
        );
        assert_eq!(
            base_url_for_plan("business"),
            "https://api.business.githubcopilot.com"
        );
        assert_eq!(
            base_url_for_plan("enterprise"),
            "https://api.enterprise.githubcopilot.com"
        );
        assert_eq!(
            base_url_for_plan("unknown"),
            "https://api.githubcopilot.com"
        );
    }

    #[test]
    fn test_copilot_status_response_serialization() {
        let resp = CopilotStatusResponse {
            connected: true,
            github_username: Some("octocat".to_string()),
            copilot_plan: Some("individual".to_string()),
            expired: false,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json["connected"].as_bool().unwrap());
        assert_eq!(json["github_username"], "octocat");
        assert_eq!(json["copilot_plan"], "individual");
        assert!(!json["expired"].as_bool().unwrap());
    }

    #[test]
    fn test_copilot_status_response_disconnected() {
        let resp = CopilotStatusResponse {
            connected: false,
            github_username: None,
            copilot_plan: None,
            expired: false,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(!json["connected"].as_bool().unwrap());
        assert!(json["github_username"].is_null());
        assert!(json["copilot_plan"].is_null());
    }

    #[test]
    fn test_github_api_headers() {
        let headers = github_api_headers("ghp_test123");
        assert_eq!(headers["authorization"], "token ghp_test123");
        assert_eq!(headers["editor-version"], EDITOR_VERSION);
        assert_eq!(headers["editor-plugin-version"], EDITOR_PLUGIN_VERSION);
        assert_eq!(headers["user-agent"], USER_AGENT_VALUE);
        assert_eq!(headers["x-github-api-version"], GITHUB_API_VERSION);
    }

    #[test]
    fn test_github_token_response_error() {
        let json = r#"{"error":"bad_verification_code","error_description":"The code passed is incorrect or expired."}"#;
        let resp: GitHubTokenResponse = serde_json::from_str(json).unwrap();
        assert!(resp.error.is_some());
        assert!(resp.access_token.is_none());
    }

    #[test]
    fn test_github_token_response_success() {
        let json = r#"{"access_token":"gho_abc123","token_type":"bearer","scope":"read:user"}"#;
        let resp: GitHubTokenResponse = serde_json::from_str(json).unwrap();
        assert!(resp.error.is_none());
        assert_eq!(resp.access_token.unwrap(), "gho_abc123");
    }

    #[test]
    fn test_copilot_token_response_deserialization() {
        let json = r#"{"token":"tid=abc;exp=1234567890;sku=copilot_for_individuals","expires_at":1234567890,"refresh_in":1500}"#;
        let resp: CopilotTokenResponse = serde_json::from_str(json).unwrap();
        assert!(resp.token.is_some());
        assert_eq!(resp.expires_at, Some(1234567890));
        assert_eq!(resp.refresh_in, Some(1500));
    }

    #[test]
    fn test_copilot_user_response_deserialization() {
        let json = r#"{"copilot_plan":"business"}"#;
        let resp: CopilotUserResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.copilot_plan.unwrap(), "business");
    }

    #[test]
    fn test_base_url_for_plan_empty_string() {
        assert_eq!(base_url_for_plan(""), "https://api.githubcopilot.com");
    }

    #[test]
    fn test_copilot_status_response_expired() {
        let resp = CopilotStatusResponse {
            connected: true,
            github_username: Some("octocat".to_string()),
            copilot_plan: Some("individual".to_string()),
            expired: true,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json["expired"].as_bool().unwrap());
        assert!(json["connected"].as_bool().unwrap());
    }

    #[test]
    fn test_device_code_response_serialization() {
        let resp = CopilotDeviceCodeResponse {
            device_code: "dc_test123".to_string(),
            user_code: "ABCD-1234".to_string(),
            verification_uri: "https://github.com/login/device".to_string(),
            expires_in: 900,
            interval: 5,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["device_code"], "dc_test123");
        assert_eq!(json["user_code"], "ABCD-1234");
        assert_eq!(json["verification_uri"], "https://github.com/login/device");
        assert_eq!(json["expires_in"], 900);
        assert_eq!(json["interval"], 5);
    }

    #[test]
    fn test_device_poll_response_pending() {
        let resp = CopilotDevicePollResponse {
            status: "pending".to_string(),
            message: Some("Waiting for user authorization".to_string()),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "pending");
        assert_eq!(json["message"], "Waiting for user authorization");
    }

    #[test]
    fn test_device_poll_response_success() {
        let resp = CopilotDevicePollResponse {
            status: "success".to_string(),
            message: Some("Copilot connected successfully".to_string()),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "success");
    }

    #[test]
    fn test_device_poll_response_expired() {
        let resp = CopilotDevicePollResponse {
            status: "expired".to_string(),
            message: Some("Device code expired".to_string()),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "expired");
    }

    #[test]
    fn test_device_poll_response_denied() {
        let resp = CopilotDevicePollResponse {
            status: "denied".to_string(),
            message: Some("Authorization was denied".to_string()),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "denied");
    }

    #[test]
    fn test_device_poll_response_slow_down() {
        let resp = CopilotDevicePollResponse {
            status: "slow_down".to_string(),
            message: Some("Polling too fast".to_string()),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "slow_down");
    }

    #[test]
    fn test_device_poll_response_no_message() {
        let resp = CopilotDevicePollResponse {
            status: "success".to_string(),
            message: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("message").is_none());
    }

    #[test]
    fn test_github_device_code_api_response_success() {
        let json = r#"{"device_code":"dc_123","user_code":"ABCD-EFGH","verification_uri":"https://github.com/login/device","expires_in":900,"interval":5}"#;
        let resp: GitHubDeviceCodeApiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.device_code.unwrap(), "dc_123");
        assert_eq!(resp.user_code.unwrap(), "ABCD-EFGH");
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_github_device_code_api_response_error() {
        let json = r#"{"error":"invalid_client","error_description":"Bad client ID"}"#;
        let resp: GitHubDeviceCodeApiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error.unwrap(), "invalid_client");
        assert!(resp.device_code.is_none());
    }

    #[test]
    fn test_github_device_code_api_response_minimal() {
        let json = r#"{"device_code":"dc_123","user_code":"ABCD","verification_uri":"https://github.com/login/device"}"#;
        let resp: GitHubDeviceCodeApiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.device_code.unwrap(), "dc_123");
        assert!(resp.expires_in.is_none());
        assert!(resp.interval.is_none());
    }

    #[test]
    fn test_github_token_response_rfc8628_authorization_pending() {
        let json = r#"{"error":"authorization_pending","error_description":"The user has not yet authorized"}"#;
        let resp: GitHubTokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error.unwrap(), "authorization_pending");
        assert!(resp.access_token.is_none());
    }

    #[test]
    fn test_github_token_response_rfc8628_slow_down() {
        let json = r#"{"error":"slow_down","error_description":"Polling too fast"}"#;
        let resp: GitHubTokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error.unwrap(), "slow_down");
    }

    #[test]
    fn test_github_token_response_rfc8628_expired_token() {
        let json = r#"{"error":"expired_token","error_description":"Device code expired"}"#;
        let resp: GitHubTokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error.unwrap(), "expired_token");
    }

    #[test]
    fn test_github_token_response_rfc8628_access_denied() {
        let json = r#"{"error":"access_denied","error_description":"User denied"}"#;
        let resp: GitHubTokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error.unwrap(), "access_denied");
    }

    #[test]
    fn test_copilot_device_pending_clone() {
        let pending = CopilotDevicePending {
            device_code: "dc_test".to_string(),
            user_id: Uuid::new_v4(),
            created_at: Utc::now(),
        };
        let cloned = pending.clone();
        assert_eq!(cloned.device_code, "dc_test");
        assert_eq!(cloned.user_id, pending.user_id);
    }

    #[test]
    fn test_copilot_pending_map_insert_and_lookup() {
        let map: CopilotDevicePendingMap = Arc::new(DashMap::new());
        let uid = Uuid::new_v4();
        map.insert(
            "dc_123".to_string(),
            CopilotDevicePending {
                device_code: "dc_123".to_string(),
                user_id: uid,
                created_at: Utc::now(),
            },
        );
        assert!(map.contains_key("dc_123"));
        let entry = map.get("dc_123").unwrap();
        assert_eq!(entry.user_id, uid);
    }

    #[test]
    fn test_copilot_pending_map_ttl_cleanup() {
        let map: CopilotDevicePendingMap = Arc::new(DashMap::new());
        let uid = Uuid::new_v4();

        // Insert an entry that's 11 minutes old (past 10-min TTL)
        map.insert(
            "dc_old".to_string(),
            CopilotDevicePending {
                device_code: "dc_old".to_string(),
                user_id: uid,
                created_at: Utc::now() - chrono::Duration::minutes(11),
            },
        );

        // Insert a fresh entry
        map.insert(
            "dc_new".to_string(),
            CopilotDevicePending {
                device_code: "dc_new".to_string(),
                user_id: uid,
                created_at: Utc::now(),
            },
        );

        let now = Utc::now();
        map.retain(|_, v| (now - v.created_at).num_minutes() < 10);

        assert!(
            !map.contains_key("dc_old"),
            "Old entry should be cleaned up"
        );
        assert!(map.contains_key("dc_new"), "Fresh entry should remain");
    }

    #[test]
    fn test_device_poll_query_deserialization() {
        let json = serde_json::json!({ "device_code": "test-code-123" });
        let q: CopilotDevicePollQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.device_code, "test-code-123");
    }

    #[test]
    fn test_copilot_token_response_partial_fields() {
        let json = r#"{"token":"tid=abc","expires_at":null,"refresh_in":null}"#;
        let resp: CopilotTokenResponse = serde_json::from_str(json).unwrap();
        assert!(resp.token.is_some());
        assert!(resp.expires_at.is_none());
        assert!(resp.refresh_in.is_none());
    }

    #[test]
    fn test_github_api_headers_token_format() {
        let headers = github_api_headers("ghp_abc");
        let auth = headers["authorization"].to_str().unwrap();
        assert!(auth.starts_with("token "));
        assert!(auth.ends_with("ghp_abc"));
    }

    #[test]
    fn test_copilot_routes_builds() {
        // Verify the router can be constructed without panicking
        let _router = copilot_routes();
    }

    #[test]
    fn test_github_user_response_missing_login() {
        let json = r#"{}"#;
        let resp: GitHubUserResponse = serde_json::from_str(json).unwrap();
        assert!(resp.login.is_none());
    }

    #[test]
    fn test_copilot_user_response_missing_plan() {
        let json = r#"{}"#;
        let resp: CopilotUserResponse = serde_json::from_str(json).unwrap();
        assert!(resp.copilot_plan.is_none());
    }
}
