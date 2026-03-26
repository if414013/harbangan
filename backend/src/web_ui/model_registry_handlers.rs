use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::providers::types::ProviderId;
use crate::routes::AppState;
use crate::web_ui::config_db::RegistryModel;

// ── Request / Response Types ─────────────────────────────────

/// Response for GET /models/registry — list all registry models.
#[derive(Debug, Serialize)]
pub struct ModelsListResponse {
    pub models: Vec<RegistryModel>,
    pub total: usize,
}

/// Request body for PATCH /models/registry/:id — toggle enabled or update display_name.
#[derive(Debug, Deserialize)]
pub struct UpdateModelRequest {
    pub enabled: Option<bool>,
    pub display_name: Option<String>,
}

/// Response after updating a model.
#[derive(Debug, Serialize)]
pub struct UpdateModelResponse {
    pub success: bool,
    pub id: Uuid,
}

/// Request body for POST /models/registry/populate.
#[derive(Debug, Deserialize)]
pub struct PopulateRequest {
    /// Optional provider_id to populate. If None, populates all providers.
    pub provider_id: Option<String>,
}

/// Response after populating models.
#[derive(Debug, Serialize)]
pub struct PopulateResponse {
    pub success: bool,
    pub models_upserted: usize,
}

/// Response after deleting a model.
#[derive(Debug, Serialize)]
pub struct DeleteModelResponse {
    pub success: bool,
    pub id: Uuid,
}

// ── Route Handlers ──────────────────────────────────────────

/// GET /models/registry — list all models in the registry.
async fn list_models(State(state): State<AppState>) -> impl IntoResponse {
    let db = match state.require_config_db() {
        Ok(db) => db,
        Err(e) => return e.into_response(),
    };

    match db.get_all_registry_models().await {
        Ok(models) => {
            let total = models.len();
            Json(ModelsListResponse { models, total }).into_response()
        }
        Err(e) => {
            tracing::error!(error = ?e, "Failed to list registry models");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to list models"})),
            )
                .into_response()
        }
    }
}

/// PATCH /models/registry/:id — update a model (enable/disable, rename).
async fn update_model(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateModelRequest>,
) -> impl IntoResponse {
    let db = match state.require_config_db() {
        Ok(db) => db,
        Err(e) => return e.into_response(),
    };

    if let Some(enabled) = body.enabled {
        match db.update_model_enabled(id, enabled).await {
            Ok(true) => {}
            Ok(false) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "Model not found"})),
                )
                    .into_response();
            }
            Err(e) => {
                tracing::error!(error = ?e, "Failed to update model enabled");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "Failed to update model"})),
                )
                    .into_response();
            }
        }
    }

    if let Some(ref display_name) = body.display_name {
        match db.update_model_display_name(id, display_name).await {
            Ok(true) => {}
            Ok(false) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "Model not found"})),
                )
                    .into_response();
            }
            Err(e) => {
                tracing::error!(error = ?e, "Failed to update model display_name");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "Failed to update model"})),
                )
                    .into_response();
            }
        }
    }

    // Reload registry cache after mutation
    let _ = state.model_cache.load_from_registry().await;

    Json(UpdateModelResponse { success: true, id }).into_response()
}

/// DELETE /models/registry/:id — remove a model from the registry.
async fn delete_model(State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    let db = match state.require_config_db() {
        Ok(db) => db,
        Err(e) => return e.into_response(),
    };

    match db.delete_registry_model(id).await {
        Ok(true) => {
            // Reload registry cache after deletion
            let _ = state.model_cache.load_from_registry().await;
            Json(DeleteModelResponse { success: true, id }).into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Model not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = ?e, "Failed to delete model");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to delete model"})),
            )
                .into_response()
        }
    }
}

/// POST /models/registry/populate — populate models from API or static data.
async fn populate_models(
    State(state): State<AppState>,
    Json(body): Json<PopulateRequest>,
) -> impl IntoResponse {
    let db = match state.require_config_db() {
        Ok(db) => db,
        Err(e) => return e.into_response(),
    };

    let providers: Vec<&str> = if let Some(ref pid) = body.provider_id {
        vec![pid.as_str()]
    } else {
        ProviderId::all_visible()
            .iter()
            .map(|p| p.as_str())
            .collect()
    };

    let mut total_upserted = 0usize;
    for provider_id in &providers {
        // For kiro, snapshot auth_manager credentials before the long-running populate call.
        // For other providers, pass None — populate_provider handles its own credential lookup.
        let auth_manager_ref = if *provider_id == "kiro" {
            let guard = state.auth_manager.read().await;
            if guard.has_credentials().await {
                Some(guard)
            } else {
                None
            }
        } else {
            None
        };

        let result = crate::web_ui::model_registry::populate_provider(
            provider_id,
            &db,
            &state.http_client,
            auth_manager_ref.as_deref(),
        )
        .await
        .map_err(|e| e.to_string());

        // Drop the guard explicitly (if held) before processing the result
        drop(auth_manager_ref);

        match result {
            Ok(count) => {
                total_upserted += count;
                tracing::info!(provider = provider_id, count, "Populated models");
            }
            Err(e) => {
                tracing::warn!(provider = provider_id, error = %e, "Failed to populate");
            }
        }
    }

    // Reload registry cache after population
    let _ = state.model_cache.load_from_registry().await;

    Json(PopulateResponse {
        success: true,
        models_upserted: total_upserted,
    })
    .into_response()
}

// ── Router ───────────────────────────────────────────────────

/// Model registry routes, nested under `/models/registry` (session-authenticated).
pub fn model_registry_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_models))
        .route("/{id}", patch(update_model))
        .route("/{id}", delete(delete_model))
        .route("/populate", post(populate_models))
}
