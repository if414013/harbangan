use std::sync::Arc;

use axum::extract::State;
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::oauth;
use crate::auth::PollResult;
use crate::error::ApiError;
use crate::routes::{AppState, SessionInfo};
use crate::web_ui::config_db::ConfigDb;

// ── Types ────────────────────────────────────────────────────────────

/// Response for GET /kiro/status
#[derive(Serialize)]
struct KiroStatusResponse {
    has_token: bool,
    expired: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    sso_start_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sso_region: Option<String>,
}

/// Request for POST /kiro/setup (start device code flow)
#[derive(Deserialize)]
struct KiroSetupRequest {
    sso_start_url: String,
    #[serde(default)]
    sso_region: Option<String>,
}

/// Response for POST /kiro/setup (start device code flow)
#[derive(Serialize)]
struct KiroSetupResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: String,
    expires_in: u64,
    interval: u64,
}

/// Request for POST /kiro/poll
#[derive(Deserialize)]
struct KiroPollRequest {
    device_code: String,
}

/// Response for POST /kiro/poll
#[derive(Serialize)]
struct KiroPollResponse {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

// ── Handlers ─────────────────────────────────────────────────────────

/// GET /_ui/api/kiro/status — has token? expired?
async fn kiro_status(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
) -> Result<Json<KiroStatusResponse>, ApiError> {
    let user_id = session.user_id;
    let config_db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Database not configured")))?;

    let token = config_db
        .get_kiro_token(user_id)
        .await
        .map_err(ApiError::Internal)?;

    // Fetch per-user SSO config
    let sso_config = config_db
        .get_user_sso_config(user_id)
        .await
        .map_err(ApiError::Internal)?;

    let (sso_start_url, sso_region) = match sso_config {
        Some((url, region)) => (Some(url), Some(region)),
        None => (None, None),
    };

    match token {
        Some((_refresh, access, expiry)) => {
            let expired = match expiry {
                Some(exp) => exp <= Utc::now(),
                None => access.is_none(),
            };
            Ok(Json(KiroStatusResponse {
                has_token: true,
                expired,
                sso_start_url,
                sso_region,
            }))
        }
        None => Ok(Json(KiroStatusResponse {
            has_token: false,
            expired: false,
            sso_start_url,
            sso_region,
        })),
    }
}

/// POST /_ui/api/kiro/setup — start device code flow for authenticated user
async fn kiro_setup(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
    Json(body): Json<KiroSetupRequest>,
) -> Result<Json<KiroSetupResponse>, ApiError> {
    let user_id = session.user_id;
    let config_db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Database not configured")))?;

    // Read SSO config from request body
    let sso_region = body.sso_region.unwrap_or_else(|| "us-east-1".to_string());

    // Normalize: strip fragment (#...) and trailing slashes
    let start_url = body
        .sso_start_url
        .split('#')
        .next()
        .unwrap_or(&body.sso_start_url)
        .trim_end_matches('/')
        .to_string();

    if start_url.is_empty() {
        return Err(ApiError::ValidationError(
            "sso_start_url is required (e.g. https://d-xxxxxxxxxx.awsapps.com/start)".into(),
        ));
    }

    // Clear any stale per-user OAuth client creds before fresh registration
    config_db
        .clear_user_oauth_client(user_id)
        .await
        .map_err(ApiError::Internal)?;

    let http_client = reqwest::Client::new();

    // Always register a fresh OAuth client per user
    tracing::info!(user_id = %user_id, "Registering fresh OAuth client for user");
    let start_url_opt = Some(start_url.as_str());
    let registration =
        oauth::register_client(&http_client, &sso_region, "device", None, start_url_opt)
            .await
            .map_err(ApiError::Internal)?;

    // Store per-user OAuth client credentials with SSO config
    config_db
        .upsert_user_oauth_client(
            user_id,
            &registration.client_id,
            &registration.client_secret,
            &sso_region,
            &start_url,
        )
        .await
        .map_err(ApiError::Internal)?;

    tracing::info!(
        user_id = %user_id,
        client_id = %registration.client_id,
        region = %sso_region,
        "Per-user OAuth client registered"
    );

    let device_auth = oauth::start_device_authorization(
        &http_client,
        &sso_region,
        &registration.client_id,
        &registration.client_secret,
        &start_url,
    )
    .await
    .map_err(ApiError::Internal)?;

    Ok(Json(KiroSetupResponse {
        device_code: device_auth.device_code,
        user_code: device_auth.user_code,
        verification_uri: device_auth.verification_uri,
        verification_uri_complete: device_auth.verification_uri_complete,
        expires_in: device_auth.expires_in,
        interval: device_auth.interval,
    }))
}

/// POST /_ui/api/kiro/poll — poll device code completion
async fn kiro_poll(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
    Json(body): Json<KiroPollRequest>,
) -> Result<Json<KiroPollResponse>, ApiError> {
    let user_id = session.user_id;
    let config_db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Database not configured")))?;

    // Load per-user OAuth client credentials
    let (client_id, client_secret, sso_region) = config_db
        .get_user_oauth_client(user_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| {
            ApiError::ValidationError(
                "OAuth client not configured for this user. Run setup first.".to_string(),
            )
        })?;

    let http_client = reqwest::Client::new();
    let poll_result = oauth::poll_device_token(
        &http_client,
        &sso_region,
        &client_id,
        &client_secret,
        &body.device_code,
    )
    .await
    .map_err(ApiError::Internal)?;

    match poll_result {
        PollResult::Pending => Ok(Json(KiroPollResponse {
            status: "pending".to_string(),
            message: Some("Waiting for user authorization".to_string()),
        })),
        PollResult::SlowDown => Ok(Json(KiroPollResponse {
            status: "slow_down".to_string(),
            message: Some("Polling too fast, please slow down".to_string()),
        })),
        PollResult::Success(token_response) => {
            // Store the token for this user
            let refresh_token = token_response.refresh_token.as_deref().unwrap_or_default();
            let access_token = &token_response.access_token;
            let expiry = token_response
                .expires_in
                .map(|secs| Utc::now() + Duration::seconds(secs as i64 - 60));

            config_db
                .upsert_kiro_token(user_id, refresh_token, Some(access_token), expiry)
                .await
                .map_err(ApiError::Internal)?;

            tracing::info!(user_id = %user_id, "Kiro token stored via device code flow");

            Ok(Json(KiroPollResponse {
                status: "success".to_string(),
                message: Some("Kiro token configured successfully".to_string()),
            }))
        }
    }
}

/// DELETE /_ui/api/kiro/token — remove own Kiro token
async fn kiro_delete_token(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id = session.user_id;
    let config_db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Database not configured")))?;

    config_db
        .delete_kiro_token(user_id)
        .await
        .map_err(ApiError::Internal)?;

    tracing::info!(user_id = %user_id, "Kiro token removed");

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Router ───────────────────────────────────────────────────────────

/// Build the Kiro token management router.
/// All routes require session authentication (handled by session middleware in Stream 2).
pub fn kiro_routes() -> Router<AppState> {
    Router::new()
        .route("/kiro/status", get(kiro_status))
        .route("/kiro/setup", post(kiro_setup))
        .route("/kiro/poll", post(kiro_poll))
        .route("/kiro/token", delete(kiro_delete_token))
}

// ── Background token refresh ─────────────────────────────────────────

/// Spawn a background task that refreshes Kiro tokens expiring within 5 minutes.
/// Runs every 5 minutes.
pub fn spawn_token_refresh_task(config_db: Arc<ConfigDb>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        let http_client = reqwest::Client::new();

        loop {
            interval.tick().await;

            let expiring = match config_db.get_expiring_kiro_tokens_with_oauth().await {
                Ok(tokens) => tokens,
                Err(e) => {
                    tracing::error!(error = ?e, "Failed to query expiring Kiro tokens");
                    continue;
                }
            };

            if expiring.is_empty() {
                continue;
            }

            tracing::debug!(count = expiring.len(), "Refreshing expiring Kiro tokens");

            // Fallback: load global OAuth creds for users without per-user creds
            let global_client_id = config_db.get("oauth_client_id").await.ok().flatten();
            let global_client_secret = config_db.get("oauth_client_secret").await.ok().flatten();
            let global_sso_region = config_db
                .get("oauth_sso_region")
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| "us-east-1".to_string());

            for (user_id, refresh_token, oauth_cid, oauth_csec, oauth_region) in &expiring {
                // Prefer per-user OAuth creds, fall back to global
                let (client_id, client_secret, sso_region) =
                    match (oauth_cid, oauth_csec, oauth_region) {
                        (Some(cid), Some(csec), Some(region)) => {
                            (cid.as_str(), csec.as_str(), region.as_str())
                        }
                        _ => match (&global_client_id, &global_client_secret) {
                            (Some(cid), Some(csec)) => {
                                (cid.as_str(), csec.as_str(), global_sso_region.as_str())
                            }
                            _ => {
                                tracing::warn!(
                                    user_id = %user_id,
                                    "Cannot refresh token: no OAuth client credentials"
                                );
                                continue;
                            }
                        },
                    };

                let result = refresh_user_token(
                    &http_client,
                    sso_region,
                    client_id,
                    client_secret,
                    refresh_token,
                )
                .await;

                match result {
                    Ok((new_access, new_refresh, new_expiry)) => {
                        let store_refresh = new_refresh.as_deref().unwrap_or(refresh_token);
                        if let Err(e) = config_db
                            .upsert_kiro_token(
                                *user_id,
                                store_refresh,
                                Some(&new_access),
                                Some(new_expiry),
                            )
                            .await
                        {
                            tracing::error!(user_id = %user_id, error = ?e, "Failed to store refreshed Kiro token");
                        } else {
                            tracing::info!(user_id = %user_id, "Kiro token refreshed successfully");
                        }
                    }
                    Err(e) => {
                        tracing::warn!(user_id = %user_id, error = ?e, "kiro_token_refresh_failed");
                        // Mark token as expired
                        if let Err(e2) = config_db.mark_kiro_token_expired(*user_id).await {
                            tracing::error!(user_id = %user_id, error = ?e2, "Failed to mark Kiro token as expired");
                        }
                    }
                }
            }
        }
    });
}

/// Refresh a single user's Kiro token via AWS SSO OIDC.
async fn refresh_user_token(
    http_client: &reqwest::Client,
    sso_region: &str,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> anyhow::Result<(String, Option<String>, chrono::DateTime<Utc>)> {
    let url = format!("https://oidc.{}.amazonaws.com/token", sso_region);

    let body = serde_json::json!({
        "grantType": "refresh_token",
        "clientId": client_id,
        "clientSecret": client_secret,
        "refreshToken": refresh_token,
    });

    let response = http_client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to send refresh request: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        anyhow::bail!("Token refresh failed: {} - {}", status, error_text);
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RefreshResponse {
        access_token: String,
        refresh_token: Option<String>,
        expires_in: Option<u64>,
    }

    let data: RefreshResponse = response
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse refresh response: {}", e))?;

    let expires_in = data.expires_in.unwrap_or(3600);
    let expires_at = Utc::now() + Duration::seconds(expires_in as i64 - 60);

    Ok((data.access_token, data.refresh_token, expires_at))
}

// Note: session-based auth middleware handles user extraction in Stream 2.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kiro_status_response_serialization() {
        let resp = KiroStatusResponse {
            has_token: true,
            expired: false,
            sso_start_url: Some("https://d-123.awsapps.com/start".to_string()),
            sso_region: Some("us-east-1".to_string()),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["has_token"], true);
        assert_eq!(json["expired"], false);
        assert_eq!(json["sso_start_url"], "https://d-123.awsapps.com/start");
        assert_eq!(json["sso_region"], "us-east-1");
    }

    #[test]
    fn test_kiro_status_response_without_sso() {
        let resp = KiroStatusResponse {
            has_token: false,
            expired: false,
            sso_start_url: None,
            sso_region: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["has_token"], false);
        assert!(json.get("sso_start_url").is_none());
        assert!(json.get("sso_region").is_none());
    }

    #[test]
    fn test_kiro_setup_request_deserialization() {
        let json = serde_json::json!({
            "sso_start_url": "https://d-123.awsapps.com/start",
            "sso_region": "us-west-2"
        });
        let req: KiroSetupRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.sso_start_url, "https://d-123.awsapps.com/start");
        assert_eq!(req.sso_region, Some("us-west-2".to_string()));
    }

    #[test]
    fn test_kiro_setup_request_default_region() {
        let json = serde_json::json!({
            "sso_start_url": "https://d-123.awsapps.com/start"
        });
        let req: KiroSetupRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.sso_start_url, "https://d-123.awsapps.com/start");
        assert_eq!(req.sso_region, None);
    }

    #[test]
    fn test_kiro_poll_response_pending() {
        let resp = KiroPollResponse {
            status: "pending".to_string(),
            message: Some("Waiting".to_string()),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "pending");
        assert_eq!(json["message"], "Waiting");
    }

    #[test]
    fn test_kiro_poll_response_no_message() {
        let resp = KiroPollResponse {
            status: "success".to_string(),
            message: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "success");
        assert!(json.get("message").is_none());
    }

    #[test]
    fn test_kiro_setup_response_serialization() {
        let resp = KiroSetupResponse {
            device_code: "ABCD-1234".to_string(),
            user_code: "USER-CODE".to_string(),
            verification_uri: "https://device.sso.us-east-1.amazonaws.com/".to_string(),
            verification_uri_complete:
                "https://device.sso.us-east-1.amazonaws.com/?user_code=USER-CODE".to_string(),
            expires_in: 600,
            interval: 5,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["device_code"], "ABCD-1234");
        assert_eq!(json["user_code"], "USER-CODE");
        assert_eq!(json["expires_in"], 600);
        assert_eq!(json["interval"], 5);
    }

    #[test]
    fn test_kiro_poll_request_deserialization() {
        let json = serde_json::json!({ "device_code": "test-code-123" });
        let req: KiroPollRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.device_code, "test-code-123");
    }
}
