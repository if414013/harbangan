use serde_json::Value;

use crate::config::Config;
use crate::error::ApiError;
use crate::models::anthropic::AnthropicMessagesRequest;
use crate::models::openai::ChatCompletionRequest;
use crate::providers::registry::ProviderRegistry;
use crate::providers::types::{ProviderCredentials, ProviderId};

use super::state::{AppState, UserKiroCreds};

/// Result of provider routing: which provider to use and optional credentials.
pub(crate) struct ProviderRouting {
    pub provider_id: ProviderId,
    pub provider_creds: Option<ProviderCredentials>,
    /// The model name with the provider prefix stripped (e.g. "claude-opus-4-6" from "anthropic/claude-opus-4-6").
    pub stripped_model: Option<String>,
}

/// Resolve which provider to route a request to, refreshing OAuth tokens if needed.
pub(crate) async fn resolve_provider_routing(
    state: &AppState,
    user_creds: Option<&UserKiroCreds>,
    model: &str,
) -> ProviderRouting {
    let user_id = user_creds.map(|c| c.user_id);
    let (raw_model, stripped_model) =
        if let Some((_provider, model_id)) = ProviderRegistry::parse_prefixed_model(model) {
            (model.to_string(), Some(model_id))
        } else {
            (model.to_string(), None)
        };
    let routing_model = stripped_model.as_deref().unwrap_or(&raw_model);

    // Ensure OAuth token is fresh before resolving provider
    if let Some(uid) = user_id {
        if let Some(db) = state.config_db.as_ref() {
            state
                .provider_registry
                .ensure_fresh_token(uid, routing_model, db, state.token_exchanger.as_ref())
                .await;
        }
    }

    let (provider_id, provider_creds) = state
        .provider_registry
        .resolve_provider(user_id, model, state.config_db.as_deref())
        .await;

    ProviderRouting {
        provider_id,
        provider_creds,
        stripped_model,
    }
}

/// Build ProviderCredentials for the Kiro pipeline from per-user creds or global auth.
///
/// The access_token is the Kiro access token, and base_url is the Kiro API URL
/// (constructed from the region).
pub(crate) async fn build_kiro_credentials(
    state: &AppState,
    user_creds: Option<&UserKiroCreds>,
) -> Result<ProviderCredentials, ApiError> {
    let (access_token, region) = if let Some(creds) = user_creds {
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

    let kiro_api_url = format!(
        "https://codewhisperer.{}.amazonaws.com/generateAssistantResponse",
        region
    );

    Ok(ProviderCredentials {
        provider: ProviderId::Kiro,
        access_token,
        base_url: Some(kiro_api_url),
    })
}

/// Read the config snapshot for guard/MCP checks in the pipeline.
pub(crate) fn read_config(state: &AppState) -> Config {
    state
        .config
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

/// Extract the last user message content from OpenAI-format messages.
pub(crate) fn extract_last_user_message(messages: &[crate::models::openai::ChatMessage]) -> String {
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
pub(crate) fn extract_last_user_message_anthropic(
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
pub(crate) fn extract_assistant_content(response: &Value) -> String {
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
pub(crate) fn extract_assistant_content_anthropic(response: &Value) -> String {
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
pub(crate) fn build_request_context_openai(
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
pub(crate) fn build_request_context_anthropic(
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

/// Run input guardrails validation. Returns Err(GuardrailBlocked) if the content is blocked.
/// On engine errors, logs a warning and allows the request through (fail-open).
pub(crate) async fn run_input_guardrail_check(
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
pub(crate) async fn run_output_guardrail_check(
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
pub(crate) async fn inject_mcp_tools<T: serde::de::DeserializeOwned>(
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
