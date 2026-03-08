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

use std::sync::RwLock;

use crate::auth::AuthManager;
use crate::cache::ModelCache;
use crate::config::Config;
use crate::converters::anthropic_to_kiro::build_kiro_payload as build_kiro_payload_anthropic;
use crate::converters::openai_to_kiro::build_kiro_payload;
use crate::error::ApiError;
use crate::http_client::KiroHttpClient;
use crate::middleware;
use crate::middleware::DEBUG_LOGGER;
use crate::models::anthropic::AnthropicMessagesRequest;
use crate::models::openai::{ChatCompletionRequest, ModelList, OpenAIModel};
use crate::providers::anthropic::AnthropicProvider;
use crate::providers::copilot::CopilotProvider;
use crate::providers::gemini::GeminiProvider;
use crate::providers::openai::OpenAIProvider;
use crate::providers::qwen::QwenProvider;
use crate::providers::registry::ProviderRegistry;
use crate::providers::types::{ProviderContext, ProviderCredentials, ProviderId};
use crate::resolver::ModelResolver;
use crate::tokenizer::{count_anthropic_message_tokens, count_message_tokens, count_tools_tokens};
use crate::web_ui::config_db::ConfigDb;
use crate::web_ui::provider_oauth::{ProviderOAuthPendingState, TokenExchanger};

/// Application version from Cargo.toml
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Per-user Kiro credentials, injected into request extensions by auth middleware.
#[derive(Debug, Clone)]
pub struct UserKiroCreds {
    pub user_id: Uuid,
    pub access_token: String,
    pub refresh_token: String,
    pub region: String,
}

/// Cached session information (in-memory, backed by DB).
// TODO: Replace `role: String` with a `Role` enum (Admin, User) with serde support.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub user_id: Uuid,
    pub email: String,
    pub role: String,
    pub expires_at: chrono::DateTime<Utc>,
}

/// Pending OAuth state for PKCE validation.
#[derive(Debug, Clone)]
pub struct OAuthPendingState {
    pub nonce: String,
    pub pkce_verifier: String,
    pub created_at: chrono::DateTime<Utc>,
}

/// Application state shared across handlers.
///
/// Future refactoring: consider grouping related fields into sub-structs
/// (e.g., AuthState, CacheState, FeatureState) to keep AppState focused.
#[derive(Clone)]
pub struct AppState {
    // Core services
    pub model_cache: ModelCache,
    pub auth_manager: Arc<tokio::sync::RwLock<AuthManager>>,
    pub http_client: Arc<KiroHttpClient>,
    pub resolver: ModelResolver,
    pub config: Arc<RwLock<Config>>,
    pub setup_complete: Arc<AtomicBool>,
    pub config_db: Option<Arc<ConfigDb>>,
    // In-memory caches
    /// session_id → SessionInfo
    pub session_cache: Arc<DashMap<Uuid, SessionInfo>>,
    /// key_hash → (user_id, key_id)
    pub api_key_cache: Arc<DashMap<String, (Uuid, Uuid)>>,
    /// user_id → (access_token, region, cached_at)
    pub kiro_token_cache: Arc<DashMap<Uuid, (String, String, std::time::Instant)>>,
    /// state_param → OAuthPendingState (10-min TTL)
    pub oauth_pending: Arc<DashMap<String, OAuthPendingState>>,
    // Feature subsystems
    /// Guardrails engine for input/output validation (None when guardrails disabled or no DB)
    pub guardrails_engine: Option<Arc<crate::guardrails::engine::GuardrailsEngine>>,
    /// MCP Gateway manager (None when mcp_enabled=false or feature not yet initialized)
    pub mcp_manager: Option<Arc<crate::mcp::McpManager>>,
    // Multi-provider support
    /// Routes requests to the right provider based on user API keys
    pub provider_registry: Arc<ProviderRegistry>,
    /// Direct Anthropic API provider
    pub anthropic_provider: Arc<AnthropicProvider>,
    /// Direct OpenAI API provider
    pub openai_provider: Arc<OpenAIProvider>,
    /// Direct Gemini API provider
    pub gemini_provider: Arc<GeminiProvider>,
    /// Direct Copilot API provider
    pub copilot_provider: Arc<CopilotProvider>,
    /// Direct Qwen API provider
    pub qwen_provider: Arc<QwenProvider>,
    // Provider OAuth relay
    /// Pending provider OAuth relay states (separate from Google SSO oauth_pending)
    pub provider_oauth_pending: Arc<DashMap<String, ProviderOAuthPendingState>>,
    /// Token exchanger for provider OAuth (mockable for tests)
    pub token_exchanger: Arc<dyn TokenExchanger>,
    // Copilot
    /// user_id → (copilot_token, base_url, cached_at)
    pub copilot_token_cache: Arc<DashMap<Uuid, (String, String, std::time::Instant)>>,
    /// Pending Copilot device flow states: device_code → CopilotDevicePending
    pub copilot_device_pending: crate::web_ui::copilot_auth::CopilotDevicePendingMap,
    // Qwen device flow
    /// Pending Qwen device flow states: device_code → QwenDevicePending
    pub qwen_device_pending: crate::web_ui::qwen_auth::QwenDevicePendingMap,
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

/// Extract the last user message content from OpenAI-format messages.
fn extract_last_user_message(messages: &[crate::models::openai::ChatMessage]) -> String {
    for msg in messages.iter().rev() {
        if msg.role == "user" {
            if let Some(ref content) = msg.content {
                if let Some(s) = content.as_str() {
                    return s.to_string();
                }
                return content.to_string();
            }
        }
    }
    String::new()
}

/// Extract the last user message content from Anthropic-format messages.
fn extract_last_user_message_anthropic(
    messages: &[crate::models::anthropic::AnthropicMessage],
) -> String {
    for msg in messages.iter().rev() {
        if msg.role == "user" {
            if let Some(s) = msg.content.as_str() {
                return s.to_string();
            }
            return msg.content.to_string();
        }
    }
    String::new()
}

/// Extract the assistant content from an OpenAI non-streaming response.
fn extract_assistant_content(response: &Value) -> String {
    response
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string()
}

/// Extract the assistant content from an Anthropic non-streaming response.
fn extract_assistant_content_anthropic(response: &Value) -> String {
    response
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|block| block.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string()
}

/// Build a RequestContext for guardrails CEL evaluation.
fn build_request_context_openai(
    request: &ChatCompletionRequest,
) -> crate::guardrails::RequestContext {
    let content_length: usize = request
        .messages
        .iter()
        .map(|m| m.content.as_ref().map_or(0, |c| c.to_string().len()))
        .sum();

    crate::guardrails::RequestContext {
        model: request.model.clone(),
        api_format: "openai".to_string(),
        message_count: request.messages.len(),
        has_tools: request.tools.is_some(),
        is_streaming: request.stream,
        content_length,
    }
}

/// Build a RequestContext for guardrails CEL evaluation (Anthropic format).
fn build_request_context_anthropic(
    request: &AnthropicMessagesRequest,
) -> crate::guardrails::RequestContext {
    let content_length: usize = request
        .messages
        .iter()
        .map(|m| m.content.to_string().len())
        .sum();

    crate::guardrails::RequestContext {
        model: request.model.clone(),
        api_format: "anthropic".to_string(),
        message_count: request.messages.len(),
        has_tools: request.tools.is_some(),
        is_streaming: request.stream,
        content_length,
    }
}

// ── Shared pipeline helpers ─────────────────────────────────────────
// Used by both OpenAI and Anthropic handlers to avoid code duplication.

/// Run input guardrails validation. Returns Err(GuardrailBlocked) if the content is blocked.
/// On engine errors, logs a warning and allows the request through (fail-open).
async fn run_input_guardrail_check(
    engine: &crate::guardrails::engine::GuardrailsEngine,
    content: &str,
    ctx: &crate::guardrails::RequestContext,
) -> Result<(), ApiError> {
    if content.is_empty() {
        return Ok(());
    }
    match engine.validate_input(content, ctx).await {
        Ok(Some(result)) if result.action == crate::guardrails::GuardrailAction::Intervened => {
            Err(ApiError::GuardrailBlocked {
                violations: result.results,
                processing_time_ms: result.total_processing_time_ms,
            })
        }
        Ok(_) => Ok(()),
        Err(e) => {
            tracing::warn!(
                error = %e,
                api_format = %ctx.api_format,
                model = %ctx.model,
                "Input guardrail check failed — failing open, request allowed through"
            );
            Ok(())
        }
    }
}

/// Run output guardrails validation. Returns Err if content is blocked or redacted.
/// On engine errors, logs a warning and allows the response through (fail-open).
async fn run_output_guardrail_check(
    engine: &crate::guardrails::engine::GuardrailsEngine,
    content: &str,
    ctx: &crate::guardrails::RequestContext,
) -> Result<(), ApiError> {
    if content.is_empty() {
        return Ok(());
    }
    match engine.validate_output(content, ctx).await {
        Ok(Some(result)) if result.action == crate::guardrails::GuardrailAction::Intervened => {
            Err(ApiError::GuardrailBlocked {
                violations: result.results,
                processing_time_ms: result.total_processing_time_ms,
            })
        }
        Ok(Some(result)) if result.action == crate::guardrails::GuardrailAction::Redacted => {
            Err(ApiError::GuardrailWarning {
                violations: result.results,
                processing_time_ms: result.total_processing_time_ms,
                redacted_content: content.to_string(),
            })
        }
        Ok(_) => Ok(()),
        Err(e) => {
            tracing::warn!(
                error = %e,
                api_format = %ctx.api_format,
                model = %ctx.model,
                "Output guardrail check failed — failing open, response allowed through"
            );
            Ok(())
        }
    }
}

/// Fetch MCP tools from the gateway and merge them into the existing tool list.
async fn inject_mcp_tools<T: serde::de::DeserializeOwned>(
    mcp: &crate::mcp::McpManager,
    headers: &axum::http::HeaderMap,
    existing_tools: Option<Vec<T>>,
) -> Option<Vec<T>> {
    let mcp_tools = mcp.get_available_tools(headers).await;
    if mcp_tools.is_empty() {
        return existing_tools;
    }
    let mut tools = existing_tools.unwrap_or_default();
    for tool_val in mcp_tools {
        if let Ok(tool) = serde_json::from_value(tool_val) {
            tools.push(tool);
        }
    }
    Some(tools)
}

// ── Direct provider helpers ──────────────────────────────────────

/// Convert an Anthropic API non-streaming response body → OpenAI chat completion JSON.
fn anthropic_response_to_openai(model: &str, body: &Value) -> Value {
    let text = body
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|b| b.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    let finish_reason = body
        .get("stop_reason")
        .and_then(|r| r.as_str())
        .map(|r| if r == "end_turn" { "stop" } else { r })
        .unwrap_or("stop")
        .to_string();

    let prompt_tokens = body
        .get("usage")
        .and_then(|u| u.get("input_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let completion_tokens = body
        .get("usage")
        .and_then(|u| u.get("output_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;

    json!({
        "id": body.get("id").and_then(|v| v.as_str()).unwrap_or("chatcmpl-direct"),
        "object": "chat.completion",
        "created": Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": text },
            "finish_reason": finish_reason,
            "logprobs": null
        }],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": prompt_tokens + completion_tokens
        }
    })
}

/// Convert an OpenAI API non-streaming response body → Anthropic messages response JSON.
fn openai_response_to_anthropic(model: &str, body: &Value) -> Value {
    let text = body
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();

    let stop_reason = body
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("finish_reason"))
        .and_then(|r| r.as_str())
        .map(|r| if r == "stop" { "end_turn" } else { r })
        .unwrap_or("end_turn")
        .to_string();

    let input_tokens = body
        .get("usage")
        .and_then(|u| u.get("prompt_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let output_tokens = body
        .get("usage")
        .and_then(|u| u.get("completion_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;

    json!({
        "id": body.get("id").and_then(|v| v.as_str()).unwrap_or("msg-direct"),
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [{ "type": "text", "text": text }],
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": { "input_tokens": input_tokens, "output_tokens": output_tokens }
    })
}

/// Handle a request via a direct provider (non-Kiro) for the OpenAI-format endpoint.
async fn handle_direct_openai(
    state: &AppState,
    provider_id: ProviderId,
    creds: ProviderCredentials,
    req: &ChatCompletionRequest,
) -> Result<Response, ApiError> {
    use crate::providers::traits::Provider;
    let ctx = ProviderContext {
        credentials: &creds,
        model: &req.model,
    };

    tracing::info!(
        model = %req.model,
        provider = ?provider_id,
        stream = req.stream,
        "Routing to direct provider (OpenAI endpoint)"
    );

    if req.stream {
        let stream = match &provider_id {
            ProviderId::OpenAI => state.openai_provider.stream_openai(&ctx, req).await?,
            ProviderId::Anthropic => state.anthropic_provider.stream_openai(&ctx, req).await?,
            ProviderId::Gemini => state.gemini_provider.stream_openai(&ctx, req).await?,
            ProviderId::Copilot => state.copilot_provider.stream_openai(&ctx, req).await?,
            ProviderId::Qwen => state.qwen_provider.stream_openai(&ctx, req).await?,
            ProviderId::Kiro => unreachable!(),
        };
        let byte_stream = stream.map(|r| r.map_err(|e| std::io::Error::other(e.to_string())));
        Response::builder()
            .status(200)
            .header("Content-Type", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .body(Body::from_stream(byte_stream))
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to build response: {}", e)))
    } else {
        let resp = match &provider_id {
            ProviderId::OpenAI => state.openai_provider.execute_openai(&ctx, req).await?,
            ProviderId::Anthropic => state.anthropic_provider.execute_openai(&ctx, req).await?,
            ProviderId::Gemini => state.gemini_provider.execute_openai(&ctx, req).await?,
            ProviderId::Copilot => state.copilot_provider.execute_openai(&ctx, req).await?,
            ProviderId::Qwen => state.qwen_provider.execute_openai(&ctx, req).await?,
            ProviderId::Kiro => unreachable!(),
        };
        let body = match provider_id {
            ProviderId::OpenAI | ProviderId::Copilot | ProviderId::Qwen => resp.body,
            ProviderId::Anthropic => anthropic_response_to_openai(&req.model, &resp.body),
            ProviderId::Gemini => serde_json::to_value(
                crate::converters::gemini_to_openai::gemini_to_openai(&req.model, &resp.body),
            )
            .unwrap_or_default(),
            ProviderId::Kiro => unreachable!(),
        };
        Ok(Json(body).into_response())
    }
}

/// Handle a request via a direct provider (non-Kiro) for the Anthropic-format endpoint.
async fn handle_direct_anthropic(
    state: &AppState,
    provider_id: ProviderId,
    creds: ProviderCredentials,
    req: &AnthropicMessagesRequest,
) -> Result<Response, ApiError> {
    use crate::providers::traits::Provider;
    let ctx = ProviderContext {
        credentials: &creds,
        model: &req.model,
    };

    tracing::info!(
        model = %req.model,
        provider = ?provider_id,
        stream = req.stream,
        "Routing to direct provider (Anthropic endpoint)"
    );

    if req.stream {
        let stream = match &provider_id {
            ProviderId::OpenAI => state.openai_provider.stream_anthropic(&ctx, req).await?,
            ProviderId::Anthropic => state.anthropic_provider.stream_anthropic(&ctx, req).await?,
            ProviderId::Gemini => state.gemini_provider.stream_anthropic(&ctx, req).await?,
            ProviderId::Copilot => state.copilot_provider.stream_anthropic(&ctx, req).await?,
            ProviderId::Qwen => state.qwen_provider.stream_anthropic(&ctx, req).await?,
            ProviderId::Kiro => unreachable!(),
        };
        let byte_stream = stream.map(|r| r.map_err(|e| std::io::Error::other(e.to_string())));
        Response::builder()
            .status(200)
            .header("Content-Type", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .body(Body::from_stream(byte_stream))
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to build response: {}", e)))
    } else {
        let resp = match &provider_id {
            ProviderId::OpenAI => state.openai_provider.execute_anthropic(&ctx, req).await?,
            ProviderId::Anthropic => {
                state
                    .anthropic_provider
                    .execute_anthropic(&ctx, req)
                    .await?
            }
            ProviderId::Gemini => state.gemini_provider.execute_anthropic(&ctx, req).await?,
            ProviderId::Copilot => state.copilot_provider.execute_anthropic(&ctx, req).await?,
            ProviderId::Qwen => state.qwen_provider.execute_anthropic(&ctx, req).await?,
            ProviderId::Kiro => unreachable!(),
        };
        let body = match provider_id {
            ProviderId::Anthropic => resp.body,
            ProviderId::OpenAI | ProviderId::Copilot | ProviderId::Qwen => {
                openai_response_to_anthropic(&req.model, &resp.body)
            }
            ProviderId::Gemini => serde_json::to_value(
                crate::converters::gemini_to_anthropic::gemini_to_anthropic(&req.model, &resp.body),
            )
            .unwrap_or_default(),
            ProviderId::Kiro => unreachable!(),
        };
        Ok(Json(body).into_response())
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
#[tracing::instrument(skip_all, name = "chat_completions")]
async fn chat_completions_handler(
    State(state): State<AppState>,
    raw_request: axum::http::Request<Body>,
) -> Result<Response, ApiError> {
    // Extract per-user Kiro credentials injected by auth middleware
    let user_creds = raw_request.extensions().get::<UserKiroCreds>().cloned();

    // Extract headers before consuming the request body (needed for MCP tool filtering)
    let headers = raw_request.headers().clone();

    // Parse JSON body
    let body_bytes = axum::body::to_bytes(raw_request.into_body(), 10 * 1024 * 1024)
        .await
        .map_err(|e| ApiError::ValidationError(format!("Failed to read body: {}", e)))?;
    let request: ChatCompletionRequest = serde_json::from_slice(&body_bytes)
        .map_err(|e| ApiError::ValidationError(format!("Invalid JSON: {}", e)))?;

    tracing::info!(
        model = %request.model,
        stream = request.stream,
        messages = request.messages.len(),
        "Request to /v1/chat/completions"
    );

    // Validate request
    if request.messages.is_empty() {
        return Err(ApiError::ValidationError(
            "messages cannot be empty".to_string(),
        ));
    }

    // ── Direct provider routing ──────────────────────────────────────
    // Check if this user has an API key for the model's native provider.
    // If so, route directly to that provider and skip the Kiro pipeline.
    let user_id = user_creds.as_ref().map(|c| c.user_id);
    // Ensure OAuth token is fresh before resolving provider
    if let Some(uid) = user_id {
        if let Some(db) = state.config_db.as_ref() {
            state
                .provider_registry
                .ensure_fresh_token(uid, &request.model, db, state.token_exchanger.as_ref())
                .await;
        }
    }
    let (provider, provider_creds) = state
        .provider_registry
        .resolve_provider(user_id, &request.model, state.config_db.as_deref())
        .await;
    if provider != ProviderId::Kiro {
        return handle_direct_openai(&state, provider, provider_creds.unwrap(), &request).await;
    }
    // ── End direct provider routing ──────────────────────────────────

    // Resolve model name
    let resolution = state.resolver.resolve(&request.model);
    let model_id = resolution.internal_id.clone();

    tracing::debug!(
        model = %request.model,
        internal_id = %model_id,
        source = %resolution.source,
        verified = resolution.is_verified,
        "Model resolved"
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

    // Inject MCP tools into request
    if config.mcp_enabled {
        if let Some(ref mcp) = state.mcp_manager {
            request.tools = inject_mcp_tools(mcp, &headers, request.tools).await;
        }
    }

    // INPUT GUARDRAIL CHECK
    if config.guardrails_enabled {
        if let Some(ref engine) = state.guardrails_engine {
            let user_content = extract_last_user_message(&request.messages);
            let ctx = build_request_context_openai(&request);
            run_input_guardrail_check(engine, &user_content, &ctx).await?;
        }
    }

    // Convert OpenAI request to Kiro format
    let kiro_payload_result = build_kiro_payload(&request, &conversation_id, &profile_arn, &config)
        .map_err(|e| ApiError::ValidationError(e))?;

    let kiro_payload = kiro_payload_result.payload;

    tracing::debug!(
        model = %request.model,
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
        let token = auth
            .get_access_token()
            .await
            .map_err(|e| ApiError::AuthError(format!("Failed to get access token: {}", e)))?;
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
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to build request: {}", e)))?;

    let response = state.http_client.request_with_retry(req).await?;

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

    tracing::debug!(
        model = %request.model,
        include_usage = include_usage,
        stream = request.stream,
        "Stream options: {:?}", request.stream_options
    );

    // Handle streaming vs non-streaming
    if request.stream {
        // Streaming response
        tracing::debug!("Handling streaming response");

        // Use proper streaming conversion from streaming module
        let openai_stream = crate::streaming::stream_kiro_to_openai(
            response,
            &request.model,
            15,
            input_tokens,
            None,
            include_usage,
            config.truncation_recovery,
        )
        .await?;

        // Convert Result<String, ApiError> stream to bytes stream for SSE
        use bytes::Bytes;
        let byte_stream = openai_stream.map(|result| {
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
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to build response: {}", e)))?;

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
        .await?;

        // OUTPUT GUARDRAIL CHECK (non-streaming only)
        if config.guardrails_enabled {
            if let Some(ref engine) = state.guardrails_engine {
                let output_text = extract_assistant_content(&openai_response);
                let ctx = build_request_context_openai(&request);
                run_output_guardrail_check(engine, &output_text, &ctx).await?;
            }
        }

        DEBUG_LOGGER.discard_buffers().await;

        Ok(Json(openai_response).into_response())
    }
}

/// POST /v1/messages - Create Anthropic message
///
/// Handles both streaming and non-streaming message requests in Anthropic format.
/// Converts Anthropic format to Kiro format, makes the request, and converts back.
#[tracing::instrument(skip_all, name = "anthropic_messages")]
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
        model = %request.model,
        stream = request.stream,
        messages = request.messages.len(),
        "Request to /v1/messages"
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
        return Err(ApiError::ValidationError(
            "messages cannot be empty".to_string(),
        ));
    }

    if request.max_tokens <= 0 {
        return Err(ApiError::ValidationError(
            "max_tokens must be positive".to_string(),
        ));
    }

    // ── Direct provider routing ──────────────────────────────────────
    let user_id = user_creds.as_ref().map(|c| c.user_id);
    // Ensure OAuth token is fresh before resolving provider
    if let Some(uid) = user_id {
        if let Some(db) = state.config_db.as_ref() {
            state
                .provider_registry
                .ensure_fresh_token(uid, &request.model, db, state.token_exchanger.as_ref())
                .await;
        }
    }
    let (provider, provider_creds) = state
        .provider_registry
        .resolve_provider(user_id, &request.model, state.config_db.as_deref())
        .await;
    if provider != ProviderId::Kiro {
        return handle_direct_anthropic(&state, provider, provider_creds.unwrap(), &request).await;
    }
    // ── End direct provider routing ──────────────────────────────────

    // Resolve model name
    let resolution = state.resolver.resolve(&request.model);
    let model_id = resolution.internal_id.clone();

    tracing::debug!(
        model = %request.model,
        internal_id = %model_id,
        source = %resolution.source,
        verified = resolution.is_verified,
        "Model resolved"
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

    // Inject MCP tools into request
    if config.mcp_enabled {
        if let Some(ref mcp) = state.mcp_manager {
            request.tools = inject_mcp_tools(mcp, &headers, request.tools).await;
        }
    }

    // INPUT GUARDRAIL CHECK
    if config.guardrails_enabled {
        if let Some(ref engine) = state.guardrails_engine {
            let user_content = extract_last_user_message_anthropic(&request.messages);
            let ctx = build_request_context_anthropic(&request);
            run_input_guardrail_check(engine, &user_content, &ctx).await?;
        }
    }

    // Convert Anthropic request to Kiro format
    let kiro_payload_result =
        build_kiro_payload_anthropic(&request, &conversation_id, &profile_arn, &config)
            .map_err(ApiError::ValidationError)?;

    let kiro_payload = kiro_payload_result.payload;

    tracing::debug!(
        model = %request.model,
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
        let token = auth
            .get_access_token()
            .await
            .map_err(|e| ApiError::AuthError(format!("Failed to get access token: {}", e)))?;
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
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to build request: {}", e)))?;

    let response = state.http_client.request_with_retry(req).await?;

    let input_tokens = count_anthropic_message_tokens(
        &request.messages,
        request.system.as_ref(),
        request.tools.as_ref(),
    );

    // Handle streaming vs non-streaming
    if request.stream {
        // Streaming response
        tracing::debug!("Handling streaming response");

        // Convert response to Anthropic SSE stream
        let first_token_timeout = config.first_token_timeout;
        let anthropic_stream = crate::streaming::stream_kiro_to_anthropic(
            response,
            &request.model,
            first_token_timeout,
            input_tokens,
            None,
            config.truncation_recovery,
        )
        .await?;

        // Convert to raw SSE response (stream already contains properly formatted SSE events)
        // Don't use Axum's Sse wrapper as it would double-wrap the events
        let byte_stream = anthropic_stream.map(|result| {
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
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to build response: {}", e)))?;

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
        .await?;

        // OUTPUT GUARDRAIL CHECK (non-streaming only)
        if config.guardrails_enabled {
            if let Some(ref engine) = state.guardrails_engine {
                let output_text = extract_assistant_content_anthropic(&anthropic_response);
                let ctx = build_request_context_anthropic(&request);
                run_output_guardrail_check(engine, &output_text, &ctx).await?;
            }
        }

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

        AppState {
            model_cache: cache,
            auth_manager,
            http_client,
            resolver,
            config: Arc::new(RwLock::new(config)),
            setup_complete: Arc::new(AtomicBool::new(true)),
            config_db: None,
            session_cache: Arc::new(DashMap::new()),
            api_key_cache: Arc::new(DashMap::new()),
            kiro_token_cache: Arc::new(DashMap::new()),
            oauth_pending: Arc::new(DashMap::new()),
            guardrails_engine: None,
            mcp_manager: None,
            provider_registry: Arc::new(ProviderRegistry::new()),
            anthropic_provider: Arc::new(AnthropicProvider::new()),
            openai_provider: Arc::new(OpenAIProvider::new()),
            gemini_provider: Arc::new(GeminiProvider::new()),
            copilot_provider: Arc::new(CopilotProvider::new()),
            qwen_provider: Arc::new(QwenProvider::new()),
            provider_oauth_pending: Arc::new(DashMap::new()),
            token_exchanger: Arc::new(crate::web_ui::provider_oauth::HttpTokenExchanger::new()),
            copilot_token_cache: Arc::new(DashMap::new()),
            copilot_device_pending: Arc::new(DashMap::new()),
            qwen_device_pending: Arc::new(DashMap::new()),
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
