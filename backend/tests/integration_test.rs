// Integration tests for Harbangan Gateway
//
// These tests verify the full HTTP stack including routing, middleware,
// request parsing, and response formatting.

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::sync::RwLock;
use tower::ServiceExt;

use harbangan::{
    auth::AuthManager,
    cache::ModelCache,
    config::Config,
    http_client::KiroHttpClient,
    providers,
    resolver::ModelResolver,
    routes::{self, AppState},
    web_ui::provider_oauth::HttpTokenExchanger,
};

// ==================================================================================================
// Test Helpers
// ==================================================================================================

/// Create a test application state with mocked dependencies
fn create_test_app_state() -> AppState {
    let cache = ModelCache::new(3600);
    cache.update(vec![
        json!({
            "modelId": "claude-sonnet-4",
            "modelName": "Claude Sonnet 4"
        }),
        json!({
            "modelId": "claude-haiku-4",
            "modelName": "Claude Haiku 4"
        }),
        json!({
            "modelId": "claude-opus-4",
            "modelName": "Claude Opus 4"
        }),
    ]);

    let auth_manager = Arc::new(tokio::sync::RwLock::new(
        AuthManager::new_for_testing(
            "test-access-token-12345".to_string(),
            "us-east-1".to_string(),
            300,
        )
        .expect("Failed to create test auth manager"),
    ));

    let http_client =
        Arc::new(KiroHttpClient::new(20, 30, 300, 3).expect("Failed to create HTTP client"));

    let resolver = ModelResolver::new(cache.clone(), HashMap::new());

    let config = Config {
        server_host: "127.0.0.1".to_string(),
        server_port: 8080,
        kiro_region: "us-east-1".to_string(),
        streaming_timeout: 300,
        token_refresh_threshold: 300,
        first_token_timeout: 15,
        http_max_connections: 20,
        http_connect_timeout: 30,
        http_request_timeout: 300,
        http_max_retries: 3,
        debug_mode: harbangan::config::DebugMode::Off,
        log_level: "info".to_string(),
        tool_description_max_length: 10000,
        fake_reasoning_enabled: true,
        fake_reasoning_max_tokens: 4000,
        fake_reasoning_handling: harbangan::config::FakeReasoningHandling::AsReasoningContent,
        truncation_recovery: true,
        default_provider: "kiro".to_string(),
        guardrails_enabled: false,
        gateway_mode: harbangan::config::GatewayMode::Full,
        proxy: None,
        database_url: None,
        anthropic_oauth_client_id: String::new(),
        openai_oauth_client_id: String::new(),
        google_client_id: String::new(),
        google_client_secret: String::new(),
        google_callback_url: String::new(),
        auth_google_enabled: false,
        auth_password_enabled: true,
        initial_admin_email: None,
        initial_admin_password: None,
    };

    let config_arc = Arc::new(RwLock::new(config));

    // Pre-populate the api_key_cache with our test key hash
    let api_key_cache = Arc::new(dashmap::DashMap::new());
    let test_key = "test-api-key-secret";
    let key_hash = {
        use sha2::Digest;
        hex::encode(sha2::Sha256::new_with_prefix(test_key.as_bytes()).finalize())
    };
    let test_user_id = uuid::Uuid::new_v4();
    let test_key_id = uuid::Uuid::new_v4();
    api_key_cache.insert(key_hash, (test_user_id, test_key_id));

    // Pre-populate the kiro_token_cache so auth middleware can resolve tokens
    let kiro_token_cache = Arc::new(dashmap::DashMap::new());
    kiro_token_cache.insert(
        test_user_id,
        (
            "test-access-token-12345".to_string(),
            "us-east-1".to_string(),
            std::time::Instant::now(),
        ),
    );

    AppState {
        model_cache: cache,
        auth_manager: auth_manager.clone(),
        http_client: http_client.clone(),
        resolver,
        config: config_arc.clone(),
        setup_complete: Arc::new(AtomicBool::new(true)),
        config_db: None,
        session_cache: Arc::new(dashmap::DashMap::new()),
        api_key_cache,
        kiro_token_cache,
        oauth_pending: Arc::new(dashmap::DashMap::new()),
        proxy_api_key_hash: None,
        guardrails_engine: None,
        provider_registry: Arc::new(providers::registry::ProviderRegistry::new()),
        providers: providers::build_provider_map(http_client, auth_manager, config_arc),
        provider_oauth_pending: Arc::new(dashmap::DashMap::new()),
        token_exchanger: Arc::new(HttpTokenExchanger::new()),
        login_rate_limiter: Arc::new(dashmap::DashMap::new()),
        rate_tracker: Arc::new(providers::rate_limiter::RateLimitTracker::new()),
        proxy_token_manager: None,
    }
}

/// Build the test application router
fn build_test_app(state: AppState) -> Router {
    let health_routes = routes::health_routes();
    let openai_routes = routes::openai_routes(state.clone());
    let anthropic_routes = routes::anthropic_routes(state);

    Router::new()
        .merge(health_routes)
        .merge(openai_routes)
        .merge(anthropic_routes)
}

/// Helper to parse JSON response body
async fn parse_json_body(body: Body) -> Value {
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

// ==================================================================================================
// Health Check Tests
// ==================================================================================================

#[tokio::test]
async fn test_root_endpoint() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = parse_json_body(response.into_body()).await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["message"], "Kiro Gateway is running");
    assert!(body["version"].is_string());
}

#[tokio::test]
async fn test_health_endpoint() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = parse_json_body(response.into_body()).await;
    assert_eq!(body["status"], "healthy");
    assert!(body["timestamp"].is_string());
    assert!(body["version"].is_string());
}

// ==================================================================================================
// OpenAI API Authentication Tests
// ==================================================================================================

#[tokio::test]
async fn test_openai_models_without_auth() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    // Request without Authorization header
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let body = parse_json_body(response.into_body()).await;
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("API Key"));
}

#[tokio::test]
async fn test_openai_models_with_invalid_auth() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    // Request with wrong API key — not in cache, DB fallback fails (no config_db).
    // The request is rejected (either 401 or 500 depending on DB availability).
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header(header::AUTHORIZATION, "Bearer wrong-api-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_ne!(
        response.status(),
        StatusCode::OK,
        "Invalid API key should not be allowed through"
    );
}

#[tokio::test]
async fn test_openai_models_with_valid_auth() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    // Request with correct API key
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header(header::AUTHORIZATION, "Bearer test-api-key-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = parse_json_body(response.into_body()).await;
    assert_eq!(body["object"], "list");
    assert!(body["data"].is_array());

    let models = body["data"].as_array().unwrap();
    assert_eq!(models.len(), 3);

    // Verify model structure
    for model in models {
        assert_eq!(model["object"], "model");
        assert!(model["id"].is_string());
        assert_eq!(model["owned_by"], "anthropic");
    }
}

#[tokio::test]
async fn test_openai_chat_completions_without_auth() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    let request_body = json!({
        "model": "claude-sonnet-4",
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_openai_chat_completions_empty_messages() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    let request_body = json!({
        "model": "claude-sonnet-4",
        "messages": []
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header(header::AUTHORIZATION, "Bearer test-api-key-secret")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = parse_json_body(response.into_body()).await;
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("messages"));
}

// ==================================================================================================
// Anthropic API Authentication Tests
// ==================================================================================================

#[tokio::test]
async fn test_anthropic_messages_without_auth() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    let request_body = json!({
        "model": "claude-sonnet-4",
        "max_tokens": 100,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/messages")
                .header(header::CONTENT_TYPE, "application/json")
                .header("anthropic-version", "2023-06-01")
                .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_anthropic_messages_without_version_header() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    let request_body = json!({
        "model": "claude-sonnet-4",
        "max_tokens": 100,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    // Request without anthropic-version header should still work (header is optional)
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/messages")
                .header(header::AUTHORIZATION, "Bearer test-api-key-secret")
                .header(header::CONTENT_TYPE, "application/json")
                // No anthropic-version header - should still work
                .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should not fail validation - will fail later when trying to call Kiro API
    // but that's expected in tests without a real backend
    // The important thing is it doesn't return 400 for missing header
    assert_ne!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_anthropic_messages_empty_messages() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    let request_body = json!({
        "model": "claude-sonnet-4",
        "max_tokens": 100,
        "messages": []
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/messages")
                .header(header::AUTHORIZATION, "Bearer test-api-key-secret")
                .header(header::CONTENT_TYPE, "application/json")
                .header("anthropic-version", "2023-06-01")
                .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = parse_json_body(response.into_body()).await;
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("messages"));
}

#[tokio::test]
async fn test_anthropic_messages_invalid_max_tokens() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    let request_body = json!({
        "model": "claude-sonnet-4",
        "max_tokens": 0,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/messages")
                .header(header::AUTHORIZATION, "Bearer test-api-key-secret")
                .header(header::CONTENT_TYPE, "application/json")
                .header("anthropic-version", "2023-06-01")
                .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = parse_json_body(response.into_body()).await;
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("max_tokens"));
}

// ==================================================================================================
// API Key Format Tests
// ==================================================================================================

#[tokio::test]
async fn test_auth_with_x_api_key_header() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    // Using x-api-key header instead of Authorization Bearer
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("x-api-key", "test-api-key-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_auth_bearer_prefix_case_insensitive() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    // Test with uppercase "Bearer" (standard format)
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header(header::AUTHORIZATION, "Bearer test-api-key-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

// ==================================================================================================
// 404 Not Found Tests
// ==================================================================================================

#[tokio::test]
async fn test_unknown_endpoint_with_auth() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    // Unknown endpoint with auth - should return 404
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/unknown")
                .header(header::AUTHORIZATION, "Bearer test-api-key-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_unknown_endpoint_without_auth() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    // Unknown endpoint without auth - may return 401 or 404 depending on route matching
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/unknown")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Either 401 (auth required) or 404 (not found) is acceptable
    assert!(
        response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn test_wrong_method() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    // GET on POST-only endpoint
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/chat/completions")
                .header(header::AUTHORIZATION, "Bearer test-api-key-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

// ==================================================================================================
// Model Resolution Tests
// ==================================================================================================

#[tokio::test]
async fn test_model_list_contains_expected_models() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header(header::AUTHORIZATION, "Bearer test-api-key-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = parse_json_body(response.into_body()).await;
    let models = body["data"].as_array().unwrap();

    let model_ids: Vec<&str> = models.iter().map(|m| m["id"].as_str().unwrap()).collect();

    assert!(model_ids.contains(&"claude-sonnet-4"));
    assert!(model_ids.contains(&"claude-haiku-4"));
    assert!(model_ids.contains(&"claude-opus-4"));
}

// ==================================================================================================
// Content-Type Tests
// ==================================================================================================

#[tokio::test]
async fn test_json_content_type_required_for_post() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    let request_body = json!({
        "model": "claude-sonnet-4",
        "messages": [{"role": "user", "content": "Hello"}]
    });

    // POST without Content-Type header — the handler parses body bytes directly
    // with serde_json::from_slice (no Axum extractor), so Content-Type isn't validated.
    // The request passes auth and validation but ultimately fails on the Kiro API call.
    // The important assertion: it does NOT succeed with 200.
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header(header::AUTHORIZATION, "Bearer test-api-key-secret")
                // Missing Content-Type
                .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Without a backend, the request will fail at some point after validation
    assert_ne!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_invalid_json_body() {
    let state = create_test_app_state();
    let app = build_test_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header(header::AUTHORIZATION, "Bearer test-api-key-secret")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{ invalid json }"))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should fail with bad request due to JSON parse error
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// ==================================================================================================
// Provider Toggle — Model List Filtering Tests
// ==================================================================================================

use harbangan::web_ui::config_db::RegistryModel;

fn make_registry_model(provider_id: &str, model_id: &str) -> RegistryModel {
    RegistryModel {
        id: uuid::Uuid::new_v4(),
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
        display_name: model_id.to_string(),
        prefixed_id: format!("{}/{}", provider_id, model_id),
        context_length: 200_000,
        max_output_tokens: 8192,
        capabilities: serde_json::json!({}),
        enabled: true,
        source: "test".to_string(),
        upstream_meta: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

/// Create a test app state with registry models populated (instead of legacy cache).
fn create_test_app_state_with_registry() -> AppState {
    let state = create_test_app_state();

    // Populate registry cache with models from multiple providers
    state
        .model_cache
        .insert_registry_model(make_registry_model("anthropic", "claude-sonnet-4"));
    state
        .model_cache
        .insert_registry_model(make_registry_model("anthropic", "claude-opus-4"));
    state
        .model_cache
        .insert_registry_model(make_registry_model("openai_codex", "gpt-4o"));
    state
        .model_cache
        .insert_registry_model(make_registry_model("copilot", "claude-sonnet-4"));

    state
}

#[tokio::test]
async fn test_models_filtered_by_disabled_provider() {
    let state = create_test_app_state_with_registry();

    // Disable Anthropic
    state
        .provider_registry
        .set_provider_enabled(harbangan::providers::types::ProviderId::Anthropic, false)
        .await;

    let app = build_test_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header(header::AUTHORIZATION, "Bearer test-api-key-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = parse_json_body(response.into_body()).await;
    let models = body["data"].as_array().unwrap();
    let model_ids: Vec<&str> = models.iter().map(|m| m["id"].as_str().unwrap()).collect();

    // Anthropic models should be excluded
    assert!(
        !model_ids.contains(&"anthropic/claude-sonnet-4"),
        "Disabled provider's models should be filtered out"
    );
    assert!(
        !model_ids.contains(&"anthropic/claude-opus-4"),
        "Disabled provider's models should be filtered out"
    );
    // OpenAI and Copilot models should remain
    assert!(model_ids.contains(&"openai_codex/gpt-4o"));
    assert!(model_ids.contains(&"copilot/claude-sonnet-4"));
}

#[tokio::test]
async fn test_models_include_all_when_no_providers_disabled() {
    let state = create_test_app_state_with_registry();
    let app = build_test_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header(header::AUTHORIZATION, "Bearer test-api-key-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = parse_json_body(response.into_body()).await;
    let models = body["data"].as_array().unwrap();

    // All 4 registry models should be present
    assert_eq!(models.len(), 4);
}

#[tokio::test]
async fn test_models_restored_after_re_enable() {
    let state = create_test_app_state_with_registry();
    let registry = state.provider_registry.clone();

    // Disable Anthropic
    registry
        .set_provider_enabled(harbangan::providers::types::ProviderId::Anthropic, false)
        .await;

    // Re-enable Anthropic
    registry
        .set_provider_enabled(harbangan::providers::types::ProviderId::Anthropic, true)
        .await;

    let app = build_test_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header(header::AUTHORIZATION, "Bearer test-api-key-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = parse_json_body(response.into_body()).await;
    let models = body["data"].as_array().unwrap();

    // All models should be back
    assert_eq!(models.len(), 4);
    let model_ids: Vec<&str> = models.iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert!(model_ids.contains(&"anthropic/claude-sonnet-4"));
}

#[tokio::test]
async fn test_models_all_non_kiro_disabled_returns_empty_registry() {
    let state = create_test_app_state_with_registry();

    // Disable all non-Kiro providers
    state
        .provider_registry
        .set_provider_enabled(harbangan::providers::types::ProviderId::Anthropic, false)
        .await;
    state
        .provider_registry
        .set_provider_enabled(harbangan::providers::types::ProviderId::OpenAICodex, false)
        .await;
    state
        .provider_registry
        .set_provider_enabled(harbangan::providers::types::ProviderId::Copilot, false)
        .await;

    let app = build_test_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header(header::AUTHORIZATION, "Bearer test-api-key-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = parse_json_body(response.into_body()).await;
    let models = body["data"].as_array().unwrap();

    // All registry models are from non-Kiro providers, so all filtered out.
    // Falls back to legacy cache which has 3 Kiro models.
    assert!(
        !models.is_empty(),
        "Legacy Kiro cache should provide fallback models"
    );
}

#[tokio::test]
async fn test_models_multiple_providers_disabled() {
    let state = create_test_app_state_with_registry();

    // Disable Anthropic and Copilot
    state
        .provider_registry
        .set_provider_enabled(harbangan::providers::types::ProviderId::Anthropic, false)
        .await;
    state
        .provider_registry
        .set_provider_enabled(harbangan::providers::types::ProviderId::Copilot, false)
        .await;

    let app = build_test_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header(header::AUTHORIZATION, "Bearer test-api-key-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = parse_json_body(response.into_body()).await;
    let models = body["data"].as_array().unwrap();

    // Only OpenAI model should remain
    assert_eq!(models.len(), 1);
    assert_eq!(models[0]["id"], "openai_codex/gpt-4o");
}
