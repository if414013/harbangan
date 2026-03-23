/// OpenAICodexProvider — direct calls to api.openai.com/v1/chat/completions.
///
/// Handles both OpenAI-format requests (pass-through) and Anthropic-format requests
/// (converted to OpenAI format before forwarding).
use async_trait::async_trait;
use futures::stream::StreamExt;
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::models::anthropic::AnthropicMessagesRequest;
use crate::models::openai::ChatCompletionRequest;
use crate::providers::anthropic_to_openai_body;
use crate::providers::traits::Provider;
use crate::providers::types::{
    ProviderContext, ProviderId, ProviderResponse, ProviderStreamResponse,
};
use crate::streaming::sse::parse_sse_stream;

const OPENAI_API_BASE: &str = "https://api.openai.com";

pub struct OpenAICodexProvider {
    client: reqwest::Client,
}

impl OpenAICodexProvider {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    fn base_url<'a>(&self, ctx: &ProviderContext<'a>) -> &'a str {
        ctx.credentials
            .base_url
            .as_deref()
            .unwrap_or(OPENAI_API_BASE)
    }

    fn completions_url(&self, ctx: &ProviderContext<'_>) -> String {
        format!("{}/v1/chat/completions", self.base_url(ctx))
    }

    async fn send_request(
        &self,
        ctx: &ProviderContext<'_>,
        mut body: Value,
        stream: bool,
    ) -> Result<reqwest::Response, ApiError> {
        let url = self.completions_url(ctx);
        body["stream"] = json!(stream);

        let response = self
            .client
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", ctx.credentials.access_token),
            )
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("OpenAI request failed: {}", e)))?;

        let status = response.status().as_u16();
        if !response.status().is_success() {
            let resp_headers = response.headers().clone();
            let error_text = response.text().await.unwrap_or_default();
            return Err(ApiError::ProviderApiError {
                provider: "openai_codex".to_string(),
                status,
                message: error_text,
                headers: Some(resp_headers),
            });
        }

        Ok(response)
    }
}

impl Default for OpenAICodexProvider {
    fn default() -> Self {
        Self::new(reqwest::Client::new())
    }
}

#[async_trait]
impl Provider for OpenAICodexProvider {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn id(&self) -> ProviderId {
        ProviderId::OpenAICodex
    }

    async fn execute_openai(
        &self,
        ctx: &ProviderContext<'_>,
        req: &ChatCompletionRequest,
    ) -> Result<ProviderResponse, ApiError> {
        let body = serde_json::to_value(req)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Serialization failed: {}", e)))?;
        let response = self.send_request(ctx, body, false).await?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body: Value = response.json().await.map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("Failed to parse OpenAI response: {}", e))
        })?;
        Ok(ProviderResponse {
            status,
            body,
            headers,
        })
    }

    async fn stream_openai(
        &self,
        ctx: &ProviderContext<'_>,
        req: &ChatCompletionRequest,
    ) -> Result<ProviderStreamResponse, ApiError> {
        let body = serde_json::to_value(req)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Serialization failed: {}", e)))?;
        let response = self.send_request(ctx, body, true).await?;
        let headers = response.headers().clone();
        let stream = response.bytes_stream().map(|chunk| {
            chunk.map_err(|e| ApiError::Internal(anyhow::anyhow!("Stream error: {}", e)))
        });
        Ok(ProviderStreamResponse {
            headers,
            stream: Box::pin(stream),
        })
    }

    async fn execute_anthropic(
        &self,
        ctx: &ProviderContext<'_>,
        req: &AnthropicMessagesRequest,
    ) -> Result<ProviderResponse, ApiError> {
        let body = anthropic_to_openai_body(req);
        let response = self.send_request(ctx, body, false).await?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body: Value = response.json().await.map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("Failed to parse OpenAI response: {}", e))
        })?;
        Ok(ProviderResponse {
            status,
            body,
            headers,
        })
    }

    async fn stream_anthropic(
        &self,
        ctx: &ProviderContext<'_>,
        req: &AnthropicMessagesRequest,
    ) -> Result<ProviderStreamResponse, ApiError> {
        let body = anthropic_to_openai_body(req);
        let response = self.send_request(ctx, body, true).await?;
        let headers = response.headers().clone();
        let byte_stream = response.bytes_stream();
        let sse_values = parse_sse_stream(byte_stream);
        let stream =
            crate::streaming::cross_format::wrap_openai_stream_as_anthropic(sse_values, &req.model);
        Ok(ProviderStreamResponse { headers, stream })
    }

    /// OpenAI responses need conversion when served through the Anthropic endpoint.
    fn normalize_response_for_anthropic(&self, model: &str, body: Value) -> Value {
        openai_response_to_anthropic(model, &body)
    }
}

/// Convert an OpenAI API non-streaming response body → Anthropic messages response JSON.
///
/// Public so that other OpenAI-compatible providers (Copilot, Custom) can reuse this.
/// Maps tool_calls to Anthropic tool_use content blocks and finish_reason "tool_calls"
/// to stop_reason "tool_use".
pub fn openai_response_to_anthropic(model: &str, body: &Value) -> Value {
    let message = body
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"));

    let text = message
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();

    let finish_reason = body
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("finish_reason"))
        .and_then(|r| r.as_str())
        .unwrap_or("stop");

    let stop_reason = match finish_reason {
        "stop" => "end_turn".to_string(),
        "tool_calls" => "tool_use".to_string(),
        other => other.to_string(),
    };

    // Build content blocks: text + tool_use
    let mut content_blocks: Vec<Value> = Vec::new();
    if !text.is_empty() {
        content_blocks.push(json!({"type": "text", "text": text}));
    }
    if let Some(tool_calls) = message
        .and_then(|m| m.get("tool_calls"))
        .and_then(|t| t.as_array())
    {
        for tc in tool_calls {
            let id = tc
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let arguments = tc
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(|a| a.as_str())
                .unwrap_or("{}");
            let input: Value = serde_json::from_str(arguments).unwrap_or(json!({}));
            content_blocks.push(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input,
            }));
        }
    }
    // Ensure at least one content block
    if content_blocks.is_empty() {
        content_blocks.push(json!({"type": "text", "text": ""}));
    }

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

    serde_json::json!({
        "id": body.get("id").and_then(|v| v.as_str()).unwrap_or("msg-direct"),
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content_blocks,
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": { "input_tokens": input_tokens, "output_tokens": output_tokens }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::anthropic::{AnthropicMessage, AnthropicMessagesRequest};
    use crate::providers::anthropic_to_openai_body;
    use crate::providers::types::ProviderCredentials;

    #[test]
    fn test_openai_codex_provider_id() {
        assert_eq!(OpenAICodexProvider::default().id(), ProviderId::OpenAICodex);
    }

    #[test]
    fn test_completions_url_default() {
        let provider = OpenAICodexProvider::default();
        let creds = ProviderCredentials {
            provider: ProviderId::OpenAICodex,
            access_token: "sk-test".to_string(),
            base_url: None,
            account_label: "default".to_string(),
        };
        let model = "gpt-4o".to_string();
        let ctx = ProviderContext {
            credentials: &creds,
            model: &model,
        };
        assert_eq!(
            provider.completions_url(&ctx),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_completions_url_custom_base() {
        let provider = OpenAICodexProvider::default();
        let creds = ProviderCredentials {
            provider: ProviderId::OpenAICodex,
            access_token: "sk-test".to_string(),
            base_url: Some("https://openrouter.ai/api".to_string()),
            account_label: "default".to_string(),
        };
        let model = "gpt-4o".to_string();
        let ctx = ProviderContext {
            credentials: &creds,
            model: &model,
        };
        assert_eq!(
            provider.completions_url(&ctx),
            "https://openrouter.ai/api/v1/chat/completions"
        );
    }

    #[test]
    fn test_anthropic_to_openai_body_basic() {
        let req = AnthropicMessagesRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: json!("Hello"),
            }],
            max_tokens: 1000,
            system: None,
            stream: false,
            tools: None,
            tool_choice: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            metadata: None,
            thinking: None,
            disable_parallel_tool_use: None,
        };

        let body = anthropic_to_openai_body(&req);
        assert_eq!(body["model"], "claude-sonnet-4");
        assert_eq!(body["max_tokens"], 1000);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "Hello");
    }

    #[test]
    fn test_anthropic_to_openai_body_with_system() {
        let req = AnthropicMessagesRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: json!("Hi"),
            }],
            max_tokens: 100,
            system: Some(json!("Be helpful")),
            stream: false,
            tools: None,
            tool_choice: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            metadata: None,
            thinking: None,
            disable_parallel_tool_use: None,
        };

        let body = anthropic_to_openai_body(&req);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "Be helpful");
        assert_eq!(body["messages"][1]["role"], "user");
    }
}
