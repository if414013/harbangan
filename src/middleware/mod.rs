// Authentication, CORS, and debug logging middleware

pub mod debug;

use axum::{body::Body, extract::State, http::Request, middleware::Next, response::Response};
use tower_http::cors::{Any, CorsLayer};

use crate::error::ApiError;
use crate::routes::AppState;

pub use debug::debug_middleware;
pub use debug::DEBUG_LOGGER;

/// Authentication middleware
///
/// Verifies the API key in the Authorization header or x-api-key header.
/// Expects format: "Bearer {PROXY_API_KEY}" or just the key in x-api-key.
pub async fn auth_middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let proxy_api_key = state
        .config
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .proxy_api_key
        .clone();

    if let Some(auth_header) = request.headers().get("authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            tracing::debug!("Received Authorization header: Bearer ****");
            let expected = format!("Bearer {}", proxy_api_key);
            tracing::debug!("Expected: ****");
            if auth_str == expected {
                return Ok(next.run(request).await);
            }
        }
    }

    if let Some(api_key_header) = request.headers().get("x-api-key") {
        if let Ok(key_str) = api_key_header.to_str() {
            tracing::debug!("Received x-api-key header: ****");
            tracing::debug!("Expected: ****");
            if key_str == proxy_api_key {
                return Ok(next.run(request).await);
            }
        }
    }

    let path = request.uri().path();
    let method = request.method();
    let request_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
    tracing::warn!(
        "[{}] Access attempt with invalid or missing API key: {} {}",
        request_id,
        method,
        path
    );
    Err(ApiError::AuthError(
        "Invalid or missing API Key".to_string(),
    ))
}

/// HSTS (HTTP Strict Transport Security) middleware
///
/// Adds the Strict-Transport-Security header when TLS is enabled,
/// instructing clients to only use HTTPS for future requests.
pub async fn hsts_middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;

    // Only add HSTS header when TLS is active
    if state
        .config
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .is_tls_active()
    {
        response.headers_mut().insert(
            axum::http::header::STRICT_TRANSPORT_SECURITY,
            axum::http::HeaderValue::from_static("max-age=31536000"),
        );
    }

    response
}

/// Create CORS middleware layer
///
/// Configures CORS to allow all origins, methods, and headers.
/// Handles OPTIONS preflight requests automatically.
pub fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
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
        let auth_manager_for_http = Arc::new(
            AuthManager::new_for_testing("test-token".to_string(), "us-east-1".to_string(), 300)
                .unwrap(),
        );
        let http_client =
            Arc::new(KiroHttpClient::new(auth_manager_for_http, 20, 30, 300, 3).unwrap());
        let auth_manager = Arc::new(tokio::sync::RwLock::new(
            AuthManager::new_for_testing("test-token".to_string(), "us-east-1".to_string(), 300)
                .unwrap(),
        ));
        let resolver = ModelResolver::new(cache.clone(), HashMap::new());
        let config = Config {
            proxy_api_key: "test-key-123".to_string(),
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
        }
    }

    async fn test_handler() -> &'static str {
        "OK"
    }

    fn create_test_app(state: AppState) -> Router {
        Router::new()
            .route("/test", get(test_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_auth_middleware_with_valid_bearer_token() {
        let state = create_test_state();
        let app = create_test_app(state);

        // Create request with valid Bearer token
        let request = Request::builder()
            .uri("/test")
            .header("authorization", "Bearer test-key-123")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auth_middleware_with_valid_x_api_key() {
        let state = create_test_state();
        let app = create_test_app(state);

        // Create request with valid x-api-key
        let request = Request::builder()
            .uri("/test")
            .header("x-api-key", "test-key-123")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auth_middleware_with_invalid_bearer_token() {
        let state = create_test_state();
        let app = create_test_app(state);

        // Create request with invalid Bearer token
        let request = Request::builder()
            .uri("/test")
            .header("authorization", "Bearer wrong-key")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_middleware_with_invalid_x_api_key() {
        let state = create_test_state();
        let app = create_test_app(state);

        // Create request with invalid x-api-key
        let request = Request::builder()
            .uri("/test")
            .header("x-api-key", "wrong-key")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_middleware_with_missing_auth() {
        let state = create_test_state();
        let app = create_test_app(state);

        // Create request without any auth headers
        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_middleware_bearer_without_prefix() {
        let state = create_test_state();
        let app = create_test_app(state);

        // Create request with token but without "Bearer " prefix
        let request = Request::builder()
            .uri("/test")
            .header("authorization", "test-key-123")
            .body(Body::empty())
            .unwrap();

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

    #[tokio::test]
    async fn test_cors_layer_allows_all_methods() {
        let state = create_test_state();
        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(cors_layer())
            .with_state(state);

        // Create OPTIONS request asking for POST method
        let request = Request::builder()
            .method("OPTIONS")
            .uri("/test")
            .header("origin", "https://example.com")
            .header("access-control-request-method", "POST")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();

        // Check that all methods are allowed
        assert!(response
            .headers()
            .contains_key("access-control-allow-methods"));
        let allow_methods = response
            .headers()
            .get("access-control-allow-methods")
            .unwrap();
        let methods_str = allow_methods.to_str().unwrap();

        // tower-http returns "*" for Any
        assert_eq!(methods_str, "*");
    }

    #[tokio::test]
    async fn test_cors_layer_allows_all_headers() {
        let state = create_test_state();
        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(cors_layer())
            .with_state(state);

        // Create OPTIONS request asking for custom headers
        let request = Request::builder()
            .method("OPTIONS")
            .uri("/test")
            .header("origin", "https://example.com")
            .header("access-control-request-method", "POST")
            .header(
                "access-control-request-headers",
                "x-custom-header, authorization",
            )
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();

        // Check that all headers are allowed
        assert!(response
            .headers()
            .contains_key("access-control-allow-headers"));
        let allow_headers = response
            .headers()
            .get("access-control-allow-headers")
            .unwrap();
        let headers_str = allow_headers.to_str().unwrap();

        // tower-http returns "*" for Any
        assert_eq!(headers_str, "*");
    }

    #[tokio::test]
    async fn test_cors_layer_with_different_origins() {
        let state = create_test_state();
        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(cors_layer())
            .with_state(state);

        // Test with different origins
        let origins = vec![
            "https://example.com",
            "http://localhost:3000",
            "https://app.example.org",
        ];

        for origin in origins {
            let request = Request::builder()
                .uri("/test")
                .header("origin", origin)
                .body(Body::empty())
                .unwrap();

            let response = app.clone().oneshot(request).await.unwrap();

            // All origins should be allowed
            assert!(response
                .headers()
                .contains_key("access-control-allow-origin"));
            let allow_origin = response
                .headers()
                .get("access-control-allow-origin")
                .unwrap();
            assert_eq!(allow_origin, "*");
        }
    }

    // HSTS middleware tests

    fn create_test_state_with_tls(tls_enabled: bool) -> AppState {
        let cache = ModelCache::new(3600);
        let auth_manager_for_http = Arc::new(
            AuthManager::new_for_testing("test-token".to_string(), "us-east-1".to_string(), 300)
                .unwrap(),
        );
        let http_client =
            Arc::new(KiroHttpClient::new(auth_manager_for_http, 20, 30, 300, 3).unwrap());
        let auth_manager = Arc::new(tokio::sync::RwLock::new(
            AuthManager::new_for_testing("test-token".to_string(), "us-east-1".to_string(), 300)
                .unwrap(),
        ));
        let resolver = ModelResolver::new(cache.clone(), HashMap::new());
        let config = Config {
            proxy_api_key: "test-key-123".to_string(),
            fake_reasoning_max_tokens: 10000,
            tls_enabled,
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
        }
    }

    #[tokio::test]
    async fn test_hsts_middleware_with_tls_enabled() {
        let state = create_test_state_with_tls(true);
        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                hsts_middleware,
            ))
            .with_state(state);

        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let response = app.clone().oneshot(request).await.unwrap();

        // Check that HSTS header is present
        assert!(response
            .headers()
            .contains_key(axum::http::header::STRICT_TRANSPORT_SECURITY));
        let hsts_header = response
            .headers()
            .get(axum::http::header::STRICT_TRANSPORT_SECURITY)
            .unwrap();
        assert_eq!(hsts_header, "max-age=31536000");
    }

    #[tokio::test]
    async fn test_hsts_middleware_with_tls_disabled() {
        let state = create_test_state_with_tls(false);
        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                hsts_middleware,
            ))
            .with_state(state);

        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let response = app.clone().oneshot(request).await.unwrap();

        // Check that HSTS header is NOT present
        assert!(!response
            .headers()
            .contains_key(axum::http::header::STRICT_TRANSPORT_SECURITY));
    }

    #[tokio::test]
    async fn test_hsts_middleware_header_value() {
        let state = create_test_state_with_tls(true);
        let app = Router::new()
            .route("/test", get(test_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                hsts_middleware,
            ))
            .with_state(state);

        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let response = app.clone().oneshot(request).await.unwrap();

        // Verify the exact header value
        let hsts_header = response
            .headers()
            .get(axum::http::header::STRICT_TRANSPORT_SECURITY)
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(hsts_header, "max-age=31536000");
    }
}
