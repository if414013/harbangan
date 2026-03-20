use std::time::Duration;

use axum::http::HeaderMap;
use serde_json::Value;

use crate::config::Config;
use crate::error::ApiError;
use crate::models::anthropic::AnthropicMessagesRequest;
use crate::models::openai::ChatCompletionRequest;
use crate::providers::rate_limiter::{AccountId, RateLimitTracker};
use crate::providers::registry::ProviderRegistry;
use crate::providers::types::{ProviderCredentials, ProviderId};

use super::state::{AppState, UserKiroCreds, PROXY_USER_ID};

/// Result of provider routing: which provider to use and optional credentials.
pub(crate) struct ProviderRouting {
    pub provider_id: ProviderId,
    pub provider_creds: Option<ProviderCredentials>,
    /// The model name with the provider prefix stripped (e.g. "claude-opus-4-6" from "anthropic/claude-opus-4-6").
    pub stripped_model: Option<String>,
    /// Account ID for rate-limit tracking (None in proxy-only mode or single-account).
    pub account_id: Option<AccountId>,
}

/// Resolve the effective user_id for provider routing.
///
/// If user credentials are present, uses their user_id. In proxy mode (no user creds),
/// falls back to the sentinel PROXY_USER_ID so the registry can still route.
pub(crate) fn resolve_user_id(
    user_creds: Option<&UserKiroCreds>,
    is_proxy: bool,
) -> Option<uuid::Uuid> {
    user_creds
        .map(|c| c.user_id)
        .or(if is_proxy { Some(PROXY_USER_ID) } else { None })
}

/// Resolve which provider to route a request to, refreshing OAuth tokens if needed.
pub(crate) async fn resolve_provider_routing(
    state: &AppState,
    user_creds: Option<&UserKiroCreds>,
    model: &str,
) -> ProviderRouting {
    let user_id = resolve_user_id(user_creds, state.proxy_api_key_hash.is_some());
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

    // Use multi-account balancing when config_db is available
    if state.config_db.is_some() {
        let (provider_id, provider_creds, account_id) = state
            .provider_registry
            .resolve_provider_with_balancing(
                user_id,
                model,
                state.config_db.as_deref(),
                &state.rate_tracker,
            )
            .await;

        return ProviderRouting {
            provider_id,
            provider_creds,
            stripped_model,
            account_id,
        };
    }

    // Proxy-only mode: single-account resolution
    let (provider_id, provider_creds) = state
        .provider_registry
        .resolve_provider(user_id, model, state.config_db.as_deref())
        .await;

    ProviderRouting {
        provider_id,
        provider_creds,
        stripped_model,
        account_id: None,
    }
}

/// Parse the Retry-After header value as seconds.
pub(crate) fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
}

/// Update rate-limit tracking from response headers.
pub(crate) fn update_rate_limits(
    rate_tracker: &RateLimitTracker,
    account_id: &Option<AccountId>,
    provider_id: &ProviderId,
    headers: &HeaderMap,
) {
    if let Some(aid) = account_id {
        rate_tracker.update_from_headers(aid, provider_id, headers);
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
        account_label: "default".to_string(),
    })
}

/// Read the config snapshot for guardrail checks in the pipeline.
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

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_resolve_user_id_with_creds() {
        let uid = Uuid::new_v4();
        let creds = UserKiroCreds {
            user_id: uid,
            access_token: "tok".to_string(),
            refresh_token: "rtok".to_string(),
            region: "us-east-1".to_string(),
        };
        // With creds, always returns creds.user_id regardless of proxy flag
        assert_eq!(resolve_user_id(Some(&creds), false), Some(uid));
        assert_eq!(resolve_user_id(Some(&creds), true), Some(uid));
    }

    #[test]
    fn test_resolve_user_id_proxy_no_creds() {
        // Proxy mode without creds → PROXY_USER_ID sentinel
        let result = resolve_user_id(None, true);
        assert_eq!(result, Some(PROXY_USER_ID));
    }

    #[test]
    fn test_resolve_user_id_non_proxy_no_creds() {
        // Non-proxy mode without creds → None
        assert_eq!(resolve_user_id(None, false), None);
    }

    #[test]
    fn test_parse_retry_after_valid() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", "30".parse().unwrap());
        assert_eq!(parse_retry_after(&headers), Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_parse_retry_after_missing() {
        let headers = HeaderMap::new();
        assert_eq!(parse_retry_after(&headers), None);
    }

    #[test]
    fn test_parse_retry_after_non_numeric() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", "abc".parse().unwrap());
        assert_eq!(parse_retry_after(&headers), None);
    }
}
