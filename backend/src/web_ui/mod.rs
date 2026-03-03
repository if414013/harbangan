pub mod api_keys;
pub mod config_api;
pub mod config_db;
pub mod google_auth;
pub mod routes;
pub mod session;
pub mod sse;
pub mod user_kiro;
pub mod users;

use std::sync::atomic::Ordering;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
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
///
/// Public routes (no auth): status, Google auth redirect/callback.
/// Session-authenticated routes: metrics, system, models, logs, config, auth/me, auth/logout,
///   Kiro token management, API key management.
/// Admin-only routes: domain allowlist management.
/// All mutating endpoints require CSRF validation.
pub fn web_ui_routes(state: AppState) -> Router {
    // --- Session-authenticated API routes (+ CSRF on mutations) ---
    let session_api_routes = Router::new()
        // Read-only (GET)
        .route("/metrics", get(routes::get_metrics))
        .route("/system", get(routes::get_system_info))
        .route("/models", get(routes::get_models))
        .route("/logs", get(routes::get_logs))
        .route("/config", get(routes::get_config))
        .route("/config/schema", get(routes::get_config_schema))
        .route("/config/history", get(routes::get_config_history))
        .route("/auth/me", get(google_auth::auth_me))
        // SSE streams (GET, authenticated via session cookie)
        .route("/stream/metrics", get(sse::metrics_stream))
        .route("/stream/logs", get(sse::logs_stream))
        // Mutating endpoints (need CSRF)
        .route("/auth/logout", post(google_auth::logout_with_session))
        // Stream 3: per-user Kiro token + API key routes
        .merge(user_kiro::kiro_routes())
        .merge(api_keys::api_key_routes())
        // Session + CSRF middleware stack
        .layer(axum::middleware::from_fn(google_auth::csrf_middleware))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            google_auth::session_middleware,
        ))
        .with_state(state.clone());

    // --- Admin-only routes (session + admin check + CSRF) ---
    // Config update + domain allowlist management require admin role.
    // Nested at /_ui/api (same prefix) so PUT /_ui/api/config stays at the same URL.
    let admin_api_routes = Router::new()
        .route("/config", put(routes::update_config))
        .merge(config_api::domain_routes())
        .merge(crate::guardrails::api::guardrails_routes())
        .layer(axum::middleware::from_fn(google_auth::admin_middleware))
        .layer(axum::middleware::from_fn(google_auth::csrf_middleware))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            google_auth::session_middleware,
        ))
        .with_state(state.clone());

    // --- Public API routes (no auth required) ---
    let public_api_routes = Router::new()
        .route("/status", get(google_auth::status))
        .route("/auth/google", get(google_auth::google_auth_redirect))
        .route(
            "/auth/google/callback",
            get(google_auth::google_auth_callback),
        )
        .with_state(state.clone());

    Router::new()
        .nest("/_ui/api", session_api_routes)
        .nest("/_ui/api", admin_api_routes)
        .nest("/_ui/api", public_api_routes)
}
