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
    extract_last_user_message_anthropic, parse_retry_after, read_config, resolve_provider_routing,
    run_input_guardrail_check, run_output_guardrail_check, update_rate_limits,
    validate_model_provider,
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
    validate_model_provider(&request.model)?;
    let mut routing = resolve_provider_routing(&state, user_creds.as_ref(), &request.model).await;

    let mut creds = if routing.provider_id == ProviderId::Kiro {
        build_kiro_credentials(&state, user_creds.as_ref()).await?
    } else {
        routing.provider_creds.clone().unwrap()
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
                    // Update rate limits from streaming response headers
                    update_rate_limits(
                        &state.rate_tracker,
                        &routing.account_id,
                        &routing.provider_id,
                        &stream_resp.headers,
                    );

                    let byte_stream = stream_resp
                        .stream
                        .map(|r| r.map_err(|e| std::io::Error::other(e.to_string())));

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
                }) if attempt < MAX_ATTEMPTS - 1 => {
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
                    routing =
                        resolve_provider_routing(&state, user_creds.as_ref(), &request.model).await;
                    if let Some(ref new_creds) = routing.provider_creds {
                        creds = new_creds.clone();
                    } else {
                        break;
                    }
                    provider = state.providers.get(&routing.provider_id).ok_or_else(|| {
                        ApiError::Internal(anyhow::anyhow!(
                            "Provider {:?} not registered",
                            routing.provider_id
                        ))
                    })?;
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
                    // Update rate limits from response headers
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

                    // Usage tracking
                    if let (Some(config_db), Some(user_creds)) =
                        (state.config_db.as_ref(), user_creds.as_ref())
                    {
                        if let Some(usage) = body.get("usage") {
                            let input_tokens = usage["input_tokens"].as_i64().unwrap_or(0) as i32;
                            let output_tokens = usage["output_tokens"].as_i64().unwrap_or(0) as i32;
                            let cost = crate::cost::calculate_cost(
                                &request.model,
                                input_tokens as i64,
                                output_tokens as i64,
                            );
                            let db = config_db.clone();
                            let user_id = user_creds.user_id;
                            let provider_str = routing.provider_id.to_string();
                            let model = request.model.clone();
                            tokio::spawn(async move {
                                if let Err(e) = db
                                    .insert_usage_record(
                                        user_id,
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
                    }

                    DEBUG_LOGGER.discard_buffers().await;
                    return Ok(Json(body).into_response());
                }
                Err(ApiError::ProviderApiError {
                    status: 429,
                    ref headers,
                    ..
                }) if attempt < MAX_ATTEMPTS - 1 => {
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
                    routing =
                        resolve_provider_routing(&state, user_creds.as_ref(), &request.model).await;
                    if let Some(ref new_creds) = routing.provider_creds {
                        creds = new_creds.clone();
                    } else {
                        break;
                    }
                    provider = state.providers.get(&routing.provider_id).ok_or_else(|| {
                        ApiError::Internal(anyhow::anyhow!(
                            "Provider {:?} not registered",
                            routing.provider_id
                        ))
                    })?;
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
