use axum::{
    body::Body,
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use futures::stream::StreamExt;

use crate::error::ApiError;
use crate::middleware::DEBUG_LOGGER;
use crate::models::anthropic::AnthropicMessagesRequest;
use crate::providers::types::{ProviderContext, ProviderId};

use super::pipeline::{
    build_kiro_credentials, build_request_context_anthropic, extract_assistant_content_anthropic,
    extract_last_user_message_anthropic, extract_usage_metric_snapshot, handle_rate_limit_retry,
    persist_non_streaming_usage, read_config, resolve_provider_routing, run_input_guardrail_check,
    run_output_guardrail_check, update_rate_limits, validate_model_provider,
    validate_model_visibility, wrap_stream_with_usage_metrics,
};
use super::state::{AppState, UserKiroCreds};

/// POST /v1/messages - Create Anthropic message
///
/// Handles both streaming and non-streaming message requests in Anthropic format.
/// All providers (including Kiro) flow through the Provider trait.
#[tracing::instrument(skip_all, name = "anthropic_messages")]
pub(crate) async fn anthropic_messages_handler(
    State(state): State<AppState>,
    raw_request: axum::http::Request<Body>,
) -> Result<Response, ApiError> {
    let user_creds = raw_request.extensions().get::<UserKiroCreds>().cloned();
    let headers = raw_request.headers().clone();

    // Parse JSON body
    let body_bytes = axum::body::to_bytes(raw_request.into_body(), 10 * 1024 * 1024)
        .await
        .map_err(|e| ApiError::ValidationError(format!("Failed to read body: {}", e)))?;
    let mut request: AnthropicMessagesRequest = serde_json::from_slice(&body_bytes)
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

    // ── Provider routing ─────────────────────────────────────────────
    let requested_model = request.model.clone();
    validate_model_provider(&requested_model)?;
    let mut routing = resolve_provider_routing(&state, user_creds.as_ref(), &requested_model).await;
    validate_model_visibility(
        &state.model_cache,
        &routing.provider_id,
        &requested_model,
        routing.stripped_model.as_deref(),
    )?;

    let mut creds = if routing.provider_id == ProviderId::Kiro {
        build_kiro_credentials(&state, user_creds.as_ref()).await?
    } else {
        routing.provider_creds.clone().ok_or_else(|| {
            ApiError::AuthError(format!(
                "No credentials available for provider {:?}",
                routing.provider_id
            ))
        })?
    };

    // Strip provider prefix from model name if present
    if let Some(ref model_id) = routing.stripped_model {
        request.model = model_id.clone();
    }

    let mut provider = state.providers.get(&routing.provider_id).ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!(
            "Provider {:?} not registered",
            routing.provider_id
        ))
    })?;

    tracing::info!(
        model = %request.model,
        provider = ?routing.provider_id,
        stream = request.stream,
        "Routing request (Anthropic endpoint)"
    );

    // ── Pre-provider pipeline stages ─────────────────────────────────
    let config = read_config(&state);

    // Truncation recovery (Kiro-specific, but harmless for others)
    if routing.provider_id == ProviderId::Kiro && config.truncation_recovery {
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
        request.messages = msg_values
            .into_iter()
            .map(|v| crate::models::anthropic::AnthropicMessage {
                role: v["role"].as_str().unwrap_or("user").to_string(),
                content: v["content"].clone(),
            })
            .collect();
    }

    // Input guardrails
    if config.guardrails_enabled {
        if let Some(ref engine) = state.guardrails_engine {
            let user_content = extract_last_user_message_anthropic(&request.messages);
            let ctx = build_request_context_anthropic(&request);
            run_input_guardrail_check(engine, &user_content, &ctx).await?;
        }
    }

    // ── Provider dispatch with failover ──────────────────────────────
    const MAX_ATTEMPTS: usize = 3;

    if request.stream {
        let mut last_error = None;
        for attempt in 0..MAX_ATTEMPTS {
            let ctx = ProviderContext {
                credentials: &creds,
                model: &request.model,
            };

            match provider.stream_anthropic(&ctx, &request).await {
                Ok(stream_resp) => {
                    update_rate_limits(
                        &state.rate_tracker,
                        &routing.account_id,
                        &routing.provider_id,
                        &stream_resp.headers,
                    );

                    let tracked_stream = wrap_stream_with_usage_metrics(
                        stream_resp.stream,
                        state.config_db.clone(),
                        user_creds.as_ref().map(|c| c.user_id),
                        routing.provider_id.clone(),
                        request.model.clone(),
                    );

                    let byte_stream =
                        tracked_stream.map(|r| r.map_err(|e| std::io::Error::other(e.to_string())));

                    let response = Response::builder()
                        .status(200)
                        .header("Content-Type", "text/event-stream")
                        .header("Cache-Control", "no-cache")
                        .header("Connection", "keep-alive")
                        .body(Body::from_stream(byte_stream))
                        .map_err(|e| {
                            ApiError::Internal(anyhow::anyhow!("Failed to build response: {}", e))
                        })?;

                    DEBUG_LOGGER.discard_buffers().await;
                    return Ok(response);
                }
                Err(ApiError::ProviderApiError {
                    status: 429,
                    ref headers,
                    ..
                }) => {
                    if !handle_rate_limit_retry(
                        &state,
                        user_creds.as_ref(),
                        &mut routing,
                        &mut creds,
                        &mut provider,
                        attempt,
                        MAX_ATTEMPTS,
                        headers,
                    )
                    .await?
                    {
                        break;
                    }
                }
                Err(e) => {
                    last_error = Some(e);
                    break;
                }
            }
        }
        return Err(last_error.unwrap_or_else(|| ApiError::RateLimited {
            provider: routing.provider_id.as_str().to_string(),
            retry_after_secs: 60,
        }));
    } else {
        let mut last_error = None;
        for attempt in 0..MAX_ATTEMPTS {
            let ctx = ProviderContext {
                credentials: &creds,
                model: &request.model,
            };

            match provider.execute_anthropic(&ctx, &request).await {
                Ok(resp) => {
                    update_rate_limits(
                        &state.rate_tracker,
                        &routing.account_id,
                        &routing.provider_id,
                        &resp.headers,
                    );

                    let body = provider.normalize_response_for_anthropic(&request.model, resp.body);

                    // Output guardrails (non-streaming only)
                    if config.guardrails_enabled {
                        if let Some(ref engine) = state.guardrails_engine {
                            let output_text = extract_assistant_content_anthropic(&body);
                            let ctx = build_request_context_anthropic(&request);
                            run_output_guardrail_check(engine, &output_text, &ctx).await?;
                        }
                    }

                    persist_non_streaming_usage(
                        extract_usage_metric_snapshot(&body),
                        &state.config_db,
                        user_creds.as_ref().map(|c| c.user_id),
                        &routing.provider_id,
                        &request.model,
                    );

                    DEBUG_LOGGER.discard_buffers().await;
                    return Ok(Json(body).into_response());
                }
                Err(ApiError::ProviderApiError {
                    status: 429,
                    ref headers,
                    ..
                }) => {
                    if !handle_rate_limit_retry(
                        &state,
                        user_creds.as_ref(),
                        &mut routing,
                        &mut creds,
                        &mut provider,
                        attempt,
                        MAX_ATTEMPTS,
                        headers,
                    )
                    .await?
                    {
                        break;
                    }
                }
                Err(e) => {
                    last_error = Some(e);
                    break;
                }
            }
        }
        Err(last_error.unwrap_or_else(|| ApiError::RateLimited {
            provider: routing.provider_id.as_str().to_string(),
            retry_after_secs: 60,
        }))
    }
}
