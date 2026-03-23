use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::http::HeaderMap;
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::Value;
use uuid::Uuid;

use crate::config::Config;
use crate::error::ApiError;
use crate::models::anthropic::AnthropicMessagesRequest;
use crate::models::openai::ChatCompletionRequest;
use crate::providers::rate_limiter::{AccountId, RateLimitTracker};
use crate::providers::registry::ProviderRegistry;
use crate::providers::types::{ProviderCredentials, ProviderId, ProviderStreamItem};
use crate::web_ui::config_db::ConfigDb;

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

/// Reject model names that belong to providers no longer supported by this gateway.
///
/// Call this before `resolve_provider_routing` so clients get a clear 400 error
/// instead of having their request silently fall through to Kiro.
pub(crate) fn validate_model_provider(model: &str) -> Result<(), ApiError> {
    if let Some(removed) = ProviderRegistry::removed_provider_for_model(model) {
        return Err(ApiError::ValidationError(format!(
            "The {removed} provider has been removed. Model '{model}' is no longer supported."
        )));
    }
    Ok(())
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

/// Handle rate-limit retry and account re-resolution.
///
/// When a 429 error is received, marks the account as rate-limited, re-resolves
/// the provider routing (which may pick a different account or provider), and
/// updates the mutable references. Returns `true` if the caller should retry,
/// `false` if retries are exhausted or no new credentials are available.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_rate_limit_retry<'a>(
    state: &'a AppState,
    user_creds: Option<&UserKiroCreds>,
    model: &str,
    routing: &mut ProviderRouting,
    creds: &mut ProviderCredentials,
    provider: &mut &'a Arc<dyn crate::providers::traits::Provider>,
    attempt: usize,
    max_attempts: usize,
    headers: &Option<axum::http::HeaderMap>,
) -> Result<bool, ApiError> {
    if attempt >= max_attempts - 1 {
        return Ok(false);
    }
    let retry_after = headers.as_ref().and_then(parse_retry_after);
    if let Some(ref aid) = routing.account_id {
        tracing::info!(
            attempt,
            account_label = %aid.account_label,
            retry_after_secs = ?retry_after.map(|d| d.as_secs()),
            "Rate limited, retrying with different account"
        );
        state.rate_tracker.mark_limited(aid, retry_after);
    }
    *routing = resolve_provider_routing(state, user_creds, model).await;
    if let Some(ref new_creds) = routing.provider_creds {
        *creds = new_creds.clone();
    } else {
        return Ok(false);
    }
    *provider = state.providers.get(&routing.provider_id).ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!(
            "Provider {:?} not registered",
            routing.provider_id
        ))
    })?;
    Ok(true)
}

/// Record usage from a non-streaming response body.
///
/// Extracts token counts from the body's `usage` field (supporting both OpenAI
/// and Anthropic field names) and spawns a background task to persist the record.
pub(crate) fn record_non_streaming_usage(
    body: &Value,
    config_db: &Option<Arc<ConfigDb>>,
    user_id: Option<Uuid>,
    provider_id: &ProviderId,
    model: &str,
) {
    let Some(db) = config_db else { return };
    let Some(uid) = user_id else { return };
    let Some(usage) = body.get("usage") else {
        return;
    };
    // Support both OpenAI (prompt_tokens/completion_tokens) and Anthropic (input_tokens/output_tokens)
    let input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let output_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    if input_tokens == 0 && output_tokens == 0 {
        return;
    }
    let cost = crate::cost::calculate_cost(model, input_tokens as i64, output_tokens as i64);
    let db = db.clone();
    let provider_str = provider_id.to_string();
    let model = model.to_string();
    tokio::spawn(async move {
        if let Err(e) = db
            .insert_usage_record(
                uid,
                &provider_str,
                &model,
                input_tokens,
                output_tokens,
                cost,
            )
            .await
        {
            tracing::warn!(error = ?e, "Failed to record usage");
        }
    });
}

/// Wrap a streaming response to extract usage data and persist it after the stream ends.
///
/// Passes all chunks through unchanged (no latency impact). Inspects each chunk's
/// text representation for `"usage":` and extracts token counts. On stream end,
/// if tokens > 0, spawns a task to call `insert_usage_record`.
pub(crate) fn wrap_stream_with_usage_tracking(
    stream: Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>>,
    config_db: Option<Arc<ConfigDb>>,
    user_id: Option<Uuid>,
    provider_id: ProviderId,
    model: String,
) -> Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>> {
    use std::sync::atomic::{AtomicI64, Ordering};

    let Some(db) = config_db else {
        return stream;
    };
    let Some(uid) = user_id else {
        return stream;
    };

    let input_tokens = Arc::new(AtomicI64::new(0));
    let output_tokens = Arc::new(AtomicI64::new(0));
    let provider_str = provider_id.to_string();

    let input_ref = input_tokens.clone();
    let output_ref = output_tokens.clone();

    Box::pin(
        stream
            .map(move |item| {
                if let Ok(ref chunk) = item {
                    if let Ok(text) = std::str::from_utf8(chunk.as_ref()) {
                        for line in text.lines() {
                            if let Some(data) = line.strip_prefix("data: ") {
                                if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                                    if let Some(usage) = parsed.get("usage") {
                                        // OpenAI format
                                        if let Some(pt) =
                                            usage.get("prompt_tokens").and_then(|v| v.as_i64())
                                        {
                                            input_ref.store(pt, Ordering::Relaxed);
                                        }
                                        if let Some(ct) =
                                            usage.get("completion_tokens").and_then(|v| v.as_i64())
                                        {
                                            output_ref.store(ct, Ordering::Relaxed);
                                        }
                                        // Anthropic format
                                        if let Some(it) =
                                            usage.get("input_tokens").and_then(|v| v.as_i64())
                                        {
                                            input_ref.store(it, Ordering::Relaxed);
                                        }
                                        if let Some(ot) =
                                            usage.get("output_tokens").and_then(|v| v.as_i64())
                                        {
                                            output_ref.store(ot, Ordering::Relaxed);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                item
            })
            .chain(futures::stream::once({
                let model = model.clone();
                let provider_str = provider_str.clone();
                async move {
                    let inp = input_tokens.load(Ordering::Relaxed);
                    let out = output_tokens.load(Ordering::Relaxed);
                    if inp > 0 || out > 0 {
                        let cost = crate::cost::calculate_cost(&model, inp, out);
                        tokio::spawn(async move {
                            if let Err(e) = db
                                .insert_usage_record(
                                    uid,
                                    &provider_str,
                                    &model,
                                    inp as i32,
                                    out as i32,
                                    cost,
                                )
                                .await
                            {
                                tracing::warn!(error = ?e, "Failed to record streaming usage");
                            }
                        });
                    }
                    Ok(Bytes::new())
                }
            }))
            .filter(|item| {
                let keep = match item {
                    Ok(bytes) => !bytes.is_empty(),
                    Err(_) => true,
                };
                futures::future::ready(keep)
            }),
    )
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;

    use futures::stream::Stream;
    use uuid::Uuid;

    use super::*;

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

    // ── record_non_streaming_usage tests ──────────────────────────────

    #[test]
    fn test_record_non_streaming_usage_no_db_is_noop() {
        let body = serde_json::json!({"usage": {"prompt_tokens": 100, "completion_tokens": 50}});
        // Should not panic — just returns early
        record_non_streaming_usage(
            &body,
            &None,
            Some(Uuid::new_v4()),
            &ProviderId::Anthropic,
            "claude-sonnet-4",
        );
    }

    #[test]
    fn test_record_non_streaming_usage_no_user_is_noop() {
        let body = serde_json::json!({"usage": {"prompt_tokens": 100, "completion_tokens": 50}});
        record_non_streaming_usage(
            &body,
            &None, // no db
            None,  // no user
            &ProviderId::Anthropic,
            "claude-sonnet-4",
        );
    }

    #[test]
    fn test_record_non_streaming_usage_no_usage_field_is_noop() {
        let body = serde_json::json!({"choices": []});
        record_non_streaming_usage(
            &body,
            &None,
            Some(Uuid::new_v4()),
            &ProviderId::Anthropic,
            "claude-sonnet-4",
        );
    }

    // ── wrap_stream_with_usage_tracking tests ─────────────────────────

    #[test]
    fn test_wrap_stream_no_db_returns_original() {
        let stream: Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>> =
            Box::pin(futures::stream::empty());
        // Should return the stream unchanged (no wrapping)
        let result = wrap_stream_with_usage_tracking(
            stream,
            None, // no db
            Some(Uuid::new_v4()),
            ProviderId::Anthropic,
            "test-model".to_string(),
        );
        // Just verify it returns without panic
        drop(result);
    }

    #[test]
    fn test_wrap_stream_no_user_returns_original() {
        let stream: Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>> =
            Box::pin(futures::stream::empty());
        let result = wrap_stream_with_usage_tracking(
            stream,
            None, // no db
            None, // no user
            ProviderId::Anthropic,
            "test-model".to_string(),
        );
        drop(result);
    }

    #[test]
    fn test_validate_model_provider_rejects_removed() {
        let result = validate_model_provider("gemini-2.5-pro");
        assert!(result.is_err());
        let result = validate_model_provider("qwen-coder-plus");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_model_provider_accepts_active() {
        assert!(validate_model_provider("claude-sonnet-4").is_ok());
        assert!(validate_model_provider("gpt-4o").is_ok());
        assert!(validate_model_provider("auto").is_ok());
    }
}
