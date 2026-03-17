/// CustomProvider — generic OpenAI-compatible proxy for local/third-party endpoints.
///
/// Forwards requests to a configurable base_url. Auth is optional (Bearer token
/// only sent when access_token is non-empty). Useful for Ollama, vLLM, LiteLLM, etc.
use std::pin::Pin;

use async_trait::async_trait;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};

use crate::providers::anthropic_to_openai_body;

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
        let body = anthropic_to_openai_body(req);
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
        let body = anthropic_to_openai_body(req);
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
    use crate::models::openai::ChatCompletionRequest;
    use crate::providers::anthropic_to_openai_body;
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

        let body = anthropic_to_openai_body(&req);
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

        let body = anthropic_to_openai_body(&req);
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

        let body = anthropic_to_openai_body(&req);
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

        let body = anthropic_to_openai_body(&req);
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

        let body = anthropic_to_openai_body(&req);
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

        let body = anthropic_to_openai_body(&req);
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

    // ── HTTP execution tests (mockito) ──────────────────────────────

    fn make_ctx_with_url(url: &str, token: &str) -> (ProviderCredentials, String) {
        (
            ProviderCredentials {
                provider: ProviderId::Custom,
                access_token: token.to_string(),
                base_url: Some(url.to_string()),
                account_label: "proxy".to_string(),
            },
            "test-model".to_string(),
        )
    }

    fn make_test_openai_req() -> ChatCompletionRequest {
        serde_json::from_value(json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn test_execute_openai_sends_correct_request() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("content-type", "application/json")
            .match_header("authorization", "Bearer test-key-123")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"chatcmpl-1","choices":[{"message":{"content":"Hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2}}"#)
            .create_async()
            .await;

        let provider = CustomProvider::new();
        let (creds, model) = make_ctx_with_url(&server.url(), "test-key-123");
        let ctx = ProviderContext {
            credentials: &creds,
            model: &model,
        };
        let req = make_test_openai_req();
        let result = provider.execute_openai(&ctx, &req).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body["choices"][0]["message"]["content"], "Hi");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_send_request_skips_auth_when_token_empty() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("content-type", "application/json")
            // Verify NO Authorization header is sent
            .match_header("authorization", mockito::Matcher::Missing)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"chatcmpl-1","choices":[]}"#)
            .create_async()
            .await;

        let provider = CustomProvider::new();
        let (creds, _model) = make_ctx_with_url(&server.url(), "");
        let ctx = ProviderContext {
            credentials: &creds,
            model: "test-model",
        };
        let body = json!({"model": "test-model", "messages": []});
        let result = provider.send_request(&ctx, body, false).await;
        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_send_request_returns_error_on_4xx() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/chat/completions")
            .with_status(401)
            .with_body(r#"{"error":"unauthorized"}"#)
            .create_async()
            .await;

        let provider = CustomProvider::new();
        let (creds, _model) = make_ctx_with_url(&server.url(), "bad-key");
        let ctx = ProviderContext {
            credentials: &creds,
            model: "test-model",
        };
        let body = json!({"model": "test-model", "messages": []});
        let result = provider.send_request(&ctx, body, false).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ApiError::ProviderApiError {
                provider,
                status,
                message,
            } => {
                assert_eq!(provider, "custom");
                assert_eq!(status, 401);
                assert!(message.contains("unauthorized"));
            }
            other => panic!("Expected ProviderApiError, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_send_request_returns_error_on_5xx() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/chat/completions")
            .with_status(500)
            .with_body("Internal Server Error")
            .create_async()
            .await;

        let provider = CustomProvider::new();
        let (creds, _model) = make_ctx_with_url(&server.url(), "key");
        let ctx = ProviderContext {
            credentials: &creds,
            model: "test-model",
        };
        let body = json!({"model": "test-model", "messages": []});
        let result = provider.send_request(&ctx, body, false).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ApiError::ProviderApiError { status, .. } => assert_eq!(status, 500),
            other => panic!("Expected ProviderApiError, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_execute_anthropic_converts_and_sends() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"chatcmpl-1","choices":[{"message":{"content":"Hello!"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2}}"#)
            .create_async()
            .await;

        let provider = CustomProvider::new();
        let (creds, _model) = make_ctx_with_url(&server.url(), "test-key");
        let ctx = ProviderContext {
            credentials: &creds,
            model: "llama3",
        };
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
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            metadata: None,
            thinking: None,
            disable_parallel_tool_use: None,
        };
        let result = provider.execute_anthropic(&ctx, &req).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body["choices"][0]["message"]["content"], "Hello!");
        mock.assert_async().await;
    }
}
