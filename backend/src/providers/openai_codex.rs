/// OpenAICodexProvider — direct calls to api.openai.com/v1/chat/completions.
///
/// Handles both OpenAI-format requests (pass-through) and Anthropic-format requests
/// (converted to OpenAI format before forwarding).
use std::pin::Pin;

use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::models::anthropic::AnthropicMessagesRequest;
use crate::models::openai::ChatCompletionRequest;
use crate::providers::traits::Provider;
use crate::providers::types::{ProviderContext, ProviderId, ProviderResponse, ProviderStreamItem};
use crate::streaming::sse::parse_sse_stream;

const OPENAI_API_BASE: &str = "https://api.openai.com";

pub struct OpenAICodexProvider {
    client: reqwest::Client,
}

impl OpenAICodexProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
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

    /// Convert Anthropic messages format to OpenAI chat completions format.
    fn anthropic_to_openai_body(req: &AnthropicMessagesRequest) -> Value {
        let mut messages: Vec<Value> = Vec::new();

        // Add system prompt if present
        if let Some(system) = &req.system {
            let system_text = system
                .as_str()
                .map(String::from)
                .or_else(|| {
                    system.as_array().map(|blocks| {
                        blocks
                            .iter()
                            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                })
                .unwrap_or_default();
            if !system_text.is_empty() {
                messages.push(json!({ "role": "system", "content": system_text }));
            }
        }

        // Add conversation messages
        for msg in &req.messages {
            let content = msg
                .content
                .as_str()
                .map(|s| json!(s))
                .unwrap_or_else(|| msg.content.clone());
            messages.push(json!({ "role": msg.role, "content": content }));
        }

        let mut body = json!({
            "model": req.model,
            "messages": messages,
            "stream": false,
        });

        if req.max_tokens > 0 {
            body["max_tokens"] = json!(req.max_tokens);
        }
        if let Some(temp) = req.temperature {
            body["temperature"] = json!(temp);
        }

        body
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
            let error_text = response.text().await.unwrap_or_default();
            return Err(ApiError::ProviderApiError {
                provider: "openai_codex".to_string(),
                status,
                message: error_text,
            });
        }

        Ok(response)
    }
}

impl Default for OpenAICodexProvider {
    fn default() -> Self {
        Self::new()
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
        let body: Value = response.json().await.map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("Failed to parse OpenAI response: {}", e))
        })?;
        Ok(ProviderResponse { status, body })
    }

    async fn stream_openai(
        &self,
        ctx: &ProviderContext<'_>,
        req: &ChatCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>>, ApiError> {
        let body = serde_json::to_value(req)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Serialization failed: {}", e)))?;
        let response = self.send_request(ctx, body, true).await?;
        // Pass raw SSE bytes from OpenAI directly to the client
        let stream = response.bytes_stream().map(|chunk| {
            chunk.map_err(|e| ApiError::Internal(anyhow::anyhow!("Stream error: {}", e)))
        });
        Ok(Box::pin(stream))
    }

    async fn execute_anthropic(
        &self,
        ctx: &ProviderContext<'_>,
        req: &AnthropicMessagesRequest,
    ) -> Result<ProviderResponse, ApiError> {
        let body = Self::anthropic_to_openai_body(req);
        let response = self.send_request(ctx, body, false).await?;
        let status = response.status().as_u16();
        let body: Value = response.json().await.map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("Failed to parse OpenAI response: {}", e))
        })?;
        Ok(ProviderResponse { status, body })
    }

    async fn stream_anthropic(
        &self,
        ctx: &ProviderContext<'_>,
        req: &AnthropicMessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>>, ApiError> {
        let body = Self::anthropic_to_openai_body(req);
        let response = self.send_request(ctx, body, true).await?;
        let byte_stream = response.bytes_stream();
        let sse = parse_sse_stream(byte_stream).map(|item| match item {
            Ok(v) => {
                let line = format!("data: {}\n\n", v);
                Ok(Bytes::from(line))
            }
            Err(e) => Err(e),
        });
        Ok(Box::pin(sse))
    }

    /// OpenAI responses need conversion when served through the Anthropic endpoint.
    fn normalize_response_for_anthropic(&self, model: &str, body: Value) -> Value {
        openai_response_to_anthropic(model, &body)
    }
}

/// Convert an OpenAI API non-streaming response body → Anthropic messages response JSON.
///
/// Public so that other OpenAI-compatible providers (Copilot, Qwen) can reuse this.
pub fn openai_response_to_anthropic(model: &str, body: &Value) -> Value {
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

    serde_json::json!({
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::anthropic::{AnthropicMessage, AnthropicMessagesRequest};
    use crate::providers::types::ProviderCredentials;

    #[test]
    fn test_openai_codex_provider_id() {
        assert_eq!(OpenAICodexProvider::new().id(), ProviderId::OpenAICodex);
    }

    #[test]
    fn test_completions_url_default() {
        let provider = OpenAICodexProvider::new();
        let creds = ProviderCredentials {
            provider: ProviderId::OpenAICodex,
            access_token: "sk-test".to_string(),
            base_url: None,
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
        let provider = OpenAICodexProvider::new();
        let creds = ProviderCredentials {
            provider: ProviderId::OpenAICodex,
            access_token: "sk-test".to_string(),
            base_url: Some("https://openrouter.ai/api".to_string()),
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

        let body = OpenAICodexProvider::anthropic_to_openai_body(&req);
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

        let body = OpenAICodexProvider::anthropic_to_openai_body(&req);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "Be helpful");
        assert_eq!(body["messages"][1]["role"], "user");
    }
}
