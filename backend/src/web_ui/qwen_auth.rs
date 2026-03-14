/// Qwen Coder OAuth Device Flow — RFC 8628 implementation.
///
/// Endpoints:
/// - POST /_ui/api/providers/qwen/device-code  — initiate device flow
/// - GET  /_ui/api/providers/qwen/device-poll   — poll for token
/// - GET  /_ui/api/providers/qwen/status        — check connection status
/// - DELETE /_ui/api/providers/qwen/disconnect   — remove Qwen token
///
/// Uses DashMap for pending device flow state (10-min TTL, 10k cap).
/// Stores tokens in `user_provider_tokens` table via ConfigDb.
use std::sync::Arc;

use axum::extract::State;
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use chrono::{Duration, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::ApiError;
use crate::providers::qwen::QwenProvider;
use crate::providers::types::ProviderId;
use crate::routes::{AppState, SessionInfo};

/// Get the QwenProvider from AppState via downcast.
fn get_qwen(state: &AppState) -> Result<&QwenProvider, ApiError> {
    state
        .providers
        .get(&ProviderId::Qwen)
        .and_then(|p| p.as_any().downcast_ref::<QwenProvider>())
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("QwenProvider not registered")))
}

// ── Constants ────────────────────────────────────────────────────────

const QWEN_DEVICE_CODE_URL: &str = "https://chat.qwen.ai/api/v1/oauth2/device/code";
const QWEN_TOKEN_URL: &str = "https://chat.qwen.ai/api/v1/oauth2/token";

/// OAuth scope for Qwen device flow.
const QWEN_OAUTH_SCOPE: &str = "openid profile email model.completion";

// ── Pending State ────────────────────────────────────────────────────

/// In-memory pending state for a Qwen device flow.
#[derive(Debug, Clone)]
pub struct QwenDevicePending {
    #[allow(dead_code)]
    pub device_code: String,
    pub code_verifier: String,
    pub user_id: Uuid,
    pub created_at: chrono::DateTime<Utc>,
}

/// Shared map of pending Qwen device flows: device_code → QwenDevicePending.
pub type QwenDevicePendingMap = Arc<DashMap<String, QwenDevicePending>>;

// ── Types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    verification_uri_complete: Option<String>,
    expires_in: u64,
    interval: u64,
}

#[derive(Deserialize)]
struct DevicePollQuery {
    device_code: String,
}

#[derive(Serialize)]
struct DevicePollResponse {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Serialize)]
struct QwenStatusResponse {
    connected: bool,
    expired: bool,
}

// ── Qwen API response types ─────────────────────────────────────────

#[derive(Deserialize)]
struct QwenDeviceCodeApiResponse {
    device_code: Option<String>,
    user_code: Option<String>,
    verification_uri: Option<String>,
    verification_uri_complete: Option<String>,
    expires_in: Option<u64>,
    interval: Option<u64>,
    // Error fields
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct QwenTokenApiResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    resource_url: Option<String>,
    expires_in: Option<i64>,
    // Error fields (RFC 8628)
    error: Option<String>,
    #[allow(dead_code)]
    error_description: Option<String>,
}

// ── PKCE Helpers ─────────────────────────────────────────────────────

fn generate_pkce_verifier() -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use rand::Rng;
    let mut random_bytes = [0u8; 96];
    rand::thread_rng().fill(&mut random_bytes[..]);
    URL_SAFE_NO_PAD.encode(random_bytes)
}

fn pkce_challenge(verifier: &str) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

fn get_client_id(config: &crate::config::Config) -> String {
    config.qwen_oauth_client_id.clone()
}

// ── Handlers ─────────────────────────────────────────────────────────

/// POST /_ui/api/providers/qwen/device-code — initiate Qwen device flow.
async fn qwen_device_code(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
) -> Result<Json<DeviceCodeResponse>, ApiError> {
    let qwen = get_qwen(&state)?;
    let pending_map = qwen.device_pending();

    // Cleanup expired entries (10-min TTL)
    let now = Utc::now();
    pending_map.retain(|_, v| (now - v.created_at).num_minutes() < 10);

    if pending_map.len() >= 10_000 {
        return Err(ApiError::Internal(anyhow::anyhow!(
            "Too many pending Qwen device flows. Please try again later."
        )));
    }

    let app_config = state.config.read().unwrap_or_else(|p| p.into_inner()).clone();
    let client_id = get_client_id(&app_config);
    let code_verifier = generate_pkce_verifier();
    let code_challenge = pkce_challenge(&code_verifier);

    let http = reqwest::Client::new();
    let resp = http
        .post(QWEN_DEVICE_CODE_URL)
        .header("accept", "application/json")
        .form(&[
            ("client_id", client_id.as_str()),
            ("scope", QWEN_OAUTH_SCOPE),
            ("code_challenge", code_challenge.as_str()),
            ("code_challenge_method", "S256"),
        ])
        .send()
        .await
        .map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("Qwen device code request failed: {}", e))
        })?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(ApiError::Internal(anyhow::anyhow!(
            "Qwen device code API returned {}: {}",
            status,
            body
        )));
    }

    let api_resp: QwenDeviceCodeApiResponse = resp.json().await.map_err(|e| {
        ApiError::Internal(anyhow::anyhow!(
            "Failed to parse Qwen device code response: {}",
            e
        ))
    })?;

    if let Some(err) = api_resp.error {
        let desc = api_resp.error_description.unwrap_or_default();
        return Err(ApiError::Internal(anyhow::anyhow!(
            "Qwen device code error: {} - {}",
            err,
            desc
        )));
    }

    let device_code = api_resp.device_code.ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!(
            "Qwen device code response missing device_code"
        ))
    })?;
    let user_code = api_resp.user_code.ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!(
            "Qwen device code response missing user_code"
        ))
    })?;
    let verification_uri = api_resp.verification_uri.ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!(
            "Qwen device code response missing verification_uri"
        ))
    })?;

    // Store pending state
    pending_map.insert(
        device_code.clone(),
        QwenDevicePending {
            device_code: device_code.clone(),
            code_verifier,
            user_id: session.user_id,
            created_at: Utc::now(),
        },
    );

    tracing::info!(
        user_id = %session.user_id,
        "Qwen device flow initiated"
    );

    Ok(Json(DeviceCodeResponse {
        device_code,
        user_code,
        verification_uri,
        verification_uri_complete: api_resp.verification_uri_complete,
        expires_in: api_resp.expires_in.unwrap_or(600),
        interval: api_resp.interval.unwrap_or(5),
    }))
}

/// GET /_ui/api/providers/qwen/device-poll — poll Qwen device flow.
async fn qwen_device_poll(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
    axum::extract::Query(params): axum::extract::Query<DevicePollQuery>,
) -> Result<Json<DevicePollResponse>, ApiError> {
    let qwen = get_qwen(&state)?;
    let pending_map = qwen.device_pending();

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
        return Ok(Json(DevicePollResponse {
            status: "expired".to_string(),
            message: Some("Device code expired. Start a new device flow.".to_string()),
        }));
    }

    let app_config = state.config.read().unwrap_or_else(|p| p.into_inner()).clone();
    let client_id = get_client_id(&app_config);
    let http = reqwest::Client::new();

    let resp = http
        .post(QWEN_TOKEN_URL)
        .header("accept", "application/json")
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", client_id.as_str()),
            ("device_code", params.device_code.as_str()),
            ("code_verifier", pending.code_verifier.as_str()),
        ])
        .send()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Qwen token poll failed: {}", e)))?;

    // Parse response body regardless of status (RFC 8628 uses 400 for pending states)
    let token_resp: QwenTokenApiResponse = resp.json().await.map_err(|e| {
        ApiError::Internal(anyhow::anyhow!(
            "Failed to parse Qwen token response: {}",
            e
        ))
    })?;

    // Handle RFC 8628 error codes
    if let Some(ref error_code) = token_resp.error {
        return match error_code.as_str() {
            "authorization_pending" => Ok(Json(DevicePollResponse {
                status: "pending".to_string(),
                message: Some("Waiting for user authorization".to_string()),
            })),
            "slow_down" => Ok(Json(DevicePollResponse {
                status: "pending".to_string(),
                message: Some("Polling too fast, please slow down".to_string()),
            })),
            "expired_token" => {
                pending_map.remove(&params.device_code);
                Ok(Json(DevicePollResponse {
                    status: "expired".to_string(),
                    message: Some("Device code expired. Start a new device flow.".to_string()),
                }))
            }
            "access_denied" => {
                pending_map.remove(&params.device_code);
                Ok(Json(DevicePollResponse {
                    status: "denied".to_string(),
                    message: Some("Authorization was denied by the user.".to_string()),
                }))
            }
            _ => {
                pending_map.remove(&params.device_code);
                Err(ApiError::Internal(anyhow::anyhow!(
                    "Qwen token error: {}",
                    error_code
                )))
            }
        };
    }

    // Success — extract tokens
    let access_token = token_resp.access_token.ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!("Qwen token response missing access_token"))
    })?;
    let refresh_token = token_resp.refresh_token.unwrap_or_default();
    let resource_url = token_resp.resource_url;
    let expires_in = token_resp.expires_in.unwrap_or(3600);

    // Store in user_provider_tokens
    let config_db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Database not configured")))?;

    let expires_at = Utc::now() + Duration::seconds(expires_in - 60);

    config_db
        .upsert_user_provider_token(
            session.user_id,
            "qwen",
            &access_token,
            &refresh_token,
            expires_at,
            "", // No email for Qwen device flow
        )
        .await
        .map_err(ApiError::Internal)?;

    // Store resource_url as base_url if provided
    if let Some(ref url) = resource_url {
        config_db
            .set_user_provider_base_url(session.user_id, "qwen", url)
            .await
            .map_err(ApiError::Internal)?;
    }

    // Invalidate provider registry cache
    state.provider_registry.invalidate(session.user_id);

    // Clean up pending state
    pending_map.remove(&params.device_code);

    tracing::info!(
        user_id = %session.user_id,
        resource_url = ?resource_url,
        "Qwen device flow completed, token stored"
    );

    Ok(Json(DevicePollResponse {
        status: "success".to_string(),
        message: Some("Qwen connected successfully".to_string()),
    }))
}

/// GET /_ui/api/providers/qwen/status — check Qwen connection status.
async fn qwen_status(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
) -> Result<Json<QwenStatusResponse>, ApiError> {
    let config_db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Database not configured")))?;

    let token = config_db
        .get_user_provider_token(session.user_id, "qwen")
        .await
        .map_err(ApiError::Internal)?;

    match token {
        Some((_access, _refresh, expires_at, _email)) => {
            let expired = expires_at <= Utc::now();
            Ok(Json(QwenStatusResponse {
                connected: true,
                expired,
            }))
        }
        None => Ok(Json(QwenStatusResponse {
            connected: false,
            expired: false,
        })),
    }
}

/// DELETE /_ui/api/providers/qwen/disconnect — remove Qwen token.
async fn qwen_disconnect(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config_db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Database not configured")))?;

    config_db
        .delete_user_provider_token(session.user_id, "qwen")
        .await
        .map_err(ApiError::Internal)?;

    // Invalidate provider registry cache
    state.provider_registry.invalidate(session.user_id);

    tracing::info!(user_id = %session.user_id, "Qwen disconnected");

    Ok(Json(json!({ "status": "disconnected" })))
}

// ── Router ───────────────────────────────────────────────────────────

pub fn qwen_auth_routes() -> Router<AppState> {
    Router::new()
        .route("/providers/qwen/device-code", post(qwen_device_code))
        .route("/providers/qwen/device-poll", get(qwen_device_poll))
        .route("/providers/qwen/status", get(qwen_status))
        .route("/providers/qwen/disconnect", delete(qwen_disconnect))
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_code_response_serialization() {
        let resp = DeviceCodeResponse {
            device_code: "QWEN-1234".to_string(),
            user_code: "ABCD-EFGH".to_string(),
            verification_uri: "https://chat.qwen.ai/device".to_string(),
            verification_uri_complete: Some(
                "https://chat.qwen.ai/device?user_code=ABCD-EFGH".to_string(),
            ),
            expires_in: 600,
            interval: 5,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["device_code"], "QWEN-1234");
        assert_eq!(json["user_code"], "ABCD-EFGH");
        assert_eq!(json["expires_in"], 600);
        assert_eq!(json["interval"], 5);
    }

    #[test]
    fn test_device_code_response_no_complete_uri() {
        let resp = DeviceCodeResponse {
            device_code: "QWEN-1234".to_string(),
            user_code: "ABCD-EFGH".to_string(),
            verification_uri: "https://chat.qwen.ai/device".to_string(),
            verification_uri_complete: None,
            expires_in: 600,
            interval: 5,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("verification_uri_complete").is_none());
    }

    #[test]
    fn test_device_poll_response_pending() {
        let resp = DevicePollResponse {
            status: "pending".to_string(),
            message: Some("Waiting for user authorization".to_string()),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "pending");
        assert_eq!(json["message"], "Waiting for user authorization");
    }

    #[test]
    fn test_device_poll_response_success() {
        let resp = DevicePollResponse {
            status: "success".to_string(),
            message: Some("Qwen connected successfully".to_string()),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "success");
    }

    #[test]
    fn test_device_poll_response_expired() {
        let resp = DevicePollResponse {
            status: "expired".to_string(),
            message: Some("Device code expired".to_string()),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "expired");
    }

    #[test]
    fn test_device_poll_response_denied() {
        let resp = DevicePollResponse {
            status: "denied".to_string(),
            message: Some("Authorization was denied".to_string()),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "denied");
    }

    #[test]
    fn test_device_poll_response_no_message() {
        let resp = DevicePollResponse {
            status: "success".to_string(),
            message: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("message").is_none());
    }

    #[test]
    fn test_qwen_status_response_connected() {
        let resp = QwenStatusResponse {
            connected: true,
            expired: false,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json["connected"].as_bool().unwrap());
        assert!(!json["expired"].as_bool().unwrap());
    }

    #[test]
    fn test_qwen_status_response_disconnected() {
        let resp = QwenStatusResponse {
            connected: false,
            expired: false,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(!json["connected"].as_bool().unwrap());
    }

    #[test]
    fn test_qwen_status_response_expired() {
        let resp = QwenStatusResponse {
            connected: true,
            expired: true,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json["connected"].as_bool().unwrap());
        assert!(json["expired"].as_bool().unwrap());
    }

    #[test]
    fn test_device_poll_query_deserialization() {
        let json = serde_json::json!({ "device_code": "test-code-123" });
        let q: DevicePollQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.device_code, "test-code-123");
    }

    #[test]
    fn test_qwen_device_code_api_response_success() {
        let json = r#"{"device_code":"dc_123","user_code":"ABCD-EFGH","verification_uri":"https://chat.qwen.ai/device","expires_in":600,"interval":5}"#;
        let resp: QwenDeviceCodeApiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.device_code.unwrap(), "dc_123");
        assert_eq!(resp.user_code.unwrap(), "ABCD-EFGH");
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_qwen_device_code_api_response_error() {
        let json = r#"{"error":"invalid_client","error_description":"Bad client ID"}"#;
        let resp: QwenDeviceCodeApiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error.unwrap(), "invalid_client");
        assert!(resp.device_code.is_none());
    }

    #[test]
    fn test_qwen_token_api_response_success() {
        let json = r#"{"access_token":"at_123","refresh_token":"rt_456","resource_url":"https://custom.qwen.ai/api","expires_in":3600}"#;
        let resp: QwenTokenApiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.access_token.unwrap(), "at_123");
        assert_eq!(resp.refresh_token.unwrap(), "rt_456");
        assert_eq!(resp.resource_url.unwrap(), "https://custom.qwen.ai/api");
        assert_eq!(resp.expires_in.unwrap(), 3600);
    }

    #[test]
    fn test_qwen_token_api_response_pending() {
        let json = r#"{"error":"authorization_pending","error_description":"The user has not yet authorized"}"#;
        let resp: QwenTokenApiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error.unwrap(), "authorization_pending");
        assert!(resp.access_token.is_none());
    }

    #[test]
    fn test_pkce_challenge_deterministic() {
        let verifier = "test-verifier-12345";
        let c1 = pkce_challenge(verifier);
        let c2 = pkce_challenge(verifier);
        assert_eq!(c1, c2);
        assert!(!c1.is_empty());
    }

    #[test]
    fn test_pkce_verifier_length() {
        let v = generate_pkce_verifier();
        assert_eq!(v.len(), 128); // 96 bytes → 128 base64 chars
    }

    #[test]
    fn test_pkce_verifier_unique() {
        let v1 = generate_pkce_verifier();
        let v2 = generate_pkce_verifier();
        assert_ne!(v1, v2);
    }

    #[test]
    fn test_qwen_auth_routes_builds() {
        let _router = qwen_auth_routes();
    }

    #[test]
    fn test_get_client_id_default() {
        // When using defaults, should return the default client ID
        let cfg = crate::config::Config::with_defaults();
        let id = get_client_id(&cfg);
        assert!(!id.is_empty());
    }

    // ── 6.5: PKCE additional tests ──────────────────────────────────

    #[test]
    fn test_pkce_challenge_is_base64url() {
        let verifier = "test-verifier-for-base64-check";
        let challenge = pkce_challenge(verifier);
        // Base64url should not contain +, /, or =
        assert!(!challenge.contains('+'));
        assert!(!challenge.contains('/'));
        assert!(!challenge.contains('='));
    }

    #[test]
    fn test_pkce_challenge_sha256_length() {
        // SHA-256 produces 32 bytes, base64url-encoded = 43 chars (no padding)
        let verifier = "any-verifier";
        let challenge = pkce_challenge(verifier);
        assert_eq!(challenge.len(), 43);
    }

    #[test]
    fn test_pkce_different_verifiers_different_challenges() {
        let c1 = pkce_challenge("verifier-one");
        let c2 = pkce_challenge("verifier-two");
        assert_ne!(c1, c2);
    }

    #[test]
    fn test_pkce_verifier_is_base64url() {
        let v = generate_pkce_verifier();
        assert!(!v.contains('+'));
        assert!(!v.contains('/'));
        assert!(!v.contains('='));
    }

    // ── 6.5: Pending state tests ────────────────────────────────────

    #[test]
    fn test_qwen_device_pending_clone() {
        let pending = QwenDevicePending {
            device_code: "dc_test".to_string(),
            code_verifier: "cv_test".to_string(),
            user_id: Uuid::new_v4(),
            created_at: Utc::now(),
        };
        let cloned = pending.clone();
        assert_eq!(cloned.device_code, "dc_test");
        assert_eq!(cloned.code_verifier, "cv_test");
        assert_eq!(cloned.user_id, pending.user_id);
    }

    #[test]
    fn test_pending_map_insert_and_lookup() {
        let map: QwenDevicePendingMap = Arc::new(DashMap::new());
        let uid = Uuid::new_v4();
        map.insert(
            "dc_123".to_string(),
            QwenDevicePending {
                device_code: "dc_123".to_string(),
                code_verifier: "cv_abc".to_string(),
                user_id: uid,
                created_at: Utc::now(),
            },
        );
        assert!(map.contains_key("dc_123"));
        let entry = map.get("dc_123").unwrap();
        assert_eq!(entry.user_id, uid);
        assert_eq!(entry.code_verifier, "cv_abc");
    }

    #[test]
    fn test_pending_map_ttl_cleanup_logic() {
        let map: QwenDevicePendingMap = Arc::new(DashMap::new());
        let uid = Uuid::new_v4();

        // Insert an entry that's 11 minutes old (past 10-min TTL)
        map.insert(
            "dc_old".to_string(),
            QwenDevicePending {
                device_code: "dc_old".to_string(),
                code_verifier: "cv_old".to_string(),
                user_id: uid,
                created_at: Utc::now() - chrono::Duration::minutes(11),
            },
        );

        // Insert a fresh entry
        map.insert(
            "dc_new".to_string(),
            QwenDevicePending {
                device_code: "dc_new".to_string(),
                code_verifier: "cv_new".to_string(),
                user_id: uid,
                created_at: Utc::now(),
            },
        );

        // Simulate the cleanup logic from qwen_device_code handler
        let now = Utc::now();
        map.retain(|_, v| (now - v.created_at).num_minutes() < 10);

        assert!(
            !map.contains_key("dc_old"),
            "Old entry should be cleaned up"
        );
        assert!(map.contains_key("dc_new"), "Fresh entry should remain");
    }

    #[test]
    fn test_pending_map_user_isolation() {
        let map: QwenDevicePendingMap = Arc::new(DashMap::new());
        let user_a = Uuid::new_v4();
        let user_b = Uuid::new_v4();

        map.insert(
            "dc_a".to_string(),
            QwenDevicePending {
                device_code: "dc_a".to_string(),
                code_verifier: "cv_a".to_string(),
                user_id: user_a,
                created_at: Utc::now(),
            },
        );
        map.insert(
            "dc_b".to_string(),
            QwenDevicePending {
                device_code: "dc_b".to_string(),
                code_verifier: "cv_b".to_string(),
                user_id: user_b,
                created_at: Utc::now(),
            },
        );

        // Each user's device code maps to their own user_id
        assert_eq!(map.get("dc_a").unwrap().user_id, user_a);
        assert_eq!(map.get("dc_b").unwrap().user_id, user_b);
    }

    // ── 6.5: Token API response edge cases ──────────────────────────

    #[test]
    fn test_qwen_token_api_response_slow_down() {
        let json = r#"{"error":"slow_down","error_description":"Polling too fast"}"#;
        let resp: QwenTokenApiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error.unwrap(), "slow_down");
        assert!(resp.access_token.is_none());
    }

    #[test]
    fn test_qwen_token_api_response_expired_token() {
        let json = r#"{"error":"expired_token","error_description":"Device code expired"}"#;
        let resp: QwenTokenApiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error.unwrap(), "expired_token");
    }

    #[test]
    fn test_qwen_token_api_response_access_denied() {
        let json = r#"{"error":"access_denied","error_description":"User denied"}"#;
        let resp: QwenTokenApiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error.unwrap(), "access_denied");
    }

    #[test]
    fn test_qwen_token_api_response_no_refresh_token() {
        let json = r#"{"access_token":"at_123","expires_in":3600}"#;
        let resp: QwenTokenApiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.access_token.unwrap(), "at_123");
        assert!(resp.refresh_token.is_none());
        assert!(resp.resource_url.is_none());
    }

    #[test]
    fn test_qwen_token_api_response_no_resource_url() {
        let json = r#"{"access_token":"at_123","refresh_token":"rt_456","expires_in":3600}"#;
        let resp: QwenTokenApiResponse = serde_json::from_str(json).unwrap();
        assert!(resp.resource_url.is_none());
    }

    #[test]
    fn test_qwen_device_code_api_response_with_complete_uri() {
        let json = r#"{"device_code":"dc_123","user_code":"ABCD-EFGH","verification_uri":"https://chat.qwen.ai/device","verification_uri_complete":"https://chat.qwen.ai/device?user_code=ABCD-EFGH","expires_in":600,"interval":5}"#;
        let resp: QwenDeviceCodeApiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            resp.verification_uri_complete.unwrap(),
            "https://chat.qwen.ai/device?user_code=ABCD-EFGH"
        );
    }

    #[test]
    fn test_qwen_device_code_api_response_minimal() {
        // Only required fields
        let json = r#"{"device_code":"dc_123","user_code":"ABCD","verification_uri":"https://chat.qwen.ai/device"}"#;
        let resp: QwenDeviceCodeApiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.device_code.unwrap(), "dc_123");
        assert!(resp.expires_in.is_none());
        assert!(resp.interval.is_none());
        assert!(resp.verification_uri_complete.is_none());
    }

    // ── 6.5: DevicePollResponse serialization edge cases ────────────

    #[test]
    fn test_device_poll_response_all_rfc8628_states() {
        for (status, msg) in [
            ("pending", "Waiting for user authorization"),
            ("pending", "Polling too fast, please slow down"),
            ("expired", "Device code expired. Start a new device flow."),
            ("denied", "Authorization was denied by the user."),
            ("success", "Qwen connected successfully"),
        ] {
            let resp = DevicePollResponse {
                status: status.to_string(),
                message: Some(msg.to_string()),
            };
            let json = serde_json::to_value(&resp).unwrap();
            assert_eq!(json["status"], status);
            assert_eq!(json["message"], msg);
        }
    }

    // ── 6.5: DeviceCodeResponse field validation ────────────────────

    #[test]
    fn test_device_code_response_all_fields() {
        let resp = DeviceCodeResponse {
            device_code: "dc_full".to_string(),
            user_code: "FULL-CODE".to_string(),
            verification_uri: "https://chat.qwen.ai/device".to_string(),
            verification_uri_complete: Some(
                "https://chat.qwen.ai/device?user_code=FULL-CODE".to_string(),
            ),
            expires_in: 900,
            interval: 10,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["device_code"], "dc_full");
        assert_eq!(json["user_code"], "FULL-CODE");
        assert_eq!(json["verification_uri"], "https://chat.qwen.ai/device");
        assert_eq!(
            json["verification_uri_complete"],
            "https://chat.qwen.ai/device?user_code=FULL-CODE"
        );
        assert_eq!(json["expires_in"], 900);
        assert_eq!(json["interval"], 10);
    }
}
