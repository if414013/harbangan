use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use std::str::FromStr;

use crate::error::ApiError;
use crate::providers::types::ProviderId;
use crate::routes::{AppState, SessionInfo};

// ── Types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderPriority {
    pub provider_id: String,
    pub priority: i32,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePriorityRequest {
    pub priorities: Vec<ProviderPriority>,
}

#[derive(Debug, Serialize)]
pub struct PriorityResponse {
    pub priorities: Vec<ProviderPriority>,
}

// ── Routes ───────────────────────────────────────────────────────

pub fn provider_priority_routes() -> Router<AppState> {
    Router::new()
        .route("/providers/priority", get(get_priority))
        .route("/providers/priority", post(update_priority))
}

// ── GET /providers/priority ──────────────────────────────────────

async fn get_priority(
    State(state): State<AppState>,
    request: axum::http::Request<axum::body::Body>,
) -> Result<Json<PriorityResponse>, ApiError> {
    let session = request
        .extensions()
        .get::<SessionInfo>()
        .ok_or_else(|| ApiError::AuthError("No session".to_string()))?;

    let db = state.require_config_db()?;
    let rows = db
        .get_user_provider_priority(session.user_id)
        .await
        .map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("Failed to get provider priority: {}", e))
        })?;

    let priorities = rows
        .into_iter()
        .map(|(provider_id, priority)| ProviderPriority {
            provider_id,
            priority,
        })
        .collect();

    Ok(Json(PriorityResponse { priorities }))
}

// ── POST /providers/priority ─────────────────────────────────────

async fn update_priority(
    State(state): State<AppState>,
    request: axum::http::Request<axum::body::Body>,
) -> Result<Json<PriorityResponse>, ApiError> {
    let session = request
        .extensions()
        .get::<SessionInfo>()
        .ok_or_else(|| ApiError::AuthError("No session".to_string()))?
        .clone();

    // Parse body
    let body = axum::body::to_bytes(request.into_body(), 1024 * 16)
        .await
        .map_err(|_| ApiError::ValidationError("Invalid request body".to_string()))?;
    let payload: UpdatePriorityRequest = serde_json::from_slice(&body)
        .map_err(|e| ApiError::ValidationError(format!("Invalid JSON: {}", e)))?;

    // Validate
    for p in &payload.priorities {
        ProviderId::from_str(&p.provider_id).map_err(|_| {
            ApiError::ValidationError(format!("Invalid provider: {}", p.provider_id))
        })?;
    }

    let db = state.require_config_db()?;

    // Upsert each priority
    for p in &payload.priorities {
        db.upsert_user_provider_priority(session.user_id, &p.provider_id, p.priority)
            .await
            .map_err(|e| {
                ApiError::Internal(anyhow::anyhow!("Failed to upsert provider priority: {}", e))
            })?;
    }

    // Invalidate provider registry cache so next request picks up new priority
    state.provider_registry.invalidate(session.user_id);

    tracing::debug!(
        user_id = %session.user_id,
        count = payload.priorities.len(),
        "Provider priority updated"
    );

    // Return the full updated list
    let rows = db
        .get_user_provider_priority(session.user_id)
        .await
        .map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("Failed to get provider priority: {}", e))
        })?;

    let priorities = rows
        .into_iter()
        .map(|(provider_id, priority)| ProviderPriority {
            provider_id,
            priority,
        })
        .collect();

    Ok(Json(PriorityResponse { priorities }))
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_priority_serialization() {
        let p = ProviderPriority {
            provider_id: "copilot".to_string(),
            priority: 1,
        };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["provider_id"], "copilot");
        assert_eq!(json["priority"], 1);
    }

    #[test]
    fn test_provider_priority_deserialization() {
        let json = r#"{"provider_id":"anthropic","priority":2}"#;
        let p: ProviderPriority = serde_json::from_str(json).unwrap();
        assert_eq!(p.provider_id, "anthropic");
        assert_eq!(p.priority, 2);
    }

    #[test]
    fn test_update_priority_request_deserialization() {
        let json = r#"{"priorities":[{"provider_id":"copilot","priority":1},{"provider_id":"anthropic","priority":2}]}"#;
        let req: UpdatePriorityRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.priorities.len(), 2);
        assert_eq!(req.priorities[0].provider_id, "copilot");
        assert_eq!(req.priorities[0].priority, 1);
        assert_eq!(req.priorities[1].provider_id, "anthropic");
        assert_eq!(req.priorities[1].priority, 2);
    }

    #[test]
    fn test_priority_response_serialization() {
        let resp = PriorityResponse {
            priorities: vec![
                ProviderPriority {
                    provider_id: "copilot".to_string(),
                    priority: 1,
                },
                ProviderPriority {
                    provider_id: "openai_codex".to_string(),
                    priority: 2,
                },
            ],
        };
        let json = serde_json::to_value(&resp).unwrap();
        let arr = json["priorities"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["provider_id"], "copilot");
        assert_eq!(arr[1]["priority"], 2);
    }

    #[test]
    fn test_valid_providers_via_provider_id() {
        use std::str::FromStr;
        assert!(ProviderId::from_str("kiro").is_ok());
        assert!(ProviderId::from_str("anthropic").is_ok());
        assert!(ProviderId::from_str("openai_codex").is_ok());
        assert!(ProviderId::from_str("gemini").is_err());
        assert!(ProviderId::from_str("copilot").is_ok());
        assert!(ProviderId::from_str("qwen").is_err());
        assert!(ProviderId::from_str("azure").is_err());
    }

    #[test]
    fn test_empty_priorities_request() {
        let json = r#"{"priorities":[]}"#;
        let req: UpdatePriorityRequest = serde_json::from_str(json).unwrap();
        assert!(req.priorities.is_empty());
    }

    #[test]
    fn test_all_visible_providers_count() {
        // Ensure we have exactly 4 visible providers (kiro, anthropic, openai_codex, copilot)
        assert_eq!(ProviderId::all_visible().len(), 4);
    }

    #[test]
    fn test_invalid_provider_rejected_by_provider_id() {
        use std::str::FromStr;
        // Verify that removed or made-up providers are rejected
        assert!(ProviderId::from_str("qwen").is_err());
        assert!(ProviderId::from_str("qwq").is_err());
        assert!(ProviderId::from_str("dashscope").is_err());
    }
}
