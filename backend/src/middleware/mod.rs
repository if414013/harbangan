// Authentication, CORS, and debug logging middleware

pub mod debug;

use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::extract::State;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use dashmap::DashMap;
use sha2::{Digest, Sha256};
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};
use uuid::Uuid;

use crate::error::ApiError;
use crate::routes::{AppState, UserKiroCreds};
use crate::web_ui::config_db::ConfigDb;

/// In-memory tracker for API key last-touched times (debounce DB writes).
static API_KEY_LAST_TOUCHED: std::sync::LazyLock<DashMap<Uuid, Instant>> =
    std::sync::LazyLock::new(DashMap::new);

/// Minimum interval between touch_api_key DB writes (5 minutes).
const TOUCH_DEBOUNCE_SECS: u64 = 300;

pub use debug::debug_middleware;
pub use debug::DEBUG_LOGGER;

/// API key authentication middleware for /v1/* proxy routes.
///
/// Extracts the API key from Authorization: Bearer, x-api-key header,
/// or api_key query parameter. SHA-256 hashes it and looks up in
/// the api_key_cache (with DB fallback). If found, resolves the user's
/// Kiro tokens and injects `UserKiroCreds` into request extensions.
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let raw_key = extract_api_key(&request).ok_or_else(|| {
        let path = request.uri().path();
        let method = request.method();
        let request_id = Uuid::new_v4().to_string()[..8].to_string();
        tracing::warn!(
            "[{}] Access attempt with invalid or missing API key: {} {}",
            request_id,
            method,
            path
        );
        ApiError::AuthError("Invalid or missing API Key".to_string())
    })?;

    // SHA-256 hash the key
    let key_hash = hex::encode(Sha256::digest(raw_key.as_bytes()));

    // Look up in cache first, using constant-time comparison for the hash
    let (user_id, key_id) = {
        let mut found = None;
        for entry in state.api_key_cache.iter() {
            if crate::web_ui::api_keys::constant_time_verify(&key_hash, entry.key()) {
                found = Some(*entry.value());
                break;
            }
        }
        if let Some(val) = found {
            val
        } else {
            // Fallback to DB
            let config_db = require_config_db(&state)?;
            let (found_key_id, found_user_id) = config_db
                .get_api_key_by_hash(&key_hash)
                .await
                .map_err(ApiError::Internal)?
                .ok_or_else(|| ApiError::AuthError("Invalid API key".to_string()))?;

            // Cache the result (bounded to 10,000 entries)
            if state.api_key_cache.len() < 10_000 {
                state
                    .api_key_cache
                    .insert(key_hash.clone(), (found_user_id, found_key_id));
            }

            (found_user_id, found_key_id)
        }
    };

    // Update last_used timestamp (debounced — at most once per 5 min per key)
    let should_touch = {
        let now = Instant::now();
        let mut needs_update = true;
        if let Some(last) = API_KEY_LAST_TOUCHED.get(&key_id) {
            if now.duration_since(*last).as_secs() < TOUCH_DEBOUNCE_SECS {
                needs_update = false;
            }
        }
        if needs_update {
            API_KEY_LAST_TOUCHED.insert(key_id, now);
        }
        needs_update
    };
    if should_touch {
        if let Some(ref db) = state.config_db {
            let db = Arc::clone(db);
            let kid = key_id;
            tokio::spawn(async move {
                let _ = db.touch_api_key(kid).await;
            });
        }
    }

    // Resolve user's Kiro tokens (cached in memory, 4-min TTL)
    let (token, refresh_token, region) = {
        // Check kiro_token_cache first
        let cached = state.kiro_token_cache.get(&user_id).and_then(|entry| {
            let (ref access_token, ref region, cached_at) = *entry;
            // 4-minute cache TTL (tokens refresh every 5 min)
            if cached_at.elapsed().as_secs() < 240 {
                Some((access_token.clone(), region.clone()))
            } else {
                None
            }
        });

        if let Some((access_token, region)) = cached {
            (access_token, String::new(), region)
        } else {
            let config_db = require_config_db(&state)?;
            let kiro_tokens = config_db
                .get_kiro_token(user_id)
                .await
                .map_err(ApiError::Internal)?
                .ok_or(ApiError::KiroTokenRequired)?;

            let (refresh_tok, access_token, _token_expiry) = kiro_tokens;
            let tok = access_token.ok_or(ApiError::KiroTokenExpired)?;

            let region = state
                .config
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .kiro_region
                .clone();

            // Cache the token (bounded to 10,000 entries)
            if state.kiro_token_cache.len() < 10_000 {
                state
                    .kiro_token_cache
                    .insert(user_id, (tok.clone(), region.clone(), Instant::now()));
            }

            (tok, refresh_tok, region)
        }
    };

    let creds = UserKiroCreds {
        user_id,
        access_token: token,
        refresh_token,
        region,
    };

    request.extensions_mut().insert(creds);
    Ok(next.run(request).await)
}

/// Extract the raw API key from the request (Authorization: Bearer, x-api-key, or query param).
fn extract_api_key(request: &Request<Body>) -> Option<String> {
    // Authorization: Bearer <key>
    if let Some(auth_header) = request.headers().get("authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(key) = auth_str.strip_prefix("Bearer ") {
                if !key.is_empty() {
                    return Some(key.to_string());
                }
            }
        }
    }

    // x-api-key header
    if let Some(api_key_header) = request.headers().get("x-api-key") {
        if let Ok(key_str) = api_key_header.to_str() {
            if !key_str.is_empty() {
                return Some(key_str.to_string());
            }
        }
    }

    // Query parameter: api_key=<key>
    if let Some(query) = request.uri().query() {
        for param in query.split('&') {
            if let Some(key) = param.strip_prefix("api_key=") {
                let decoded = urlencoding::decode(key).unwrap_or_default();
                if !decoded.is_empty() {
                    return Some(decoded.into_owned());
                }
            }
        }
    }

    None
}

/// HSTS (HTTP Strict Transport Security) middleware
///
/// Always adds the Strict-Transport-Security header since TLS is always on.
pub async fn hsts_middleware(
    State(_state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;

    response.headers_mut().insert(
        axum::http::header::STRICT_TRANSPORT_SECURITY,
        axum::http::HeaderValue::from_static("max-age=31536000"),
    );

    response
}

/// Create CORS middleware layer.
///
/// For API proxy routes (/v1/*), allows all origins/methods/headers (clients
/// send API keys, not cookies).
pub fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::any())
        .allow_methods(AllowMethods::any())
        .allow_headers(AllowHeaders::any())
}

/// Create CORS middleware layer for the web UI.
#[allow(dead_code)]
///
/// Uses the origin derived from GOOGLE_CALLBACK_URL and allows credentials.
pub fn web_ui_cors_layer(callback_url: Option<&str>) -> CorsLayer {
    let origin = callback_url
        .map(crate::web_ui::google_auth::derive_origin)
        .unwrap_or_else(|| "http://localhost:9001".to_string());

    CorsLayer::new()
        .allow_origin(
            origin
                .parse::<axum::http::HeaderValue>()
                .unwrap_or_else(|_| axum::http::HeaderValue::from_static("http://localhost:9001")),
        )
        .allow_methods(AllowMethods::any())
        .allow_headers(AllowHeaders::any())
        .allow_credentials(true)
}

fn require_config_db(state: &AppState) -> Result<Arc<ConfigDb>, ApiError> {
    state.require_config_db()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        auth::AuthManager, cache::ModelCache, config::Config, http_client::KiroHttpClient,
        resolver::ModelResolver,
    };
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::get,
        Router,
    };
    use std::collections::HashMap;
    use std::sync::Arc;
    use tower::util::ServiceExt;

    fn create_test_state() -> AppState {
        let cache = ModelCache::new(3600);
        let http_client = Arc::new(KiroHttpClient::new(20, 30, 300, 3).unwrap());
        let auth_manager = Arc::new(tokio::sync::RwLock::new(
            AuthManager::new_for_testing("test-token".to_string(), "us-east-1".to_string(), 300)
                .unwrap(),
        ));
        let resolver = ModelResolver::new(cache.clone(), HashMap::new());
        let config = Config {
            fake_reasoning_max_tokens: 10000,
            ..Config::with_defaults()
        };

        let metrics = Arc::new(crate::metrics::MetricsCollector::new());

        AppState {
            model_cache: cache,
            auth_manager,
            http_client,
            resolver,
            config: Arc::new(std::sync::RwLock::new(config)),
            setup_complete: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            metrics,
            log_buffer: Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
            config_db: None,
            session_cache: Arc::new(dashmap::DashMap::new()),
            api_key_cache: Arc::new(dashmap::DashMap::new()),
            kiro_token_cache: Arc::new(dashmap::DashMap::new()),
            oauth_pending: Arc::new(dashmap::DashMap::new()),
        }
    }

    async fn test_handler() -> &'static str {
        "OK"
    }

    #[tokio::test]
    async fn test_auth_middleware_with_missing_auth() {
        let state = create_test_state();
        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ))
            .with_state(state);

        // Create request without any auth headers
        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // CORS middleware tests

    #[tokio::test]
    async fn test_cors_layer_allows_all_origins() {
        let state = create_test_state();
        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(cors_layer())
            .with_state(state);

        // Create request with Origin header
        let request = Request::builder()
            .uri("/test")
            .header("origin", "https://example.com")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();

        // Check that CORS headers are present
        assert!(response
            .headers()
            .contains_key("access-control-allow-origin"));
        let allow_origin = response
            .headers()
            .get("access-control-allow-origin")
            .unwrap();
        assert_eq!(allow_origin, "*");
    }

    #[tokio::test]
    async fn test_cors_layer_handles_preflight_options() {
        let state = create_test_state();
        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(cors_layer())
            .with_state(state);

        // Create OPTIONS preflight request
        let request = Request::builder()
            .method("OPTIONS")
            .uri("/test")
            .header("origin", "https://example.com")
            .header("access-control-request-method", "POST")
            .header("access-control-request-headers", "content-type")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();

        // Check that preflight response has correct status
        assert_eq!(response.status(), StatusCode::OK);

        // Check CORS headers
        assert!(response
            .headers()
            .contains_key("access-control-allow-origin"));
        assert!(response
            .headers()
            .contains_key("access-control-allow-methods"));
        assert!(response
            .headers()
            .contains_key("access-control-allow-headers"));
    }

    // HSTS middleware tests

    #[tokio::test]
    async fn test_hsts_middleware_always_adds_header() {
        let state = create_test_state();
        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                hsts_middleware,
            ))
            .with_state(state);

        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let response = app.clone().oneshot(request).await.unwrap();

        // HSTS header is always present (TLS is always on)
        assert!(response
            .headers()
            .contains_key(axum::http::header::STRICT_TRANSPORT_SECURITY));
        let hsts_header = response
            .headers()
            .get(axum::http::header::STRICT_TRANSPORT_SECURITY)
            .unwrap();
        assert_eq!(hsts_header, "max-age=31536000");
    }

    // ── API key auth middleware tests ─────────────────────────────────

    #[tokio::test]
    async fn test_auth_middleware_valid_cached_key() {
        use sha2::{Digest, Sha256};

        let state = create_test_state();
        let user_id = uuid::Uuid::new_v4();
        let key_id = uuid::Uuid::new_v4();
        let raw_key = "test-api-key-12345";
        let key_hash = hex::encode(Sha256::digest(raw_key.as_bytes()));

        // Insert into api_key_cache
        state.api_key_cache.insert(key_hash, (user_id, key_id));

        // Insert Kiro token for this user into kiro_token_cache
        state.kiro_token_cache.insert(
            user_id,
            (
                "fake-access-token".to_string(),
                "us-east-1".to_string(),
                std::time::Instant::now(),
            ),
        );

        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ))
            .with_state(state);

        let request = Request::builder()
            .uri("/test")
            .header("authorization", format!("Bearer {}", raw_key))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auth_middleware_invalid_key() {
        let state = create_test_state();
        // No keys in cache, no DB → cache miss falls through to DB lookup.
        // Without config_db, require_config_db() returns 500 (ConfigError).
        // The key point: the request is rejected (not 200).

        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ))
            .with_state(state);

        let request = Request::builder()
            .uri("/test")
            .header("authorization", "Bearer unknown-key-value")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        // Without a DB, unrecognized keys get 500 (config_db unavailable).
        // The essential behavior: request is NOT allowed through.
        assert_ne!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auth_middleware_xapikey_header() {
        use sha2::{Digest, Sha256};

        let state = create_test_state();
        let user_id = uuid::Uuid::new_v4();
        let key_id = uuid::Uuid::new_v4();
        let raw_key = "my-xapikey-test";
        let key_hash = hex::encode(Sha256::digest(raw_key.as_bytes()));

        state.api_key_cache.insert(key_hash, (user_id, key_id));
        state.kiro_token_cache.insert(
            user_id,
            (
                "fake-access-token".to_string(),
                "us-east-1".to_string(),
                std::time::Instant::now(),
            ),
        );

        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ))
            .with_state(state);

        let request = Request::builder()
            .uri("/test")
            .header("x-api-key", raw_key)
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auth_middleware_bearer_token() {
        use sha2::{Digest, Sha256};

        let state = create_test_state();
        let user_id = uuid::Uuid::new_v4();
        let key_id = uuid::Uuid::new_v4();
        let raw_key = "my-bearer-test-key";
        let key_hash = hex::encode(Sha256::digest(raw_key.as_bytes()));

        state.api_key_cache.insert(key_hash, (user_id, key_id));
        state.kiro_token_cache.insert(
            user_id,
            (
                "fake-access-token".to_string(),
                "us-east-1".to_string(),
                std::time::Instant::now(),
            ),
        );

        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ))
            .with_state(state);

        let request = Request::builder()
            .uri("/test")
            .header("authorization", format!("Bearer {}", raw_key))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
