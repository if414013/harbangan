use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::routes::AppState;

// ── Request / Response Types ─────────────────────────────────

/// Request body for PUT /visibility-defaults/:provider_id.
#[derive(Debug, Deserialize)]
pub struct SetVisibilityDefaultsRequest {
    pub model_ids: Vec<String>,
}

/// Response for GET /visibility-defaults.
#[derive(Debug, Serialize)]
pub struct AllVisibilityDefaultsResponse {
    pub defaults: HashMap<String, Vec<String>>,
}

/// Response for PUT /visibility-defaults/:provider_id.
#[derive(Debug, Serialize)]
pub struct SetVisibilityDefaultsResponse {
    pub success: bool,
    pub provider_id: String,
    pub count: usize,
}

/// Response for DELETE /visibility-defaults/:provider_id.
#[derive(Debug, Serialize)]
pub struct DeleteVisibilityDefaultsResponse {
    pub success: bool,
    pub provider_id: String,
}

/// Response for POST /visibility-defaults/:provider_id/apply.
#[derive(Debug, Serialize)]
pub struct ApplyVisibilityDefaultsResponse {
    pub success: bool,
    pub provider_id: String,
    pub enabled: usize,
    pub disabled: usize,
}

/// Per-provider result in apply-all response.
#[derive(Debug, Serialize)]
pub struct ProviderApplyResult {
    pub provider_id: String,
    pub enabled: usize,
    pub disabled: usize,
}

/// Response for POST /visibility-defaults/apply-all.
#[derive(Debug, Serialize)]
pub struct ApplyAllVisibilityDefaultsResponse {
    pub success: bool,
    pub results: Vec<ProviderApplyResult>,
}

// ── Route Handlers ──────────────────────────────────────────

/// GET /visibility-defaults — list all visibility defaults grouped by provider.
async fn get_all_defaults(State(state): State<AppState>) -> impl IntoResponse {
    let db = match state.require_config_db() {
        Ok(db) => db,
        Err(e) => return e.into_response(),
    };

    match db.get_all_visibility_defaults().await {
        Ok(defaults) => Json(AllVisibilityDefaultsResponse { defaults }).into_response(),
        Err(e) => {
            tracing::error!(error = ?e, "Failed to get visibility defaults");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to get visibility defaults"})),
            )
                .into_response()
        }
    }
}

/// PUT /visibility-defaults/:provider_id — replace visibility defaults for a provider.
async fn set_defaults(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
    Json(body): Json<SetVisibilityDefaultsRequest>,
) -> impl IntoResponse {
    let db = match state.require_config_db() {
        Ok(db) => db,
        Err(e) => return e.into_response(),
    };

    match db
        .set_visibility_defaults(&provider_id, &body.model_ids)
        .await
    {
        Ok(count) => {
            tracing::info!(provider = %provider_id, count, "Set visibility defaults");
            Json(SetVisibilityDefaultsResponse {
                success: true,
                provider_id,
                count,
            })
            .into_response()
        }
        Err(e) => {
            tracing::error!(error = ?e, provider = %provider_id, "Failed to set visibility defaults");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to set visibility defaults"})),
            )
                .into_response()
        }
    }
}

/// DELETE /visibility-defaults/:provider_id — remove all visibility defaults for a provider.
async fn delete_defaults(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> impl IntoResponse {
    let db = match state.require_config_db() {
        Ok(db) => db,
        Err(e) => return e.into_response(),
    };

    match db.delete_visibility_defaults(&provider_id).await {
        Ok(_) => {
            tracing::info!(provider = %provider_id, "Deleted visibility defaults");
            Json(DeleteVisibilityDefaultsResponse {
                success: true,
                provider_id,
            })
            .into_response()
        }
        Err(e) => {
            tracing::error!(error = ?e, provider = %provider_id, "Failed to delete visibility defaults");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to delete visibility defaults"})),
            )
                .into_response()
        }
    }
}

/// POST /visibility-defaults/:provider_id/apply — apply defaults for one provider.
async fn apply_defaults(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> impl IntoResponse {
    let db = match state.require_config_db() {
        Ok(db) => db,
        Err(e) => return e.into_response(),
    };

    match db.apply_visibility_defaults(&provider_id).await {
        Ok((enabled, disabled)) => {
            // Reload registry cache after applying
            let _ = state.model_cache.load_from_registry().await;
            tracing::info!(provider = %provider_id, enabled, disabled, "Applied visibility defaults");
            Json(ApplyVisibilityDefaultsResponse {
                success: true,
                provider_id,
                enabled,
                disabled,
            })
            .into_response()
        }
        Err(e) => {
            tracing::error!(error = ?e, provider = %provider_id, "Failed to apply visibility defaults");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to apply visibility defaults"})),
            )
                .into_response()
        }
    }
}

/// POST /visibility-defaults/apply-all — apply defaults for all providers.
async fn apply_all_defaults(State(state): State<AppState>) -> impl IntoResponse {
    let db = match state.require_config_db() {
        Ok(db) => db,
        Err(e) => return e.into_response(),
    };

    match db.apply_all_visibility_defaults().await {
        Ok(results) => {
            // Reload registry cache after applying
            let _ = state.model_cache.load_from_registry().await;
            let results: Vec<ProviderApplyResult> = results
                .into_iter()
                .map(|(provider_id, enabled, disabled)| ProviderApplyResult {
                    provider_id,
                    enabled,
                    disabled,
                })
                .collect();
            tracing::info!(count = results.len(), "Applied all visibility defaults");
            Json(ApplyAllVisibilityDefaultsResponse {
                success: true,
                results,
            })
            .into_response()
        }
        Err(e) => {
            tracing::error!(error = ?e, "Failed to apply all visibility defaults");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to apply all visibility defaults"})),
            )
                .into_response()
        }
    }
}

// ── Router ───────────────────────────────────────────────────

/// Visibility defaults routes (admin-only), nested under `/visibility-defaults`.
pub fn visibility_defaults_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(get_all_defaults))
        .route("/:provider_id", put(set_defaults).delete(delete_defaults))
        .route("/:provider_id/apply", post(apply_defaults))
        .route("/apply-all", post(apply_all_defaults))
}
