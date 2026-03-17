use axum::{
    body::Body,
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use futures::stream::StreamExt;

use crate::error::ApiError;
use crate::middleware::DEBUG_LOGGER;
use crate::models::openai::{ChatCompletionRequest, ModelList, OpenAIModel};
use crate::providers::types::{ProviderContext, ProviderId};

use super::pipeline::{
    build_kiro_credentials, build_request_context_openai, extract_assistant_content,
    extract_last_user_message, read_config, resolve_provider_routing, run_input_guardrail_check,
    run_output_guardrail_check, update_rate_limits,
};
use super::state::{AppState, UserKiroCreds};

/// GET /v1/models - List available models
///
/// Returns a list of available models in OpenAI format.
/// Merges Kiro API models (legacy cache) with registry-backed models.
pub(crate) async fn get_models_handler(
    State(state): State<AppState>,
) -> Result<Json<ModelList>, ApiError> {
    tracing::info!("Request to /v1/models");

    // 1. Kiro models from legacy cache
    let kiro_ids = state.model_cache.get_all_model_ids();
    let mut models: Vec<OpenAIModel> = kiro_ids
        .into_iter()
        .map(|id| {
            let mut model = OpenAIModel::new(id);
            model.description = Some("Claude model via Kiro API".to_string());
            model
        })
        .collect();

    // 2. Registry models (direct providers)
    let registry_models = state.model_cache.get_all_registry_models();
    let kiro_set: std::collections::HashSet<String> = models.iter().map(|m| m.id.clone()).collect();

    for rm in registry_models {
        if kiro_set.contains(&rm.prefixed_id) {
            continue;
        }
        models.push(OpenAIModel {
            id: rm.prefixed_id,
            object: "model".to_string(),
            created: rm.created_at.timestamp(),
            owned_by: rm.provider_id,
            description: Some(rm.display_name),
        });
    }

    Ok(Json(ModelList::new(models)))
}

/// POST /v1/chat/completions - Create chat completion
///
/// Handles both streaming and non-streaming chat completion requests.
/// All providers (including Kiro) flow through the Provider trait.
#[tracing::instrument(skip_all, name = "chat_completions")]
pub(crate) async fn chat_completions_handler(
    State(state): State<AppState>,
    raw_request: axum::http::Request<Body>,
) -> Result<Response, ApiError> {
    let user_creds = raw_request.extensions().get::<UserKiroCreds>().cloned();

    // Parse JSON body
    let body_bytes = axum::body::to_bytes(raw_request.into_body(), 10 * 1024 * 1024)
        .await
        .map_err(|e| ApiError::ValidationError(format!("Failed to read body: {}", e)))?;
    let mut request: ChatCompletionRequest = serde_json::from_slice(&body_bytes)
        .map_err(|e| ApiError::ValidationError(format!("Invalid JSON: {}", e)))?;

    tracing::info!(
        model = %request.model,
        stream = request.stream,
        messages = request.messages.len(),
        "Request to /v1/chat/completions"
    );

    if request.messages.is_empty() {
        return Err(ApiError::ValidationError(
            "messages cannot be empty".to_string(),
        ));
    }

    // ── Provider routing ─────────────────────────────────────────────
    let mut routing = resolve_provider_routing(&state, user_creds.as_ref(), &request.model).await;

    // Build credentials: for Kiro, derive from user creds / global auth;
    // for direct providers, use the credentials from the registry.
    let mut creds = if routing.provider_id == ProviderId::Kiro {
        build_kiro_credentials(&state, user_creds.as_ref()).await?
    } else {
        routing.provider_creds.clone().unwrap()
    };

    // Strip provider prefix from model name if present
    if let Some(ref model_id) = routing.stripped_model {
        request.model = model_id.clone();
    }

    let provider = state.providers.get(&routing.provider_id).ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!(
            "Provider {:?} not registered",
            routing.provider_id
        ))
    })?;

    tracing::info!(
        model = %request.model,
        provider = ?routing.provider_id,
        stream = request.stream,
        "Routing request (OpenAI endpoint)"
    );

    // ── Pre-provider pipeline stages ─────────────────────────────────
    let config = read_config(&state);

    // Truncation recovery (Kiro-specific, but harmless for others)
    if routing.provider_id == ProviderId::Kiro && config.truncation_recovery {
        let mut msg_values: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter_map(|m| serde_json::to_value(m).ok())
            .collect();
        crate::truncation::inject_openai_truncation_recovery(&mut msg_values);
        request.messages = msg_values
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();
    }

    // Input guardrails
    if config.guardrails_enabled {
        if let Some(ref engine) = state.guardrails_engine {
            let user_content = extract_last_user_message(&request.messages);
            let ctx = build_request_context_openai(&request);
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

            match provider.stream_openai(&ctx, &request).await {
                Ok(stream) => {
                    let byte_stream =
                        stream.map(|r| r.map_err(|e| std::io::Error::other(e.to_string())));

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
                Err(ApiError::ProviderApiError { status: 429, .. })
                    if attempt < MAX_ATTEMPTS - 1 =>
                {
                    // Rate limited — mark account and retry with different one
                    if let Some(ref aid) = routing.account_id {
                        tracing::info!(
                            attempt,
                            account_label = %aid.account_label,
                            "Rate limited, retrying with different account"
                        );
                        state.rate_tracker.mark_limited(aid, None);
                    }
                    // Re-resolve to get a different account
                    routing =
                        resolve_provider_routing(&state, user_creds.as_ref(), &request.model).await;
                    if let Some(ref new_creds) = routing.provider_creds {
                        creds = new_creds.clone();
                    } else {
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

            match provider.execute_openai(&ctx, &request).await {
                Ok(resp) => {
                    // Update rate limits from response headers
                    update_rate_limits(
                        &state.rate_tracker,
                        &routing.account_id,
                        &routing.provider_id,
                        &resp.headers,
                    );

                    let body = provider.normalize_response_for_openai(&request.model, resp.body);

                    // Output guardrails (non-streaming only)
                    if config.guardrails_enabled {
                        if let Some(ref engine) = state.guardrails_engine {
                            let output_text = extract_assistant_content(&body);
                            let ctx = build_request_context_openai(&request);
                            run_output_guardrail_check(engine, &output_text, &ctx).await?;
                        }
                    }

                    // Usage tracking (non-streaming only)
                    if let (Some(config_db), Some(user_creds)) =
                        (state.config_db.as_ref(), user_creds.as_ref())
                    {
                        if let Some(usage) = body.get("usage") {
                            let input_tokens = usage["prompt_tokens"].as_i64().unwrap_or(0) as i32;
                            let output_tokens =
                                usage["completion_tokens"].as_i64().unwrap_or(0) as i32;
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
                Err(ApiError::ProviderApiError { status: 429, .. })
                    if attempt < MAX_ATTEMPTS - 1 =>
                {
                    if let Some(ref aid) = routing.account_id {
                        tracing::info!(
                            attempt,
                            account_label = %aid.account_label,
                            "Rate limited, retrying with different account"
                        );
                        state.rate_tracker.mark_limited(aid, None);
                    }
                    routing =
                        resolve_provider_routing(&state, user_creds.as_ref(), &request.model).await;
                    if let Some(ref new_creds) = routing.provider_creds {
                        creds = new_creds.clone();
                    } else {
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
