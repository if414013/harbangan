//! Antigravity route handlers for Cloud Code backend.
//!
//! Provides handler functions for routing OpenAI and Anthropic requests
//! through the Cloud Code API with multi-account load balancing.

use axum::body::Body;
use axum::response::Response;
use serde_json::Value;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use super::account_manager::AccountManager;
use super::constants::DEFAULT_PROJECT_ID;
use super::converters::anthropic_to_google::convert_anthropic_to_google;
use super::http_client::{CloudCodeClient, CloudCodeError};
use super::request_builder::{build_cloud_code_request, build_headers};
use super::session::SessionManager;
use super::streaming::StreamingParser;
use crate::error::ApiError;
use crate::models::anthropic::AnthropicMessagesRequest;
use crate::models::openai::ChatCompletionRequest;

/// Maximum number of account retries on auth/rate-limit errors.
const MAX_ACCOUNT_RETRIES: usize = 3;

/// Handles an OpenAI chat completions request via the Cloud Code backend.
///
/// Selects an account, converts to Google GenAI format, sends through
/// Cloud Code, and streams back as OpenAI SSE.
pub async fn antigravity_chat_completions(
    account_manager: &AccountManager,
    http_client: &CloudCodeClient,
    session_manager: &Mutex<SessionManager>,
    request: &ChatCompletionRequest,
) -> Result<Response, ApiError> {
    let model = &request.model;
    info!(model, "Antigravity chat completions request");

    // Convert OpenAI request to Anthropic intermediate, then to Google
    // For now, build a minimal Anthropic request from the OpenAI one
    let anthropic_req = openai_to_anthropic_request(request);
    let google_request = convert_anthropic_to_google(&anthropic_req);

    // Send with account retry loop
    let response = send_with_account_retry(
        account_manager,
        http_client,
        session_manager,
        model,
        google_request,
    )
    .await?;

    // Parse SSE and convert to OpenAI streaming format
    let http_response_bytes = response
        .bytes()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to read response: {}", e)))?;
    let raw = String::from_utf8_lossy(&http_response_bytes);
    let mut parser = StreamingParser::new(model);
    let mut events: Vec<String> = Vec::new();
    for data in super::streaming::parse_sse_lines(&raw) {
        for event in parser.process_sse_data(&data) {
            events.push(format!("data: {}\n\n", event));
        }
    }
    for event in parser.finish() {
        events.push(format!("data: {}\n\n", event));
    }
    let body = events.join("");
    Response::builder()
        .status(200)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(Body::from(body))
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to build response: {}", e)))
}

/// Handles an Anthropic messages request via the Cloud Code backend.
///
/// Selects an account, converts to Google GenAI format, sends through
/// Cloud Code, and streams back as Anthropic SSE.
pub async fn antigravity_anthropic_messages(
    account_manager: &AccountManager,
    http_client: &CloudCodeClient,
    session_manager: &Mutex<SessionManager>,
    request: &AnthropicMessagesRequest,
) -> Result<Response, ApiError> {
    let model = &request.model;
    info!(model, "Antigravity anthropic messages request");

    let google_request = convert_anthropic_to_google(request);

    // Send with account retry loop
    let response = send_with_account_retry(
        account_manager,
        http_client,
        session_manager,
        model,
        google_request,
    )
    .await?;

    // Parse SSE and convert to Anthropic streaming format
    let http_response_bytes = response
        .bytes()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to read response: {}", e)))?;
    let raw = String::from_utf8_lossy(&http_response_bytes);
    let mut parser = StreamingParser::new(model);
    let mut events: Vec<String> = Vec::new();
    for data in super::streaming::parse_sse_lines(&raw) {
        for event in parser.process_sse_data(&data) {
            events.push(format!("data: {}\n\n", event));
        }
    }
    for event in parser.finish() {
        events.push(format!("data: {}\n\n", event));
    }
    let body = events.join("");
    Response::builder()
        .status(200)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(Body::from(body))
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to build response: {}", e)))
}

// === Internal Helpers ===

/// Sends a request through Cloud Code with account selection and retry.
///
/// On 401: marks failure, tries next account with fresh token.
/// On 429: marks account rate-limited, selects next account.
/// On 400: returns error immediately.
/// On other errors: marks failure, tries next account.
async fn send_with_account_retry(
    account_manager: &AccountManager,
    http_client: &CloudCodeClient,
    session_manager: &Mutex<SessionManager>,
    model: &str,
    google_request: Value,
) -> Result<reqwest::Response, ApiError> {
    let mut last_error = None;

    for attempt in 0..MAX_ACCOUNT_RETRIES {
        // Select an account
        let (email, wait_ms) = account_manager
            .select_account(model)
            .await
            .map_err(ApiError::Internal)?;

        if wait_ms > 0 {
            debug!(email = %email, wait_ms, "Waiting before request");
            tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
        }

        // Get token and project
        let token = account_manager
            .get_token_for_account(&email)
            .await
            .map_err(|e| ApiError::AuthError(format!("Token fetch failed: {}", e)))?;

        let project_id = account_manager
            .get_project_for_account(&email)
            .unwrap_or_else(|| DEFAULT_PROJECT_ID.to_string());

        // Get session ID
        let session_id = {
            let mut mgr = session_manager.lock().await;
            mgr.get_or_create(&email).to_string()
        };

        // Build envelope and headers
        let envelope = build_cloud_code_request(&project_id, model, google_request.clone());
        let headers = build_headers(&token, &session_id, model);

        debug!(
            email = %email,
            attempt = attempt + 1,
            "Sending Cloud Code request"
        );

        match http_client.send_streaming_request(headers, envelope).await {
            Ok(response) => {
                account_manager.notify_success(&email, model);
                return Ok(response);
            }
            Err(CloudCodeError::Unauthorized(msg)) => {
                warn!(email = %email, "401 from Cloud Code, trying next account");
                account_manager.notify_failure(&email, model);
                last_error = Some(ApiError::AuthError(msg));
            }
            Err(CloudCodeError::RateLimited) => {
                warn!(email = %email, model, "429 from Cloud Code, trying next account");
                account_manager.notify_rate_limit(&email, model);
                last_error = Some(ApiError::KiroApiError {
                    status: 429,
                    message: "Rate limited on all accounts".to_string(),
                });
            }
            Err(CloudCodeError::BadRequest(msg)) => {
                error!(email = %email, body = %msg, "400 from Cloud Code");
                return Err(ApiError::ValidationError(msg));
            }
            Err(e) => {
                warn!(email = %email, error = %e, "Cloud Code request failed");
                account_manager.notify_failure(&email, model);
                last_error = Some(ApiError::Internal(anyhow::anyhow!("{}", e)));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        ApiError::Internal(anyhow::anyhow!(
            "All account retries exhausted for model '{}'",
            model
        ))
    }))
}

/// Converts an OpenAI ChatCompletionRequest to an AnthropicMessagesRequest.
///
/// Lightweight shim so we can reuse the Anthropic->Google converter.
fn openai_to_anthropic_request(request: &ChatCompletionRequest) -> AnthropicMessagesRequest {
    use crate::models::anthropic::AnthropicMessage;

    let mut system = None;
    let mut messages = Vec::new();

    for msg in &request.messages {
        let role = msg.role.as_str();
        let content = msg.content.clone().unwrap_or(serde_json::json!(""));

        if role == "system" {
            system = Some(content);
        } else {
            messages.push(AnthropicMessage {
                role: role.to_string(),
                content,
            });
        }
    }

    AnthropicMessagesRequest {
        model: request.model.clone(),
        messages,
        max_tokens: request.max_tokens.unwrap_or(8192),
        system,
        stream: request.stream,
        tools: None,
        tool_choice: None,
        temperature: request.temperature,
        top_p: request.top_p,
        top_k: None,
        stop_sequences: request.stop.clone().map(|s| match s {
            serde_json::Value::String(s) => vec![s],
            serde_json::Value::Array(arr) => arr
                .into_iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => vec![],
        }),
        metadata: None,
    }
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::openai::ChatMessage;

    fn make_request(model: &str, messages: Vec<ChatMessage>) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: model.to_string(),
            messages,
            stream: true,
            temperature: None,
            top_p: None,
            n: None,
            max_tokens: None,
            max_completion_tokens: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
            tools: None,
            tool_choice: None,
            stream_options: None,
            logit_bias: None,
            logprobs: None,
            top_logprobs: None,
            user: None,
            seed: None,
            parallel_tool_calls: None,
        }
    }

    #[test]
    fn test_openai_to_anthropic_basic() {
        let mut request = make_request(
            "claude-sonnet-4-5",
            vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: Some(serde_json::json!("You are helpful.")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: Some(serde_json::json!("Hello")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
        );
        request.max_tokens = Some(1024);
        request.temperature = Some(0.7);

        let anthropic = openai_to_anthropic_request(&request);
        assert_eq!(anthropic.model, "claude-sonnet-4-5");
        assert_eq!(anthropic.max_tokens, 1024);
        assert!(anthropic.system.is_some());
        assert_eq!(anthropic.messages.len(), 1);
        assert_eq!(anthropic.messages[0].role, "user");
        assert_eq!(anthropic.temperature, Some(0.7));
    }

    #[test]
    fn test_openai_to_anthropic_no_system() {
        let request = make_request(
            "gemini-3-flash",
            vec![ChatMessage {
                role: "user".to_string(),
                content: Some(serde_json::json!("Hi")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
        );

        let anthropic = openai_to_anthropic_request(&request);
        assert!(anthropic.system.is_none());
        assert_eq!(anthropic.max_tokens, 8192);
        assert_eq!(anthropic.messages.len(), 1);
    }

    #[test]
    fn test_openai_to_anthropic_stop_sequences() {
        let mut request = make_request(
            "claude-sonnet-4-5",
            vec![ChatMessage {
                role: "user".to_string(),
                content: Some(serde_json::json!("Hi")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
        );
        request.stop = Some(serde_json::json!(["END", "STOP"]));

        let anthropic = openai_to_anthropic_request(&request);
        let seqs = anthropic.stop_sequences.unwrap();
        assert_eq!(seqs.len(), 2);
        assert!(seqs.contains(&"END".to_string()));
        assert!(seqs.contains(&"STOP".to_string()));
    }
}
