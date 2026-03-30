mod anthropic;
mod openai;
pub mod pipeline;
pub mod state;

pub use state::{AppState, OAuthPendingState, SessionInfo, UserKiroCreds, PROXY_USER_ID};

use axum::{
    middleware as axum_middleware,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde_json::{json, Value};

use crate::middleware;

/// Application version from Cargo.toml
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Health check routes (no authentication required)
pub fn health_routes() -> Router {
    Router::new()
        .route("/", get(root_handler))
        .route("/health", get(health_handler))
}

/// OpenAI API routes (require authentication)
pub fn openai_routes(state: AppState) -> Router {
    Router::new()
        .route("/v1/models", get(openai::get_models_handler))
        .route(
            "/v1/chat/completions",
            post(openai::chat_completions_handler),
        )
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::auth_middleware,
        ))
        .with_state(state)
}

/// Anthropic API routes (require authentication)
pub fn anthropic_routes(state: AppState) -> Router {
    Router::new()
        .route("/v1/messages", post(anthropic::anthropic_messages_handler))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::auth_middleware,
        ))
        .with_state(state)
}

/// GET / - Simple health check
///
/// Returns basic status and version information.
/// This endpoint does not require authentication (for load balancers).
async fn root_handler() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "message": "Kiro Gateway is running",
        "version": VERSION
    }))
}

/// GET /health - Detailed health check
///
/// Returns detailed health information including timestamp.
/// This endpoint does not require authentication (for load balancers).
async fn health_handler() -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "timestamp": Utc::now().to_rfc3339(),
        "version": VERSION
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    use axum::body::Body;
    use axum::extract::State;
    use dashmap::DashMap;

    use crate::auth::AuthManager;
    use crate::cache::ModelCache;
    use crate::config::Config;
    use crate::error::ApiError;
    use crate::http_client::KiroHttpClient;
    use crate::providers::registry::ProviderRegistry;
    use crate::resolver::ModelResolver;

    use std::sync::RwLock;

    fn create_test_state() -> AppState {
        let cache = ModelCache::new(3600);
        cache.update(vec![
            serde_json::json!({
                "modelId": "claude-sonnet-4.5",
                "modelName": "Claude Sonnet 4.5"
            }),
            serde_json::json!({
                "modelId": "claude-haiku-4",
                "modelName": "Claude Haiku 4"
            }),
        ]);

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

        let config_arc = Arc::new(RwLock::new(config));

        AppState {
            proxy_api_key_hash: None,
            model_cache: cache,
            auth_manager: Arc::clone(&auth_manager),
            http_client: Arc::clone(&http_client),
            resolver,
            config: Arc::clone(&config_arc),
            setup_complete: Arc::new(AtomicBool::new(true)),
            config_db: None,
            session_cache: Arc::new(DashMap::new()),
            api_key_cache: Arc::new(DashMap::new()),
            kiro_token_cache: Arc::new(DashMap::new()),
            oauth_pending: Arc::new(DashMap::new()),
            guardrails_engine: None,
            provider_registry: Arc::new(ProviderRegistry::new()),
            providers: crate::providers::build_provider_map(http_client, auth_manager, config_arc),
            provider_oauth_pending: Arc::new(DashMap::new()),
            token_exchanger: Arc::new(crate::web_ui::provider_oauth::HttpTokenExchanger::new()),
            login_rate_limiter: Arc::new(DashMap::new()),
            rate_tracker: Arc::new(crate::providers::rate_limiter::RateLimitTracker::new()),
        }
    }

    #[tokio::test]
    async fn test_root_handler() {
        let json = root_handler().await;
        let value = json.0;

        assert_eq!(value["status"], "ok");
        assert_eq!(value["message"], "Kiro Gateway is running");
        assert_eq!(value["version"], VERSION);
    }

    #[tokio::test]
    async fn test_health_handler() {
        let json = health_handler().await;
        let value = json.0;

        assert_eq!(value["status"], "healthy");
        assert!(value["timestamp"].is_string());
        assert_eq!(value["version"], VERSION);
    }

    #[tokio::test]
    async fn test_get_models_handler() {
        let state = create_test_state();

        // Insert enabled registry models so get_enabled_registry_models() returns them
        use crate::web_ui::config_db::RegistryModel;
        use chrono::Utc;
        let now = Utc::now();
        for (prefixed, display, provider) in [
            ("kiro/claude-sonnet-4.5", "Claude Sonnet 4.5", "kiro"),
            ("kiro/claude-haiku-4", "Claude Haiku 4", "kiro"),
        ] {
            state.model_cache.insert_registry_model(RegistryModel {
                id: uuid::Uuid::new_v4(),
                provider_id: provider.to_string(),
                model_id: prefixed.to_string(),
                display_name: display.to_string(),
                prefixed_id: prefixed.to_string(),
                context_length: 200_000,
                max_output_tokens: 64_000,
                capabilities: serde_json::json!({}),
                enabled: true,
                source: "test".to_string(),
                upstream_meta: None,
                created_at: now,
                updated_at: now,
            });
        }

        // Call handler
        let result = openai::get_models_handler(State(state)).await;
        assert!(result.is_ok());

        let model_list = result.unwrap().0;
        assert_eq!(model_list.object, "list");
        assert_eq!(model_list.data.len(), 2);

        // Check model properties
        let model_ids: Vec<String> = model_list.data.iter().map(|m| m.id.clone()).collect();
        assert!(model_ids.contains(&"kiro/claude-sonnet-4.5".to_string()));
        assert!(model_ids.contains(&"kiro/claude-haiku-4".to_string()));

        // Check model fields
        for model in &model_list.data {
            assert_eq!(model.object, "model");
            assert_eq!(model.owned_by, "kiro");
            assert!(model.description.is_some());
        }
    }

    fn create_proxy_test_state() -> AppState {
        use crate::config::ProxyConfig;
        let proxy = ProxyConfig {
            api_key: "test-key-long-enough".to_string(),
            anthropic_api_key: Some("sk-ant-test".to_string()),
            openai_api_key: Some("sk-proj-test".to_string()),
            ..Default::default()
        };
        let registry = ProviderRegistry::from_proxy_config(&proxy);

        let mut state = create_test_state();
        state.provider_registry = Arc::new(registry);
        state
    }

    #[tokio::test]
    async fn test_get_models_proxy_no_duplicates() {
        let state = create_proxy_test_state();
        let result = openai::get_models_handler(State(state)).await.unwrap().0;
        let ids: Vec<&str> = result.data.iter().map(|m| m.id.as_str()).collect();

        // No duplicate model IDs
        let mut seen = std::collections::HashSet::new();
        for id in &ids {
            assert!(seen.insert(id), "duplicate model ID: {}", id);
        }
    }

    /// Helper: build an axum::http::Request from JSON body and optional headers.
    fn build_anthropic_request(
        body: &crate::models::anthropic::AnthropicMessagesRequest,
        extra_headers: Option<&[(&str, &str)]>,
    ) -> axum::http::Request<Body> {
        let body_json = serde_json::to_vec(body).unwrap();
        let mut builder = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("content-type", "application/json");
        if let Some(hdrs) = extra_headers {
            for (k, v) in hdrs {
                builder = builder.header(*k, *v);
            }
        }
        builder.body(Body::from(body_json)).unwrap()
    }

    /// Helper: build an axum::http::Request from an OpenAI JSON body.
    fn build_openai_request(
        body: &crate::models::openai::ChatCompletionRequest,
    ) -> axum::http::Request<Body> {
        let body_json = serde_json::to_vec(body).unwrap();
        axum::http::Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("content-type", "application/json")
            .body(Body::from(body_json))
            .unwrap()
    }

    #[tokio::test]
    async fn test_anthropic_messages_handler_without_version_header() {
        let state = create_test_state();

        // Create a request without anthropic-version header
        // This should NOT fail - the header is optional for compatibility
        let body = crate::models::anthropic::AnthropicMessagesRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![crate::models::anthropic::AnthropicMessage {
                role: "user".to_string(),
                content: serde_json::json!("Hello"),
            }],
            max_tokens: 100,
            system: None,
            stream: false,
            tools: None,
            tool_choice: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            metadata: None,
            thinking: None,
            disable_parallel_tool_use: None,
        };

        let raw_request = build_anthropic_request(&body, None);

        // Call handler - will fail later when trying to call Kiro API,
        // but should NOT fail due to missing anthropic-version header
        let result = anthropic::anthropic_messages_handler(State(state), raw_request).await;

        // The request should proceed past header validation
        // It will fail on the actual API call, but that's expected in tests
        match result {
            Err(ApiError::ValidationError(msg)) => {
                // Should NOT be about anthropic-version
                assert!(
                    !msg.contains("anthropic-version"),
                    "anthropic-version header should be optional, got error: {}",
                    msg
                );
            }
            _ => {
                // Any other error is fine - we just want to ensure it's not
                // failing due to missing anthropic-version header
            }
        }
    }

    #[tokio::test]
    async fn test_anthropic_messages_handler_empty_messages() {
        let state = create_test_state();

        // Create a request with empty messages
        let body = crate::models::anthropic::AnthropicMessagesRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![],
            max_tokens: 100,
            system: None,
            stream: false,
            tools: None,
            tool_choice: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            metadata: None,
            thinking: None,
            disable_parallel_tool_use: None,
        };

        let raw_request =
            build_anthropic_request(&body, Some(&[("anthropic-version", "2023-06-01")]));

        // Call handler - should fail due to empty messages
        let result = anthropic::anthropic_messages_handler(State(state), raw_request).await;

        assert!(result.is_err());
        match result {
            Err(ApiError::ValidationError(msg)) => {
                assert!(msg.contains("messages"));
            }
            _ => panic!("Expected ValidationError for empty messages"),
        }
    }

    #[tokio::test]
    async fn test_chat_completions_handler_rejects_removed_model_with_explicit_prefix() {
        let state = create_test_state();
        let body = crate::models::openai::ChatCompletionRequest {
            model: "anthropic/gemini-2.5-pro".to_string(),
            messages: vec![crate::models::openai::ChatMessage {
                role: "user".to_string(),
                content: Some(serde_json::json!("Hello")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            stream: false,
            temperature: None,
            top_p: None,
            n: None,
            max_tokens: None,
            max_completion_tokens: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
            tools: None,
            tool_choice: None,
            stream_options: None,
            logit_bias: None,
            logprobs: None,
            top_logprobs: None,
            user: None,
            seed: None,
            parallel_tool_calls: None,
            reasoning_effort: None,
            response_format: None,
        };

        let raw_request = build_openai_request(&body);
        let result = openai::chat_completions_handler(State(state), raw_request).await;

        match result {
            Err(ApiError::ValidationError(msg)) => assert!(msg.contains("removed")),
            _ => panic!("Expected ValidationError for explicitly prefixed removed model"),
        }
    }

    #[tokio::test]
    async fn test_anthropic_messages_handler_rejects_removed_model_with_explicit_prefix() {
        let state = create_test_state();
        let body = crate::models::anthropic::AnthropicMessagesRequest {
            model: "openai_codex/qwen-coder-plus".to_string(),
            messages: vec![crate::models::anthropic::AnthropicMessage {
                role: "user".to_string(),
                content: serde_json::json!("Hello"),
            }],
            max_tokens: 100,
            system: None,
            stream: false,
            tools: None,
            tool_choice: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            metadata: None,
            thinking: None,
            disable_parallel_tool_use: None,
        };

        let raw_request =
            build_anthropic_request(&body, Some(&[("anthropic-version", "2023-06-01")]));
        let result = anthropic::anthropic_messages_handler(State(state), raw_request).await;

        match result {
            Err(ApiError::ValidationError(msg)) => assert!(msg.contains("removed")),
            _ => panic!("Expected ValidationError for explicitly prefixed removed model"),
        }
    }
}
