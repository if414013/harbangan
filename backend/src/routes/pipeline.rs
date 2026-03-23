use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::http::HeaderMap;
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde::Deserialize;
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
    /// The original client model string used for provider selection and retries.
    pub routing_model: String,
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
    let removed = ProviderRegistry::removed_provider_for_model(model).or_else(|| {
        ProviderRegistry::parse_prefixed_model(model).and_then(|(_, stripped_model)| {
            ProviderRegistry::removed_provider_for_model(&stripped_model)
        })
    });
    if let Some(removed) = removed {
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
    let stripped_model =
        ProviderRegistry::parse_prefixed_model(model).map(|(_provider, model_id)| model_id);

    // Ensure OAuth token is fresh before resolving provider
    if let Some(uid) = user_id {
        if let Some(db) = state.config_db.as_ref() {
            state
                .provider_registry
                .ensure_fresh_token(uid, model, db, state.token_exchanger.as_ref())
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
            routing_model: model.to_string(),
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
        routing_model: model.to_string(),
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
    let routing_model = routing.routing_model.clone();
    *routing = resolve_provider_routing(state, user_creds, &routing_model).await;
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct UsageMetricSnapshot {
    pub input_tokens: i64,
    pub output_tokens: i64,
}

impl UsageMetricSnapshot {
    fn apply_fields(&mut self, fields: UsageMetricFields) {
        if let Some(input_tokens) = fields.input_tokens {
            self.input_tokens = input_tokens;
        }
        if let Some(output_tokens) = fields.output_tokens {
            self.output_tokens = output_tokens;
        }
    }

    fn has_usage(&self) -> bool {
        self.input_tokens > 0 || self.output_tokens > 0
    }

    fn normalize_for_persistence(self) -> Option<NormalizedUsageMetric> {
        if self.input_tokens < 0 || self.output_tokens < 0 {
            tracing::warn!(
                input_tokens = self.input_tokens,
                output_tokens = self.output_tokens,
                "Skipping usage metrics with negative token counts"
            );
            return None;
        }

        let input_tokens = match i32::try_from(self.input_tokens) {
            Ok(value) => value,
            Err(_) => {
                tracing::warn!(
                    input_tokens = self.input_tokens,
                    output_tokens = self.output_tokens,
                    "Skipping usage metrics with input token count outside i32 range"
                );
                return None;
            }
        };

        let output_tokens = match i32::try_from(self.output_tokens) {
            Ok(value) => value,
            Err(_) => {
                tracing::warn!(
                    input_tokens = self.input_tokens,
                    output_tokens = self.output_tokens,
                    "Skipping usage metrics with output token count outside i32 range"
                );
                return None;
            }
        };

        Some(NormalizedUsageMetric {
            input_tokens,
            output_tokens,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NormalizedUsageMetric {
    input_tokens: i32,
    output_tokens: i32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
struct UsageMetricFields {
    #[serde(default, alias = "prompt_tokens")]
    input_tokens: Option<i64>,
    #[serde(default, alias = "completion_tokens")]
    output_tokens: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct UsageMetricEnvelope {
    #[serde(default)]
    usage: Option<UsageMetricFields>,
}

pub(crate) fn extract_usage_metric_snapshot(body: &Value) -> Option<UsageMetricSnapshot> {
    let usage = body.get("usage").cloned()?;
    let fields = serde_json::from_value::<UsageMetricFields>(usage).ok()?;
    let mut snapshot = UsageMetricSnapshot::default();
    snapshot.apply_fields(fields);
    snapshot.has_usage().then_some(snapshot)
}

/// Persist sanitized usage counters from a non-streaming response body.
pub(crate) fn persist_non_streaming_usage(
    snapshot: Option<UsageMetricSnapshot>,
    config_db: &Option<Arc<ConfigDb>>,
    user_id: Option<Uuid>,
    provider_id: &ProviderId,
    model: &str,
) {
    let Some(snapshot) = snapshot else { return };
    let Some(db) = config_db else { return };
    let Some(uid) = user_id else { return };
    if !snapshot.has_usage() {
        return;
    }
    let Some(metric) = snapshot.normalize_for_persistence() else {
        return;
    };
    let cost = crate::cost::calculate_cost(
        model,
        i64::from(metric.input_tokens),
        i64::from(metric.output_tokens),
    );
    let db = db.clone();
    let provider_str = provider_id.to_string();
    let model = model.to_string();
    tokio::spawn(async move {
        if let Err(error) = db
            .insert_usage_metric(
                uid,
                &provider_str,
                &model,
                metric.input_tokens,
                metric.output_tokens,
                cost,
            )
            .await
        {
            tracing::warn!(error = ?error, "Failed to persist usage metrics");
        }
    });
}

/// Wrap a streaming response to extract usage data and persist it after the stream ends.
///
/// Passes all chunks through unchanged (no latency impact). Inspects each chunk's
/// text representation for `"usage":` and extracts token counts. On stream end,
/// if tokens > 0, spawns a task to call `insert_usage_metric`.
pub(crate) fn wrap_stream_with_usage_metrics(
    stream: Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>>,
    config_db: Option<Arc<ConfigDb>>,
    user_id: Option<Uuid>,
    provider_id: ProviderId,
    model: String,
) -> Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>> {
    let Some(db) = config_db else {
        return stream;
    };
    let Some(uid) = user_id else {
        return stream;
    };

    let state = Arc::new(std::sync::Mutex::new(StreamUsageState::default()));
    let provider_str = provider_id.to_string();

    let state_for_chunks = state.clone();

    Box::pin(
        stream
            .map(move |item| {
                if let Ok(ref chunk) = item {
                    let mut usage_state = state_for_chunks
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    update_stream_usage_from_chunk(&mut usage_state, chunk.as_ref());
                }
                item
            })
            .chain(futures::stream::once({
                let model = model.clone();
                let provider_str = provider_str.clone();
                let state = state.clone();
                async move {
                    let snapshot = {
                        let mut usage_state = state
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        finalize_stream_usage(&mut usage_state);
                        usage_state.snapshot
                    };
                    if snapshot.has_usage() {
                        if let Some(metric) = snapshot.normalize_for_persistence() {
                            let cost = crate::cost::calculate_cost(
                                &model,
                                i64::from(metric.input_tokens),
                                i64::from(metric.output_tokens),
                            );
                            tokio::spawn(async move {
                                if let Err(error) = db
                                    .insert_usage_metric(
                                        uid,
                                        &provider_str,
                                        &model,
                                        metric.input_tokens,
                                        metric.output_tokens,
                                        cost,
                                    )
                                    .await
                                {
                                    tracing::warn!(
                                        error = ?error,
                                        "Failed to persist streaming usage metrics"
                                    );
                                }
                            });
                        }
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

#[derive(Default)]
struct StreamUsageState {
    buffer: String,
    snapshot: UsageMetricSnapshot,
}

fn update_stream_usage_from_chunk(state: &mut StreamUsageState, chunk: &[u8]) {
    state.buffer.push_str(&String::from_utf8_lossy(chunk));

    while let Some(pos) = state.buffer.find('\n') {
        let line = state.buffer[..pos].trim_end_matches('\r').to_string();
        state.buffer.drain(..pos + 1);
        update_stream_usage_from_line(state, &line);
    }
}

fn finalize_stream_usage(state: &mut StreamUsageState) {
    if state.buffer.is_empty() {
        return;
    }

    let remaining = std::mem::take(&mut state.buffer);
    for line in remaining.lines() {
        update_stream_usage_from_line(state, line.trim_end_matches('\r'));
    }
}

fn update_stream_usage_from_line(state: &mut StreamUsageState, line: &str) {
    let Some(data) = line.strip_prefix("data: ") else {
        return;
    };
    if data == "[DONE]" {
        return;
    }
    let Ok(parsed) = serde_json::from_str::<UsageMetricEnvelope>(data) else {
        return;
    };
    let Some(usage) = parsed.usage else {
        return;
    };
    state.snapshot.apply_fields(usage);
}

#[cfg(test)]
mod tests {
    use dashmap::DashMap;
    use futures::stream::Stream;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use std::sync::RwLock;
    use uuid::Uuid;

    use super::*;
    use crate::auth::AuthManager;
    use crate::cache::ModelCache;
    use crate::http_client::KiroHttpClient;
    use crate::providers::registry::ProviderRegistry;
    use crate::resolver::ModelResolver;

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

    // ── persist_non_streaming_usage tests ─────────────────────────────

    #[test]
    fn test_persist_non_streaming_usage_no_db_is_noop() {
        // Should not panic — just returns early
        persist_non_streaming_usage(
            Some(UsageMetricSnapshot {
                input_tokens: 100,
                output_tokens: 50,
            }),
            &None,
            Some(Uuid::new_v4()),
            &ProviderId::Anthropic,
            "claude-sonnet-4",
        );
    }

    #[test]
    fn test_persist_non_streaming_usage_no_user_is_noop() {
        persist_non_streaming_usage(
            Some(UsageMetricSnapshot {
                input_tokens: 100,
                output_tokens: 50,
            }),
            &None, // no db
            None,  // no user
            &ProviderId::Anthropic,
            "claude-sonnet-4",
        );
    }

    #[test]
    fn test_persist_non_streaming_usage_none_is_noop() {
        persist_non_streaming_usage(
            None,
            &None,
            Some(Uuid::new_v4()),
            &ProviderId::Anthropic,
            "claude-sonnet-4",
        );
    }

    #[test]
    fn test_extract_usage_metric_snapshot_openai_fields() {
        let body = serde_json::json!({
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50
            }
        });

        assert_eq!(
            extract_usage_metric_snapshot(&body),
            Some(UsageMetricSnapshot {
                input_tokens: 100,
                output_tokens: 50,
            })
        );
    }

    #[test]
    fn test_extract_usage_metric_snapshot_anthropic_fields() {
        let body = serde_json::json!({
            "usage": {
                "input_tokens": 9,
                "output_tokens": 7
            }
        });

        assert_eq!(
            extract_usage_metric_snapshot(&body),
            Some(UsageMetricSnapshot {
                input_tokens: 9,
                output_tokens: 7,
            })
        );
    }

    #[test]
    fn test_extract_usage_metric_snapshot_zero_usage_is_none() {
        let body = serde_json::json!({
            "usage": {
                "prompt_tokens": 0,
                "completion_tokens": 0
            }
        });

        assert!(extract_usage_metric_snapshot(&body).is_none());
    }

    #[test]
    fn test_usage_metric_snapshot_normalize_rejects_negative_values() {
        let snapshot = UsageMetricSnapshot {
            input_tokens: -1,
            output_tokens: 5,
        };

        assert!(snapshot.normalize_for_persistence().is_none());
    }

    #[test]
    fn test_usage_metric_snapshot_normalize_rejects_out_of_range_values() {
        let snapshot = UsageMetricSnapshot {
            input_tokens: i64::from(i32::MAX) + 1,
            output_tokens: 5,
        };

        assert!(snapshot.normalize_for_persistence().is_none());
    }

    // ── wrap_stream_with_usage_metrics tests ──────────────────────────

    #[test]
    fn test_wrap_stream_with_usage_metrics_no_db_returns_original() {
        let stream: Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>> =
            Box::pin(futures::stream::empty());
        // Should return the stream unchanged (no wrapping)
        let result = wrap_stream_with_usage_metrics(
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
    fn test_wrap_stream_with_usage_metrics_no_user_returns_original() {
        let stream: Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>> =
            Box::pin(futures::stream::empty());
        let result = wrap_stream_with_usage_metrics(
            stream,
            None, // no db
            None, // no user
            ProviderId::Anthropic,
            "test-model".to_string(),
        );
        drop(result);
    }

    #[test]
    fn test_update_stream_usage_from_chunk_handles_split_openai_usage_event() {
        let mut state = StreamUsageState::default();

        update_stream_usage_from_chunk(&mut state, br#"data: {"usage":{"prompt_tokens":123"#);
        assert_eq!(state.snapshot, UsageMetricSnapshot::default());

        update_stream_usage_from_chunk(
            &mut state,
            br#","completion_tokens":45}}

"#,
        );

        assert_eq!(
            state.snapshot,
            UsageMetricSnapshot {
                input_tokens: 123,
                output_tokens: 45,
            }
        );
    }

    #[test]
    fn test_finalize_stream_usage_parses_remaining_anthropic_usage_line() {
        let mut state = StreamUsageState::default();

        update_stream_usage_from_chunk(
            &mut state,
            br#"data: {"usage":{"input_tokens":9,"output_tokens":7}}"#,
        );
        finalize_stream_usage(&mut state);

        assert_eq!(
            state.snapshot,
            UsageMetricSnapshot {
                input_tokens: 9,
                output_tokens: 7,
            }
        );
    }

    #[test]
    fn test_validate_model_provider_rejects_removed() {
        let result = validate_model_provider("gemini-2.5-pro");
        assert!(result.is_err());
        let result = validate_model_provider("qwen-coder-plus");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_model_provider_rejects_removed_with_explicit_prefix() {
        let result = validate_model_provider("anthropic/gemini-2.5-pro");
        assert!(result.is_err());
        let result = validate_model_provider("openai_codex/qwen-coder-plus");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_model_provider_accepts_active() {
        assert!(validate_model_provider("claude-sonnet-4").is_ok());
        assert!(validate_model_provider("gpt-4o").is_ok());
        assert!(validate_model_provider("auto").is_ok());
    }

    fn create_test_state(
        provider_registry: Arc<ProviderRegistry>,
        proxy_api_key_hash: Option<[u8; 32]>,
    ) -> AppState {
        let cache = ModelCache::new(3600);
        let http_client = Arc::new(KiroHttpClient::new(20, 30, 300, 3).unwrap());
        let auth_manager = Arc::new(tokio::sync::RwLock::new(
            AuthManager::new_for_testing("test-token".to_string(), "us-east-1".to_string(), 300)
                .unwrap(),
        ));
        let resolver = ModelResolver::new(cache.clone(), HashMap::new());
        let config = Config {
            fake_reasoning_max_tokens: 10_000,
            ..Config::with_defaults()
        };
        let config_arc = Arc::new(RwLock::new(config));

        AppState {
            proxy_api_key_hash,
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
            provider_registry,
            providers: crate::providers::build_provider_map(http_client, auth_manager, config_arc),
            provider_oauth_pending: Arc::new(DashMap::new()),
            token_exchanger: Arc::new(crate::web_ui::provider_oauth::HttpTokenExchanger::new()),
            login_rate_limiter: Arc::new(DashMap::new()),
            rate_tracker: Arc::new(crate::providers::rate_limiter::RateLimitTracker::new()),
        }
    }

    #[tokio::test]
    async fn test_handle_rate_limit_retry_preserves_explicit_prefix_routing() {
        let mut proxy_credentials = HashMap::new();
        proxy_credentials.insert(
            ProviderId::Anthropic,
            ProviderCredentials {
                provider: ProviderId::Anthropic,
                access_token: "anthropic-token".to_string(),
                base_url: None,
                account_label: "proxy".to_string(),
            },
        );
        proxy_credentials.insert(
            ProviderId::OpenAICodex,
            ProviderCredentials {
                provider: ProviderId::OpenAICodex,
                access_token: "openai-token".to_string(),
                base_url: None,
                account_label: "proxy".to_string(),
            },
        );
        let registry = Arc::new(ProviderRegistry::new_with_proxy(
            proxy_credentials,
            std::collections::HashSet::new(),
        ));
        let state = create_test_state(Arc::clone(&registry), Some([7; 32]));

        let mut routing =
            resolve_provider_routing(&state, None, "openai_codex/claude-sonnet-4").await;
        assert_eq!(routing.provider_id, ProviderId::OpenAICodex);
        assert_eq!(routing.routing_model, "openai_codex/claude-sonnet-4");
        assert_eq!(routing.stripped_model.as_deref(), Some("claude-sonnet-4"));

        let mut creds = routing
            .provider_creds
            .clone()
            .expect("expected credentials");
        let mut provider = state
            .providers
            .get(&routing.provider_id)
            .expect("provider should exist");

        let should_retry = handle_rate_limit_retry(
            &state,
            None,
            &mut routing,
            &mut creds,
            &mut provider,
            0,
            3,
            &None,
        )
        .await
        .expect("retry resolution should succeed");

        assert!(should_retry);
        assert_eq!(routing.provider_id, ProviderId::OpenAICodex);
        assert_eq!(routing.routing_model, "openai_codex/claude-sonnet-4");
        assert_eq!(creds.provider, ProviderId::OpenAICodex);
    }
}
