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
use dashmap::DashMap;
use futures::stream::StreamExt;
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use uuid::Uuid;

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::RwLock;
use std::time::Instant;

use crate::auth::AuthManager;
use crate::cache::ModelCache;
use crate::config::Config;
use crate::converters::anthropic_to_kiro::build_kiro_payload as build_kiro_payload_anthropic;
use crate::converters::openai_to_kiro::build_kiro_payload;
use crate::error::ApiError;
use crate::http_client::KiroHttpClient;
use crate::log_capture::LogEntry;
use crate::metrics::MetricsCollector;
use crate::middleware;
use crate::middleware::DEBUG_LOGGER;
use crate::models::anthropic::AnthropicMessagesRequest;
use crate::models::openai::{ChatCompletionRequest, ModelList, OpenAIModel};
use crate::resolver::ModelResolver;
use crate::tokenizer::{count_anthropic_message_tokens, count_message_tokens, count_tools_tokens};
use crate::web_ui::config_db::ConfigDb;

/// Application version from Cargo.toml
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Per-user Kiro credentials, injected into request extensions by auth middleware.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct UserKiroCreds {
    pub user_id: Uuid,
    pub access_token: String,
    pub refresh_token: String,
    pub region: String,
}

/// Cached session information (in-memory, backed by DB).
// TODO: Replace `role: String` with a `Role` enum (Admin, User) with serde support.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SessionInfo {
    pub user_id: Uuid,
    pub email: String,
    pub role: String,
    pub expires_at: chrono::DateTime<Utc>,
}

/// Pending OAuth state for PKCE validation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct OAuthPendingState {
    pub nonce: String,
    pub pkce_verifier: String,
    pub created_at: chrono::DateTime<Utc>,
}

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub model_cache: ModelCache,
    pub auth_manager: Arc<tokio::sync::RwLock<AuthManager>>,
    pub http_client: Arc<KiroHttpClient>,
    pub resolver: ModelResolver,
    pub config: Arc<RwLock<Config>>,
    #[allow(dead_code)]
    pub setup_complete: Arc<AtomicBool>,
    pub metrics: Arc<MetricsCollector>,
    pub log_buffer: Arc<Mutex<VecDeque<LogEntry>>>,
    pub config_db: Option<Arc<ConfigDb>>,
    /// In-memory session cache: session_id → SessionInfo
    #[allow(dead_code)]
    pub session_cache: Arc<DashMap<Uuid, SessionInfo>>,
    /// In-memory API key cache: key_hash → (user_id, key_id)
    #[allow(dead_code)]
    pub api_key_cache: Arc<DashMap<String, (Uuid, Uuid)>>,
    /// In-memory Kiro token cache: user_id → (access_token, region, cached_at)
    pub kiro_token_cache: Arc<DashMap<Uuid, (String, String, std::time::Instant)>>,
    /// Pending OAuth states: state_param → OAuthPendingState (10-min TTL)
    #[allow(dead_code)]
    pub oauth_pending: Arc<DashMap<String, OAuthPendingState>>,
}

impl AppState {
    /// Get the config database or return an error.
    pub fn require_config_db(&self) -> Result<Arc<ConfigDb>, ApiError> {
        self.config_db
            .as_ref()
            .cloned()
            .ok_or_else(|| ApiError::ConfigError("Config database not available".to_string()))
    }

    /// Evict all cached data for a user (sessions, API keys, Kiro tokens).
    /// Call after role change or user deletion.
    #[allow(dead_code)]
    pub fn evict_user_caches(&self, user_id: Uuid) {
        self.session_cache.retain(|_, info| info.user_id != user_id);
        self.api_key_cache.retain(|_, (uid, _)| *uid != user_id);
        self.kiro_token_cache.remove(&user_id);
    }
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
        ApiError::Forbidden(_) => "auth",
        ApiError::SessionExpired => "auth",
        ApiError::DomainNotAllowed(_) => "auth",
        ApiError::KiroTokenRequired => "auth",
        ApiError::KiroTokenExpired => "auth",
        ApiError::LastAdmin => "validation",
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
    raw_request: axum::http::Request<Body>,
) -> Result<Response, ApiError> {
    // Extract per-user Kiro credentials injected by auth middleware
    let user_creds = raw_request.extensions().get::<UserKiroCreds>().cloned();

    // Parse JSON body
    let body_bytes = axum::body::to_bytes(raw_request.into_body(), 10 * 1024 * 1024)
        .await
        .map_err(|e| ApiError::ValidationError(format!("Failed to read body: {}", e)))?;
    let request: ChatCompletionRequest = serde_json::from_slice(&body_bytes)
        .map_err(|e| ApiError::ValidationError(format!("Invalid JSON: {}", e)))?;

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

    // Generate conversation ID
    let conversation_id = Uuid::new_v4().to_string();

    // Get profile ARN
    let auth = state.auth_manager.read().await;
    let profile_arn = auth.get_profile_arn().await.unwrap_or_default();
    drop(auth);

    // Read config snapshot for this request
    let config = state
        .config
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();

    // Inject truncation recovery messages if enabled
    let mut request = request;
    if config.truncation_recovery {
        // Convert messages to Value for injection
        let mut msg_values: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter_map(|m| serde_json::to_value(m).ok())
            .collect();
        crate::truncation::inject_openai_truncation_recovery(&mut msg_values);
        // Convert back
        request.messages = msg_values
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();
    }

    // Convert OpenAI request to Kiro format
    let kiro_payload_result = build_kiro_payload(&request, &conversation_id, &profile_arn, &config)
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

    // Get access token and region from per-user creds (injected by middleware) or fallback to global auth
    let (access_token, region) = if let Some(ref creds) = user_creds {
        (creds.access_token.clone(), creds.region.clone())
    } else {
        let auth = state.auth_manager.read().await;
        let token = auth.get_access_token().await.map_err(|e| {
            let err = ApiError::AuthError(format!("Failed to get access token: {}", e));
            state.metrics.record_error(error_type_from_api_error(&err));
            err
        })?;
        let r = auth.get_region().await;
        (token, r)
    };

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
            config.truncation_recovery,
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

        let first_token_timeout = config.first_token_timeout;
        let openai_response = crate::streaming::collect_openai_response(
            response,
            &request.model,
            first_token_timeout,
            input_tokens,
            config.truncation_recovery,
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
    raw_request: axum::http::Request<Body>,
) -> Result<Response, ApiError> {
    // Extract per-user Kiro credentials injected by auth middleware
    let user_creds = raw_request.extensions().get::<UserKiroCreds>().cloned();

    // Extract headers before consuming the request body
    let headers = raw_request.headers().clone();

    // Parse JSON body
    let body_bytes = axum::body::to_bytes(raw_request.into_body(), 10 * 1024 * 1024)
        .await
        .map_err(|e| ApiError::ValidationError(format!("Failed to read body: {}", e)))?;
    let request: AnthropicMessagesRequest = serde_json::from_slice(&body_bytes)
        .map_err(|e| ApiError::ValidationError(format!("Invalid JSON: {}", e)))?;

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

    // Generate conversation ID
    let conversation_id = Uuid::new_v4().to_string();

    // Get profile ARN
    let auth = state.auth_manager.read().await;
    let profile_arn = auth.get_profile_arn().await.unwrap_or_default();
    drop(auth);

    // Read config snapshot for this request
    let config = state
        .config
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();

    // Inject truncation recovery messages if enabled
    let mut request = request;
    if config.truncation_recovery {
        // Convert messages to Value for injection
        let mut msg_values: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content
                })
            })
            .collect();
        crate::truncation::inject_anthropic_truncation_recovery(&mut msg_values);
        // Convert back
        request.messages = msg_values
            .into_iter()
            .map(|v| crate::models::anthropic::AnthropicMessage {
                role: v["role"].as_str().unwrap_or("user").to_string(),
                content: v["content"].clone(),
            })
            .collect();
    }

    // Convert Anthropic request to Kiro format
    let kiro_payload_result =
        build_kiro_payload_anthropic(&request, &conversation_id, &profile_arn, &config).map_err(
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

    // Get access token and region from per-user creds (injected by middleware) or fallback to global auth
    let (access_token, region) = if let Some(ref creds) = user_creds {
        (creds.access_token.clone(), creds.region.clone())
    } else {
        let auth = state.auth_manager.read().await;
        let token = auth.get_access_token().await.map_err(|e| {
            let err = ApiError::AuthError(format!("Failed to get access token: {}", e));
            state.metrics.record_error(error_type_from_api_error(&err));
            err
        })?;
        let r = auth.get_region().await;
        (token, r)
    };

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
        let first_token_timeout = config.first_token_timeout;
        let anthropic_stream = crate::streaming::stream_kiro_to_anthropic(
            response,
            &request.model,
            first_token_timeout,
            input_tokens,
            Some(output_tokens_handle),
            config.truncation_recovery,
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

        let first_token_timeout = config.first_token_timeout;
        let anthropic_response = crate::streaming::collect_anthropic_response(
            response,
            &request.model,
            first_token_timeout,
            input_tokens,
            config.truncation_recovery,
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
            config: Arc::new(RwLock::new(config)),
            setup_complete: Arc::new(AtomicBool::new(true)),
            metrics,
            log_buffer: Arc::new(Mutex::new(VecDeque::new())),
            config_db: None,
            session_cache: Arc::new(DashMap::new()),
            api_key_cache: Arc::new(DashMap::new()),
            kiro_token_cache: Arc::new(DashMap::new()),
            oauth_pending: Arc::new(DashMap::new()),
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
        };

        let raw_request = build_anthropic_request(&body, None);

        // Call handler - will fail later when trying to call Kiro API,
        // but should NOT fail due to missing anthropic-version header
        let result = anthropic_messages_handler(State(state), raw_request).await;

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
        };

        let raw_request =
            build_anthropic_request(&body, Some(&[("anthropic-version", "2023-06-01")]));

        // Call handler - should fail due to empty messages
        let result = anthropic_messages_handler(State(state), raw_request).await;

        assert!(result.is_err());
        match result {
            Err(ApiError::ValidationError(msg)) => {
                assert!(msg.contains("messages"));
            }
            _ => panic!("Expected ValidationError for empty messages"),
        }
    }
}
