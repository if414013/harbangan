use axum::{
    body::Body,
    extract::State,
    middleware::{self as axum_middleware},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use bytes::Bytes;
use chrono::Utc;
use futures::stream::StreamExt;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use crate::antigravity::account_manager::AccountManager;
use crate::antigravity::http_client::CloudCodeClient;
use crate::antigravity::router::{Backend, BackendRouter};
use crate::antigravity::session::SessionManager;
use crate::auth::AuthManager;
use crate::cache::ModelCache;
use crate::config::Config;
use crate::converters::anthropic_to_kiro::build_kiro_payload as build_kiro_payload_anthropic;
use crate::converters::openai_to_kiro::build_kiro_payload;
use crate::error::ApiError;
use crate::http_client::KiroHttpClient;
use crate::metrics::MetricsCollector;
use crate::middleware;
use crate::middleware::DEBUG_LOGGER;
use crate::models::anthropic::AnthropicMessagesRequest;
use crate::models::openai::{ChatCompletionRequest, ModelList, OpenAIModel};
use crate::resolver::ModelResolver;
use crate::tokenizer::{count_anthropic_message_tokens, count_message_tokens, count_tools_tokens};
use std::time::Instant;

/// Application version from Cargo.toml
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub proxy_api_key: String,
    pub model_cache: ModelCache,
    pub auth_manager: Arc<AuthManager>,
    pub http_client: Arc<KiroHttpClient>,
    pub resolver: ModelResolver,
    pub config: Arc<Config>,
    pub metrics: Arc<MetricsCollector>,
    // Antigravity backend (optional, enabled via config)
    pub backend_router: Option<BackendRouter>,
    pub antigravity_client: Option<Arc<CloudCodeClient>>,
    pub account_manager: Option<Arc<AccountManager>>,
    pub session_manager: Option<Arc<tokio::sync::Mutex<SessionManager>>>,
}

/// Guard to ensure active connections are decremented on drop
struct RequestGuard {
    metrics: Arc<MetricsCollector>,
    start_time: Instant,
    model: String,
    completed: bool,
}

impl RequestGuard {
    fn new(metrics: Arc<MetricsCollector>, model: String) -> Self {
        metrics.record_request_start();
        Self {
            metrics,
            start_time: Instant::now(),
            model,
            completed: false,
        }
    }

    fn complete(&mut self, input_tokens: u64, output_tokens: u64) {
        if !self.completed {
            let latency_ms = self.start_time.elapsed().as_secs_f64() * 1000.0;
            self.metrics
                .record_request_end(latency_ms, &self.model, input_tokens, output_tokens);
            self.completed = true;
        }
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        if !self.completed {
            // Request failed or was cancelled - just decrement active connections
            self.metrics
                .active_connections
                .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

fn error_type_from_api_error(err: &ApiError) -> &'static str {
    match err {
        ApiError::AuthError(_) => "auth",
        ApiError::ValidationError(_) => "validation",
        ApiError::KiroApiError { .. } => "upstream",
        ApiError::Internal(_) => "internal",
        ApiError::InvalidModel(_) => "validation",
        ApiError::ConfigError(_) => "config",
    }
}

/// Health check routes (no authentication required)
pub fn health_routes() -> Router {
    Router::new()
        .route("/", get(root_handler))
        .route("/health", get(health_handler))
}

/// OpenAI API routes (require authentication)
pub fn openai_routes(state: AppState) -> Router {
    Router::new()
        .route("/v1/models", get(get_models_handler))
        .route("/v1/chat/completions", post(chat_completions_handler))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::auth_middleware,
        ))
        .with_state(state)
}

/// Anthropic API routes (require authentication)
pub fn anthropic_routes(state: AppState) -> Router {
    Router::new()
        .route("/v1/messages", post(anthropic_messages_handler))
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

/// GET /v1/models - List available models
///
/// Returns a list of available models in OpenAI format.
/// Models are loaded from the cache (populated at startup).
async fn get_models_handler(State(state): State<AppState>) -> Result<Json<ModelList>, ApiError> {
    tracing::info!("Request to /v1/models");

    // Get all model IDs from cache
    let model_ids = state.model_cache.get_all_model_ids();

    // Build OpenAI-compatible model list
    let models: Vec<OpenAIModel> = model_ids
        .into_iter()
        .map(|id| {
            let mut model = OpenAIModel::new(id);
            model.description = Some("Claude model via Kiro API".to_string());
            model
        })
        .collect();

    Ok(Json(ModelList::new(models)))
}

/// POST /v1/chat/completions - Create chat completion
///
/// Handles both streaming and non-streaming chat completion requests.
/// Converts OpenAI format to Kiro format, makes the request, and converts back.
async fn chat_completions_handler(
    State(state): State<AppState>,
    Json(request): Json<ChatCompletionRequest>,
) -> Result<Response, ApiError> {
    tracing::info!(
        "Request to /v1/chat/completions: model={}, stream={}, messages={}",
        request.model,
        request.stream,
        request.messages.len()
    );

    // Validate request
    if request.messages.is_empty() {
        let err = ApiError::ValidationError("messages cannot be empty".to_string());
        state.metrics.record_error(error_type_from_api_error(&err));
        return Err(err);
    }

    // Resolve model name
    let resolution = state.resolver.resolve(&request.model);
    let model_id = resolution.internal_id.clone();

    let mut guard = RequestGuard::new(Arc::clone(&state.metrics), model_id.clone());

    tracing::debug!(
        "Model resolution: {} -> {} (source: {}, verified: {})",
        request.model,
        model_id,
        resolution.source,
        resolution.is_verified
    );

    // Route to antigravity backend if enabled and model matches
    if let Some(router) = &state.backend_router {
        if matches!(router.resolve(&request.model), Backend::Antigravity) {
            let client = state.antigravity_client.as_ref().ok_or_else(|| {
                ApiError::ConfigError("Antigravity client not initialized".to_string())
            })?;
            let acct_mgr = state.account_manager.as_ref().ok_or_else(|| {
                ApiError::ConfigError("Account manager not initialized".to_string())
            })?;
            let sess_mgr = state.session_manager.as_ref().ok_or_else(|| {
                ApiError::ConfigError("Session manager not initialized".to_string())
            })?;
            guard.complete(0, 0);
            return crate::antigravity::handlers::antigravity_chat_completions(
                acct_mgr, client, sess_mgr, &request,
            )
            .await;
        }
    }

    // Generate conversation ID
    let conversation_id = Uuid::new_v4().to_string();

    // Get profile ARN
    let profile_arn = state
        .auth_manager
        .get_profile_arn()
        .await
        .unwrap_or_default();

    // Convert OpenAI request to Kiro format
    let kiro_payload_result =
        build_kiro_payload(&request, &conversation_id, &profile_arn, &state.config).map_err(
            |e| {
                let err = ApiError::ValidationError(e);
                state.metrics.record_error(error_type_from_api_error(&err));
                err
            },
        )?;

    let kiro_payload = kiro_payload_result.payload;

    tracing::debug!(
        "Kiro payload: {}",
        serde_json::to_string_pretty(&kiro_payload).unwrap_or_default()
    );

    // Log Kiro request body for debugging
    if let Ok(kiro_body_json) = serde_json::to_vec_pretty(&kiro_payload) {
        DEBUG_LOGGER
            .log_kiro_request_body(Bytes::from(kiro_body_json))
            .await;
    }

    // Get access token
    let access_token = state.auth_manager.get_access_token().await.map_err(|e| {
        let err = ApiError::AuthError(format!("Failed to get access token: {}", e));
        state.metrics.record_error(error_type_from_api_error(&err));
        err
    })?;

    // Get region
    let region = state.auth_manager.get_region().await;

    // Build Kiro API URL - use /v1/chat/completions endpoint
    let kiro_api_url = format!(
        "https://codewhisperer.{}.amazonaws.com/generateAssistantResponse",
        region
    );

    // Build request
    let req = state
        .http_client
        .client()
        .post(&kiro_api_url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .json(&kiro_payload)
        .build()
        .map_err(|e| {
            let err = ApiError::Internal(anyhow::anyhow!("Failed to build request: {}", e));
            state.metrics.record_error(error_type_from_api_error(&err));
            err
        })?;

    let response = state
        .http_client
        .request_with_retry(req)
        .await
        .inspect_err(|e| {
            state.metrics.record_error(error_type_from_api_error(e));
        })?;

    let input_tokens = count_message_tokens(&request.messages, false)
        + count_tools_tokens(request.tools.as_ref(), false);

    // Extract include_usage from stream_options
    // Default to true for better compatibility with OpenCode and other clients
    // that expect providers to report token usage
    let include_usage = request
        .stream_options
        .as_ref()
        .and_then(|opts| opts.get("include_usage"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true); // Changed from false to true

    tracing::info!(
        "Stream options: {:?}, include_usage: {}, streaming: {}",
        request.stream_options,
        include_usage,
        request.stream
    );

    // Handle streaming vs non-streaming
    if request.stream {
        // Streaming response
        tracing::debug!("Handling streaming response");

        use crate::metrics::collector::StreamingMetricsTracker;

        let streaming_tracker = StreamingMetricsTracker::new(
            Arc::clone(&state.metrics),
            model_id.clone(),
            input_tokens as u64,
        );
        let output_tokens_handle = streaming_tracker.output_tokens_handle();

        // Use proper streaming conversion from streaming module
        let openai_stream = crate::streaming::stream_kiro_to_openai(
            response,
            &request.model,
            15,
            input_tokens,
            Some(output_tokens_handle),
            include_usage,
        )
        .await
        .inspect_err(|e| {
            state.metrics.record_error(error_type_from_api_error(e));
        })?;

        // Convert Result<String, ApiError> stream to bytes stream for SSE
        use bytes::Bytes;
        let byte_stream = openai_stream.map(move |result| {
            let _tracker = &streaming_tracker;
            result
                .map(Bytes::from)
                .map_err(|e| std::io::Error::other(e.to_string()))
        });

        // Return as SSE response with proper headers
        let response = Response::builder()
            .status(200)
            .header("Content-Type", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .body(Body::from_stream(byte_stream))
            .map_err(|e| {
                let err = ApiError::Internal(anyhow::anyhow!("Failed to build response: {}", e));
                state.metrics.record_error(error_type_from_api_error(&err));
                err
            })?;

        std::mem::drop(guard);

        DEBUG_LOGGER.discard_buffers().await;

        Ok(response)
    } else {
        // Non-streaming response
        // Kiro API always returns AWS Event Stream format, even for non-streaming requests.
        // We use collect_openai_response to parse the stream and aggregate into a single response.
        tracing::debug!("Handling non-streaming response (collecting stream)");

        let first_token_timeout = state.config.first_token_timeout;
        let openai_response = crate::streaming::collect_openai_response(
            response,
            &request.model,
            first_token_timeout,
            input_tokens,
        )
        .await
        .inspect_err(|e| {
            state.metrics.record_error(error_type_from_api_error(e));
        })?;

        let output_tokens = openai_response
            .get("usage")
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0);

        guard.complete(input_tokens as u64, output_tokens);

        DEBUG_LOGGER.discard_buffers().await;

        Ok(Json(openai_response).into_response())
    }
}

/// POST /v1/messages - Create Anthropic message
///
/// Handles both streaming and non-streaming message requests in Anthropic format.
/// Converts Anthropic format to Kiro format, makes the request, and converts back.
async fn anthropic_messages_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<AnthropicMessagesRequest>,
) -> Result<Response, ApiError> {
    tracing::info!(
        "Request to /v1/messages: model={}, stream={}, messages={}",
        request.model,
        request.stream,
        request.messages.len()
    );

    // Check anthropic-version header (optional, for compatibility logging)
    let anthropic_version = headers
        .get("anthropic-version")
        .and_then(|v| v.to_str().ok());

    if let Some(version) = anthropic_version {
        tracing::debug!("anthropic-version: {}", version);
    }

    // Validate request
    if request.messages.is_empty() {
        let err = ApiError::ValidationError("messages cannot be empty".to_string());
        state.metrics.record_error(error_type_from_api_error(&err));
        return Err(err);
    }

    if request.max_tokens <= 0 {
        let err = ApiError::ValidationError("max_tokens must be positive".to_string());
        state.metrics.record_error(error_type_from_api_error(&err));
        return Err(err);
    }

    // Resolve model name
    let resolution = state.resolver.resolve(&request.model);
    let model_id = resolution.internal_id.clone();

    let mut guard = RequestGuard::new(Arc::clone(&state.metrics), model_id.clone());

    tracing::debug!(
        "Model resolution: {} -> {} (source: {}, verified: {})",
        request.model,
        model_id,
        resolution.source,
        resolution.is_verified
    );

    // Route to antigravity backend if enabled and model matches
    if let Some(router) = &state.backend_router {
        if matches!(router.resolve(&request.model), Backend::Antigravity) {
            let client = state.antigravity_client.as_ref().ok_or_else(|| {
                ApiError::ConfigError("Antigravity client not initialized".to_string())
            })?;
            let acct_mgr = state.account_manager.as_ref().ok_or_else(|| {
                ApiError::ConfigError("Account manager not initialized".to_string())
            })?;
            let sess_mgr = state.session_manager.as_ref().ok_or_else(|| {
                ApiError::ConfigError("Session manager not initialized".to_string())
            })?;
            guard.complete(0, 0);
            return crate::antigravity::handlers::antigravity_anthropic_messages(
                acct_mgr, client, sess_mgr, &request,
            )
            .await;
        }
    }

    // Generate conversation ID
    let conversation_id = Uuid::new_v4().to_string();

    // Get profile ARN
    let profile_arn = state
        .auth_manager
        .get_profile_arn()
        .await
        .unwrap_or_default();

    // Convert Anthropic request to Kiro format
    let kiro_payload_result =
        build_kiro_payload_anthropic(&request, &conversation_id, &profile_arn, &state.config)
            .map_err(|e| {
                let err = ApiError::ValidationError(e);
                state.metrics.record_error(error_type_from_api_error(&err));
                err
            })?;

    let kiro_payload = kiro_payload_result.payload;

    tracing::debug!(
        "Kiro payload: {}",
        serde_json::to_string_pretty(&kiro_payload).unwrap_or_default()
    );

    // Log Kiro request body for debugging
    if let Ok(kiro_body_json) = serde_json::to_vec_pretty(&kiro_payload) {
        DEBUG_LOGGER
            .log_kiro_request_body(Bytes::from(kiro_body_json))
            .await;
    }

    // Get access token
    let access_token = state.auth_manager.get_access_token().await.map_err(|e| {
        let err = ApiError::AuthError(format!("Failed to get access token: {}", e));
        state.metrics.record_error(error_type_from_api_error(&err));
        err
    })?;

    // Get region
    let region = state.auth_manager.get_region().await;

    // Build Kiro API URL - use /v1/messages endpoint
    let kiro_api_url = format!(
        "https://codewhisperer.{}.amazonaws.com/generateAssistantResponse",
        region
    );

    // Build request
    let req = state
        .http_client
        .client()
        .post(&kiro_api_url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .json(&kiro_payload)
        .build()
        .map_err(|e| {
            let err = ApiError::Internal(anyhow::anyhow!("Failed to build request: {}", e));
            state.metrics.record_error(error_type_from_api_error(&err));
            err
        })?;

    let response = state
        .http_client
        .request_with_retry(req)
        .await
        .inspect_err(|e| {
            state.metrics.record_error(error_type_from_api_error(e));
        })?;

    let input_tokens = count_anthropic_message_tokens(
        &request.messages,
        request.system.as_ref(),
        request.tools.as_ref(),
    );

    // Handle streaming vs non-streaming
    if request.stream {
        // Streaming response
        tracing::debug!("Handling streaming response");

        use crate::metrics::collector::StreamingMetricsTracker;

        let streaming_tracker = StreamingMetricsTracker::new(
            Arc::clone(&state.metrics),
            model_id.clone(),
            input_tokens as u64,
        );
        let output_tokens_handle = streaming_tracker.output_tokens_handle();

        // Convert response to Anthropic SSE stream
        let first_token_timeout = state.config.first_token_timeout;
        let anthropic_stream = crate::streaming::stream_kiro_to_anthropic(
            response,
            &request.model,
            first_token_timeout,
            input_tokens,
            Some(output_tokens_handle),
        )
        .await
        .inspect_err(|e| {
            state.metrics.record_error(error_type_from_api_error(e));
        })?;

        // Convert to raw SSE response (stream already contains properly formatted SSE events)
        // Don't use Axum's Sse wrapper as it would double-wrap the events
        let byte_stream = anthropic_stream.map(move |result| {
            let _tracker = &streaming_tracker;
            result
                .map(Bytes::from)
                .map_err(|e| std::io::Error::other(e.to_string()))
        });

        let response = Response::builder()
            .status(200)
            .header("Content-Type", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .body(Body::from_stream(byte_stream))
            .map_err(|e| {
                let err = ApiError::Internal(anyhow::anyhow!("Failed to build response: {}", e));
                state.metrics.record_error(error_type_from_api_error(&err));
                err
            })?;

        std::mem::drop(guard);

        DEBUG_LOGGER.discard_buffers().await;

        Ok(response)
    } else {
        // Non-streaming response
        // Kiro API always returns AWS Event Stream format, even for non-streaming requests.
        // We use collect_anthropic_response to parse the stream and aggregate into a single response.
        tracing::debug!("Handling non-streaming response (collecting stream)");

        let first_token_timeout = state.config.first_token_timeout;
        let anthropic_response = crate::streaming::collect_anthropic_response(
            response,
            &request.model,
            first_token_timeout,
            input_tokens,
        )
        .await
        .inspect_err(|e| {
            state.metrics.record_error(error_type_from_api_error(e));
        })?;

        let output_tokens = anthropic_response
            .get("usage")
            .and_then(|u| u.get("output_tokens"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0);

        guard.complete(input_tokens as u64, output_tokens);

        DEBUG_LOGGER.discard_buffers().await;

        Ok(Json(anthropic_response).into_response())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

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
            proxy_api_key: "test-key".to_string(),
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
            antigravity: crate::config::AntigravityConfig {
                enabled: false,
                refresh_token: None,
                project_id: None,
            },
        });

        let metrics = Arc::new(crate::metrics::MetricsCollector::new());

        AppState {
            proxy_api_key: "test-key".to_string(),
            model_cache: cache,
            auth_manager,
            http_client,
            resolver,
            config,
            metrics,
            backend_router: None,
            antigravity_client: None,
            account_manager: None,
            session_manager: None,
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

        // Call handler
        let result = get_models_handler(State(state)).await;
        assert!(result.is_ok());

        let model_list = result.unwrap().0;
        assert_eq!(model_list.object, "list");
        assert_eq!(model_list.data.len(), 2);

        // Check model properties
        let model_ids: Vec<String> = model_list.data.iter().map(|m| m.id.clone()).collect();
        assert!(model_ids.contains(&"claude-sonnet-4.5".to_string()));
        assert!(model_ids.contains(&"claude-haiku-4".to_string()));

        // Check model fields
        for model in &model_list.data {
            assert_eq!(model.object, "model");
            assert_eq!(model.owned_by, "anthropic");
            assert!(model.description.is_some());
        }
    }

    #[tokio::test]
    async fn test_anthropic_messages_handler_without_version_header() {
        let state = create_test_state();

        // Create a request without anthropic-version header
        // This should NOT fail - the header is optional for compatibility
        let request = crate::models::anthropic::AnthropicMessagesRequest {
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
        };

        let headers = axum::http::HeaderMap::new();

        // Call handler - will fail later when trying to call Kiro API,
        // but should NOT fail due to missing anthropic-version header
        let result = anthropic_messages_handler(State(state), headers, Json(request)).await;

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
        let request = crate::models::anthropic::AnthropicMessagesRequest {
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
        };

        let mut headers = axum::http::HeaderMap::new();
        headers.insert("anthropic-version", "2023-06-01".parse().unwrap());

        // Call handler - should fail due to empty messages
        let result = anthropic_messages_handler(State(state), headers, Json(request)).await;

        assert!(result.is_err());
        match result {
            Err(ApiError::ValidationError(msg)) => {
                assert!(msg.contains("messages"));
            }
            _ => panic!("Expected ValidationError for empty messages"),
        }
    }
}
