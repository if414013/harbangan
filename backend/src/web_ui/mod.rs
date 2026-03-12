pub mod api_keys;
pub mod config_api;
pub mod config_db;
pub mod copilot_auth;
pub mod google_auth;
pub mod model_registry;
pub mod model_registry_handlers;
pub mod provider_oauth;
pub mod provider_priority;
pub mod qwen_auth;
pub mod routes;
pub mod session;
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
        .route("/system", get(routes::get_system_info))
        .route("/models", get(routes::get_models))
        .route("/config", get(routes::get_config))
        .route("/config/schema", get(routes::get_config_schema))
        .route("/config/history", get(routes::get_config_history))
        .route("/auth/me", get(google_auth::auth_me))
        // Mutating endpoints (need CSRF)
        .route("/auth/logout", post(google_auth::logout_with_session))
        // Stream 3: per-user Kiro token + API key routes
        .merge(user_kiro::kiro_routes())
        .merge(api_keys::api_key_routes())
        // Multi-provider: per-user provider OAuth management
        .merge(provider_oauth::provider_oauth_routes())
        // Copilot: GitHub OAuth connect/callback/status/disconnect
        .merge(copilot_auth::copilot_routes())
        // Qwen: device flow connect/poll/status/disconnect
        .merge(qwen_auth::qwen_auth_routes())
        // Provider priority management
        .merge(provider_priority::provider_priority_routes())
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
        .merge(config_api::user_routes())
        .merge(crate::guardrails::api::guardrails_routes())
        .nest("/admin/mcp", crate::mcp::api::mcp_admin_routes())
        .nest(
            "/admin/models",
            model_registry_handlers::model_registry_routes(),
        )
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
        // Provider OAuth relay routes (authenticated by relay_token, not session)
        .merge(provider_oauth::provider_oauth_public_routes())
        .with_state(state.clone());

    Router::new()
        .nest("/_ui/api", session_api_routes)
        .merge(Router::new().nest("/_ui/api", admin_api_routes))
        .merge(Router::new().nest("/_ui/api", public_api_routes))
}
