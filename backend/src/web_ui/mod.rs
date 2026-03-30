pub mod admin_pool;
pub mod api_keys;
pub mod config_api;
pub mod config_db;
pub mod copilot_auth;
pub mod crypto;
pub mod google_auth;
pub mod model_registry;
pub mod model_registry_handlers;
pub mod model_visibility_handlers;
pub mod password_auth;
pub mod provider_oauth;
pub mod provider_priority;
pub mod proxy_relay;
pub mod routes;
pub mod session;
pub mod static_models;
pub mod usage;
pub mod user_kiro;
pub mod users;

use std::sync::atomic::Ordering;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
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
///   Kiro token management, API key management, model registry.
/// Admin-only routes: domain allowlist management.
/// All mutating endpoints require CSRF validation.
pub fn web_ui_routes(state: AppState) -> Router {
    // --- Session-authenticated API routes (+ CSRF on mutations) ---
    let session_api_routes = Router::new()
        // Read-only (GET)
        .route("/system", get(routes::get_system_info))
        .route("/models", get(routes::get_models))
        .route("/auth/me", get(google_auth::auth_me))
        // (logout moved to its own group — no session required)
        // Stream 3: per-user Kiro token + API key routes
        .merge(user_kiro::kiro_routes())
        .merge(api_keys::api_key_routes())
        // Multi-provider: per-user provider OAuth management
        .merge(provider_oauth::provider_oauth_routes())
        // Copilot: GitHub OAuth connect/callback/status/disconnect
        .merge(copilot_auth::copilot_routes())
        // Provider priority management
        .merge(provider_priority::provider_priority_routes())
        // Multi-account: per-user account management + rate limits
        .merge(admin_pool::user_account_routes())
        .merge(admin_pool::rate_limit_routes())
        // Model registry (session-authenticated, all users)
        .nest(
            "/models/registry",
            model_registry_handlers::model_registry_routes(),
        )
        // Usage tracking (session-authenticated, own usage only)
        .route("/usage", get(usage::get_usage))
        // Google account linking (session-authenticated)
        .route("/auth/google/link", get(google_auth::google_link_redirect))
        // Password auth: 2FA setup/verify, password change (session-authenticated)
        .route("/auth/2fa/setup", get(password_auth::setup_2fa_handler))
        .route("/auth/2fa/verify", post(password_auth::verify_2fa_handler))
        .route(
            "/auth/password/change",
            post(password_auth::change_password_handler),
        )
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
        .route(
            "/config",
            get(routes::get_config).put(routes::update_config),
        )
        .route("/config/schema", get(routes::get_config_schema))
        .route("/config/history", get(routes::get_config_history))
        .merge(config_api::domain_routes())
        .merge(config_api::user_routes())
        .merge(crate::guardrails::api::guardrails_routes())
        // Usage tracking (admin only - global stats)
        .route("/admin/usage", get(usage::get_admin_usage))
        .route("/admin/usage/users", get(usage::get_admin_usage_by_users))
        // Admin pool management
        .merge(admin_pool::admin_pool_routes())
        // Model registry mutations (admin-only)
        .nest(
            "/models/registry",
            model_registry_handlers::model_registry_admin_routes(),
        )
        // Model visibility defaults (admin-only)
        .nest(
            "/models/visibility-defaults",
            model_visibility_handlers::visibility_defaults_routes(),
        )
        // Admin password auth: create users, reset passwords
        .route(
            "/admin/users/create",
            post(password_auth::admin_create_user_handler),
        )
        .route(
            "/admin/users/:id/reset-password",
            post(password_auth::admin_reset_password_handler),
        )
        .route(
            "/admin/providers/:provider_id",
            patch(provider_oauth::toggle_provider_enabled),
        )
        .layer(axum::middleware::from_fn(google_auth::admin_middleware))
        .layer(axum::middleware::from_fn(google_auth::csrf_middleware))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            google_auth::session_middleware,
        ))
        .with_state(state.clone());

    // --- Logout route (CSRF only, no session required) ---
    // Logout must be reachable even with an expired/invalid session so it can
    // always clear cookies and delete the DB session row.
    let logout_routes = Router::new()
        .route("/auth/logout", post(google_auth::logout_with_session))
        .layer(axum::middleware::from_fn(google_auth::csrf_middleware))
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
        // Password auth: login + 2FA completion (no session required)
        .route("/auth/login", post(password_auth::login_handler))
        .route("/auth/login/2fa", post(password_auth::login_2fa_handler))
        .with_state(state.clone());

    Router::new()
        .nest("/_ui/api", session_api_routes)
        .merge(Router::new().nest("/_ui/api", admin_api_routes))
        .merge(Router::new().nest("/_ui/api", logout_routes))
        .merge(Router::new().nest("/_ui/api", public_api_routes))
}
