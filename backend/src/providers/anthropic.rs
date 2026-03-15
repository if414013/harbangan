/// AnthropicProvider — direct calls to api.anthropic.com/v1/messages.
///
/// Handles both Anthropic-format requests (pass-through) and OpenAI-format requests
/// (converted to Anthropic format before forwarding).
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

const ANTHROPIC_API_BASE: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct AnthropicProvider {
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    fn base_url<'a>(&self, ctx: &ProviderContext<'a>) -> &'a str {
        ctx.credentials
            .base_url
            .as_deref()
            .unwrap_or(ANTHROPIC_API_BASE)
    }

    fn messages_url(&self, ctx: &ProviderContext<'_>) -> String {
        format!("{}/v1/messages", self.base_url(ctx))
    }

    /// Convert OpenAI ChatCompletionRequest to Anthropic messages format.
    fn openai_to_anthropic_body(req: &ChatCompletionRequest) -> Value {
        let messages: Vec<Value> = req
            .messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                let content = m
                    .content
                    .as_ref()
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
                json!({ "role": m.role, "content": content })
            })
            .collect();

        let system: Option<String> =
            req.messages
                .iter()
                .find(|m| m.role == "system")
                .and_then(|m| {
                    m.content
                        .as_ref()
                        .and_then(|c| c.as_str())
                        .map(String::from)
                });

        let max_tokens = req.max_tokens.unwrap_or(4096);

        let mut body = json!({
            "model": req.model,
            "messages": messages,
            "max_tokens": max_tokens,
            "stream": false,
        });

        if let Some(sys) = system {
            body["system"] = json!(sys);
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
        let url = self.messages_url(ctx);
        body["stream"] = json!(stream);

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &ctx.credentials.access_token)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Anthropic request failed: {}", e)))?;

        let status = response.status().as_u16();
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(ApiError::ProviderApiError {
                provider: "anthropic".to_string(),
                status,
                message: error_text,
            });
        }

        Ok(response)
    }
}

impl Default for AnthropicProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn id(&self) -> ProviderId {
        ProviderId::Anthropic
    }

    async fn execute_openai(
        &self,
        ctx: &ProviderContext<'_>,
        req: &ChatCompletionRequest,
    ) -> Result<ProviderResponse, ApiError> {
        let body = Self::openai_to_anthropic_body(req);
        let response = self.send_request(ctx, body, false).await?;
        let status = response.status().as_u16();
        let body: Value = response.json().await.map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("Failed to parse Anthropic response: {}", e))
        })?;
        Ok(ProviderResponse { status, body })
    }

    async fn stream_openai(
        &self,
        ctx: &ProviderContext<'_>,
        req: &ChatCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>>, ApiError> {
        let body = Self::openai_to_anthropic_body(req);
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

    async fn execute_anthropic(
        &self,
        ctx: &ProviderContext<'_>,
        req: &AnthropicMessagesRequest,
    ) -> Result<ProviderResponse, ApiError> {
        let body = serde_json::to_value(req)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Serialization failed: {}", e)))?;
        let response = self.send_request(ctx, body, false).await?;
        let status = response.status().as_u16();
        let body: Value = response.json().await.map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("Failed to parse Anthropic response: {}", e))
        })?;
        Ok(ProviderResponse { status, body })
    }

    async fn stream_anthropic(
        &self,
        ctx: &ProviderContext<'_>,
        req: &AnthropicMessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>>, ApiError> {
        let body = serde_json::to_value(req)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Serialization failed: {}", e)))?;
        let response = self.send_request(ctx, body, true).await?;
        // Pass raw SSE bytes from Anthropic directly to the client
        let stream = response.bytes_stream().map(|chunk| {
            chunk.map_err(|e| ApiError::Internal(anyhow::anyhow!("Stream error: {}", e)))
        });
        Ok(Box::pin(stream))
    }

    /// Anthropic responses need conversion when served through the OpenAI endpoint.
    fn normalize_response_for_openai(&self, model: &str, body: Value) -> Value {
        let text = body
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|b| b.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        let finish_reason = body
            .get("stop_reason")
            .and_then(|r| r.as_str())
            .map(|r| if r == "end_turn" { "stop" } else { r })
            .unwrap_or("stop")
            .to_string();

        let prompt_tokens = body
            .get("usage")
            .and_then(|u| u.get("input_tokens"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let completion_tokens = body
            .get("usage")
            .and_then(|u| u.get("output_tokens"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;

        serde_json::json!({
            "id": body.get("id").and_then(|v| v.as_str()).unwrap_or("chatcmpl-direct"),
            "object": "chat.completion",
            "created": chrono::Utc::now().timestamp(),
            "model": model,
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": text },
                "finish_reason": finish_reason,
                "logprobs": null
            }],
            "usage": {
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "total_tokens": prompt_tokens + completion_tokens
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::openai::ChatMessage;
    use crate::providers::types::ProviderCredentials;

    fn make_ctx_parts(token: &str) -> (ProviderCredentials, String) {
        (
            ProviderCredentials {
                provider: ProviderId::Anthropic,
                access_token: token.to_string(),
                base_url: None,
            },
            "claude-sonnet-4".to_string(),
        )
    }

    #[test]
    fn test_anthropic_provider_id() {
        assert_eq!(AnthropicProvider::new().id(), ProviderId::Anthropic);
    }

    #[test]
    fn test_messages_url_default() {
        let provider = AnthropicProvider::new();
        let (creds, model) = make_ctx_parts("sk-ant-test");
        let ctx = ProviderContext {
            credentials: &creds,
            model: &model,
        };
        assert_eq!(
            provider.messages_url(&ctx),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn test_messages_url_custom_base() {
        let provider = AnthropicProvider::new();
        let creds = ProviderCredentials {
            provider: ProviderId::Anthropic,
            access_token: "sk-ant-test".to_string(),
            base_url: Some("https://custom.example.com".to_string()),
        };
        let model = "claude-sonnet-4".to_string();
        let ctx = ProviderContext {
            credentials: &creds,
            model: &model,
        };
        assert_eq!(
            provider.messages_url(&ctx),
            "https://custom.example.com/v1/messages"
        );
    }

    #[test]
    fn test_openai_to_anthropic_body_basic() {
        let req = ChatCompletionRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some(json!("Hello")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            stream: false,
            max_tokens: Some(1000),
            temperature: None,
            top_p: None,
            n: None,
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
            reasoning_effort: None,
            response_format: None,
        };

        let body = AnthropicProvider::openai_to_anthropic_body(&req);
        assert_eq!(body["model"], "claude-sonnet-4");
        assert_eq!(body["max_tokens"], 1000);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "Hello");
    }

    #[test]
    fn test_openai_to_anthropic_body_system_extracted() {
        let req = ChatCompletionRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: Some(json!("You are helpful")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: Some(json!("Hi")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            n: None,
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
            reasoning_effort: None,
            response_format: None,
        };

        let body = AnthropicProvider::openai_to_anthropic_body(&req);
        assert_eq!(body["system"], "You are helpful");
        // System message must NOT be in messages array
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn test_openai_to_anthropic_body_default_max_tokens() {
        let req = ChatCompletionRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some(json!("Hi")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            n: None,
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
            reasoning_effort: None,
            response_format: None,
        };

        let body = AnthropicProvider::openai_to_anthropic_body(&req);
        assert_eq!(body["max_tokens"], 4096);
    }
}
