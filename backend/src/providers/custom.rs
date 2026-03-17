/// CustomProvider — generic OpenAI-compatible proxy for local/third-party endpoints.
///
/// Forwards requests to a configurable base_url. Auth is optional (Bearer token
/// only sent when access_token is non-empty). Useful for Ollama, vLLM, LiteLLM, etc.
use std::pin::Pin;

use async_trait::async_trait;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::models::anthropic::AnthropicMessagesRequest;
use crate::models::openai::ChatCompletionRequest;
use crate::providers::openai_codex::openai_response_to_anthropic;
use crate::providers::traits::Provider;
use crate::providers::types::{ProviderContext, ProviderId, ProviderResponse, ProviderStreamItem};
use crate::streaming::sse::parse_sse_stream;

pub struct CustomProvider {
    client: reqwest::Client,
}

impl CustomProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    fn completions_url(ctx: &ProviderContext<'_>) -> Result<String, ApiError> {
        let base = ctx.credentials.base_url.as_deref().ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "Custom provider requires CUSTOM_PROVIDER_URL"
            ))
        })?;
        // If base already ends with /chat/completions, use as-is
        if base.ends_with("/chat/completions") {
            Ok(base.to_string())
        } else {
            let trimmed = base.trim_end_matches('/');
            Ok(format!("{}/chat/completions", trimmed))
        }
    }

    /// Convert Anthropic messages format to OpenAI chat completions format.
    fn anthropic_to_openai_body(req: &AnthropicMessagesRequest) -> Value {
        let mut messages: Vec<Value> = Vec::new();

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
        let url = Self::completions_url(ctx)?;
        body["stream"] = json!(stream);

        let mut builder = self
            .client
            .post(&url)
            .header("content-type", "application/json");

        // Only add auth header when a key is configured
        if !ctx.credentials.access_token.is_empty() {
            builder = builder.header(
                "Authorization",
                format!("Bearer {}", ctx.credentials.access_token),
            );
        }

        let response = builder.json(&body).send().await.map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("Custom provider request failed: {}", e))
        })?;

        let status = response.status().as_u16();
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(ApiError::ProviderApiError {
                provider: "custom".to_string(),
                status,
                message: error_text,
            });
        }

        Ok(response)
    }
}

impl Default for CustomProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for CustomProvider {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn id(&self) -> ProviderId {
        ProviderId::Custom
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
            ApiError::Internal(anyhow::anyhow!(
                "Failed to parse custom provider response: {}",
                e
            ))
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
    ) -> Result<Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>>, ApiError> {
        let body = serde_json::to_value(req)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Serialization failed: {}", e)))?;
        let response = self.send_request(ctx, body, true).await?;
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
        let headers = response.headers().clone();
        let body: Value = response.json().await.map_err(|e| {
            ApiError::Internal(anyhow::anyhow!(
                "Failed to parse custom provider response: {}",
                e
            ))
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
    ) -> Result<Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>>, ApiError> {
        let body = Self::anthropic_to_openai_body(req);
        let response = self.send_request(ctx, body, true).await?;
        let byte_stream = response.bytes_stream();
        let sse_values = parse_sse_stream(byte_stream);
        Ok(crate::streaming::cross_format::wrap_openai_stream_as_anthropic(sse_values, &req.model))
    }

    fn normalize_response_for_anthropic(&self, model: &str, body: Value) -> Value {
        openai_response_to_anthropic(model, &body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::anthropic::{AnthropicMessage, AnthropicMessagesRequest};
    use crate::providers::types::ProviderCredentials;

    #[test]
    fn test_custom_provider_id() {
        assert_eq!(CustomProvider::new().id(), ProviderId::Custom);
    }

    #[test]
    fn test_completions_url_with_base() {
        let creds = ProviderCredentials {
            provider: ProviderId::Custom,
            access_token: String::new(),
            base_url: Some("http://localhost:11434/v1".to_string()),
            account_label: "proxy".to_string(),
        };
        let ctx = ProviderContext {
            credentials: &creds,
            model: "llama3",
        };
        assert_eq!(
            CustomProvider::completions_url(&ctx).unwrap(),
            "http://localhost:11434/v1/chat/completions"
        );
    }

    #[test]
    fn test_completions_url_trailing_slash() {
        let creds = ProviderCredentials {
            provider: ProviderId::Custom,
            access_token: String::new(),
            base_url: Some("http://localhost:11434/v1/".to_string()),
            account_label: "proxy".to_string(),
        };
        let ctx = ProviderContext {
            credentials: &creds,
            model: "llama3",
        };
        assert_eq!(
            CustomProvider::completions_url(&ctx).unwrap(),
            "http://localhost:11434/v1/chat/completions"
        );
    }

    #[test]
    fn test_completions_url_already_has_path() {
        let creds = ProviderCredentials {
            provider: ProviderId::Custom,
            access_token: String::new(),
            base_url: Some("http://localhost:8080/v1/chat/completions".to_string()),
            account_label: "proxy".to_string(),
        };
        let ctx = ProviderContext {
            credentials: &creds,
            model: "llama3",
        };
        assert_eq!(
            CustomProvider::completions_url(&ctx).unwrap(),
            "http://localhost:8080/v1/chat/completions"
        );
    }

    #[test]
    fn test_completions_url_missing_base_url_errors() {
        let creds = ProviderCredentials {
            provider: ProviderId::Custom,
            access_token: String::new(),
            base_url: None,
            account_label: "proxy".to_string(),
        };
        let ctx = ProviderContext {
            credentials: &creds,
            model: "llama3",
        };
        assert!(CustomProvider::completions_url(&ctx).is_err());
    }

    #[test]
    fn test_anthropic_to_openai_body_basic() {
        let req = AnthropicMessagesRequest {
            model: "llama3".to_string(),
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

        let body = CustomProvider::anthropic_to_openai_body(&req);
        assert_eq!(body["model"], "llama3");
        assert_eq!(body["max_tokens"], 1000);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "Hello");
    }

    #[test]
    fn test_anthropic_to_openai_body_with_system() {
        let req = AnthropicMessagesRequest {
            model: "llama3".to_string(),
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

        let body = CustomProvider::anthropic_to_openai_body(&req);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "Be helpful");
        assert_eq!(body["messages"][1]["role"], "user");
    }

    #[test]
    fn test_anthropic_to_openai_body_with_temperature() {
        let req = AnthropicMessagesRequest {
            model: "llama3".to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: json!("Hello"),
            }],
            max_tokens: 100,
            system: None,
            stream: false,
            tools: None,
            tool_choice: None,
            temperature: Some(0.5),
            top_p: None,
            top_k: None,
            stop_sequences: None,
            metadata: None,
            thinking: None,
            disable_parallel_tool_use: None,
        };

        let body = CustomProvider::anthropic_to_openai_body(&req);
        let temp = body["temperature"].as_f64().unwrap();
        assert!((temp - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_anthropic_to_openai_body_zero_max_tokens_omitted() {
        let req = AnthropicMessagesRequest {
            model: "llama3".to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: json!("Hello"),
            }],
            max_tokens: 0,
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

        let body = CustomProvider::anthropic_to_openai_body(&req);
        assert!(body.get("max_tokens").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_body_system_array_blocks() {
        let req = AnthropicMessagesRequest {
            model: "llama3".to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: json!("Hi"),
            }],
            max_tokens: 100,
            system: Some(json!([
                {"type": "text", "text": "First block"},
                {"type": "text", "text": "Second block"}
            ])),
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

        let body = CustomProvider::anthropic_to_openai_body(&req);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "First block\nSecond block");
    }

    #[test]
    fn test_anthropic_to_openai_body_multi_turn() {
        let req = AnthropicMessagesRequest {
            model: "llama3".to_string(),
            messages: vec![
                AnthropicMessage {
                    role: "user".to_string(),
                    content: json!("Hello"),
                },
                AnthropicMessage {
                    role: "assistant".to_string(),
                    content: json!("Hi there!"),
                },
                AnthropicMessage {
                    role: "user".to_string(),
                    content: json!("How are you?"),
                },
            ],
            max_tokens: 100,
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

        let body = CustomProvider::anthropic_to_openai_body(&req);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"], "How are you?");
    }

    #[test]
    fn test_custom_provider_default() {
        let provider = CustomProvider::default();
        assert_eq!(provider.id(), ProviderId::Custom);
    }
}
