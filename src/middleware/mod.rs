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
    if let Some(auth_header) = request.headers().get("authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            tracing::debug!("Received Authorization header: {}", auth_str);
            let expected = format!("Bearer {}", state.proxy_api_key);
            tracing::debug!("Expected: {}", expected);
            if auth_str == expected {
                return Ok(next.run(request).await);
            }
        }
    }

    if let Some(api_key_header) = request.headers().get("x-api-key") {
        if let Ok(key_str) = api_key_header.to_str() {
            tracing::debug!("Received x-api-key header: {}", key_str);
            tracing::debug!("Expected: {}", state.proxy_api_key);
            if key_str == state.proxy_api_key {
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
        let auth_manager = Arc::new(
            AuthManager::new_for_testing("test-token".to_string(), "us-east-1".to_string(), 300)
                .unwrap(),
        );
        let http_client =
            Arc::new(KiroHttpClient::new(auth_manager.clone(), 20, 30, 300, 3).unwrap());
        let resolver = ModelResolver::new(cache.clone(), HashMap::new());
        let config = Arc::new(Config {
            server_host: "0.0.0.0".to_string(),
            server_port: 8000,
            proxy_api_key: "test-key-123".to_string(),
            kiro_region: "us-east-1".to_string(),
            kiro_cli_db_file: std::path::PathBuf::from("/tmp/test.db"),
            streaming_timeout: 300,
            token_refresh_threshold: 300,
            first_token_timeout: 15,
            http_max_connections: 20,
            http_connect_timeout: 30,
            http_request_timeout: 300,
            http_max_retries: 3,
            debug_mode: crate::config::DebugMode::Off,
            log_level: "info".to_string(),
            tool_description_max_length: 10000,
            fake_reasoning_enabled: false,
            fake_reasoning_max_tokens: 10000,
            fake_reasoning_handling: crate::config::FakeReasoningHandling::AsReasoningContent,
            dashboard: false,
            tls_enabled: false,
            tls_cert_path: None,
            tls_key_path: None,
        });

        let metrics = Arc::new(crate::metrics::MetricsCollector::new());

        AppState {
            proxy_api_key: "test-key-123".to_string(),
            model_cache: cache,
            auth_manager,
            http_client,
            resolver,
            config,
            metrics,
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
}
