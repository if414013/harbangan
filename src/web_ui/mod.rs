pub mod config_api;
pub mod config_db;
pub mod routes;
pub mod sse;

use std::sync::atomic::Ordering;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

use crate::routes::AppState;

/// Middleware that blocks proxy endpoints (`/v1/*`) with 503 when setup is incomplete.
/// Wire this into the main router via `axum::middleware::from_fn_with_state`.
#[allow(dead_code)]
pub async fn setup_guard(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let setup_complete = state.setup_complete.load(Ordering::Relaxed);
    let path = request.uri().path();

    if !setup_complete && path.starts_with("/v1") {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": {
                    "message": "Setup required. Please complete setup at /_ui/",
                    "type": "setup_required"
                }
            })),
        )
            .into_response();
    }

    next.run(request).await
}

/// Build the web UI router with all /_ui/ routes.
/// Setup and config-read endpoints are unauthenticated so the frontend
/// can check setup status and submit initial configuration.
/// All other API routes require auth.
pub fn web_ui_routes(state: AppState) -> Router {
    use crate::middleware;

    // API routes that require auth
    let authed_api_routes = Router::new()
        .route("/metrics", get(routes::get_metrics))
        .route("/system", get(routes::get_system_info))
        .route("/models", get(routes::get_models))
        .route("/logs", get(routes::get_logs))
        .route("/config", axum::routing::put(routes::update_config))
        .route("/config/history", get(routes::get_config_history))
        .route("/stream/metrics", get(sse::metrics_stream))
        .route("/stream/logs", get(sse::logs_stream))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::auth_middleware,
        ))
        .with_state(state.clone());

    // API routes that work without auth (setup flow + config read)
    let public_api_routes = Router::new()
        .route("/setup", post(routes::setup))
        .route("/config", get(routes::get_config))
        .route("/config/schema", get(routes::get_config_schema))
        .with_state(state.clone());

    // HTML pages + static assets (no auth)
    let page_routes = Router::new()
        .route("/", get(routes::dashboard_page))
        .route("/config", get(routes::config_page))
        .route("/assets/{filename}", get(routes::static_asset));

    Router::new()
        .nest("/_ui/api", authed_api_routes)
        .nest("/_ui/api", public_api_routes)
        .nest("/_ui", page_routes)
}
