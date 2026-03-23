/// CopilotProvider — direct calls to GitHub Copilot's OpenAI-compatible API.
///
/// Copilot uses per-user base URLs (determined by copilot_plan) and requires
/// VS Code-mimicking headers on every request. Handles both OpenAI-format
/// (pass-through) and Anthropic-format (converted to OpenAI) inputs.
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use dashmap::DashMap;
use futures::stream::StreamExt;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::ApiError;
use crate::models::anthropic::AnthropicMessagesRequest;
use crate::models::openai::ChatCompletionRequest;
use crate::providers::anthropic_to_openai_body;
use crate::providers::traits::Provider;
use crate::providers::types::{
    ProviderContext, ProviderId, ProviderResponse, ProviderStreamResponse,
};
use crate::web_ui::copilot_auth::CopilotDevicePendingMap;

pub struct CopilotProvider {
    client: reqwest::Client,
    streaming_client: reqwest::Client,
    /// user_id → (copilot_token, base_url, cached_at)
    token_cache: Arc<DashMap<Uuid, (String, String, Instant)>>,
    /// Pending Copilot device flow states: device_code → CopilotDevicePending
    device_pending: CopilotDevicePendingMap,
}

impl CopilotProvider {
    pub fn new(client: reqwest::Client, streaming_client: reqwest::Client) -> Self {
        Self {
            client,
            streaming_client,
            token_cache: Arc::new(DashMap::new()),
            device_pending: Arc::new(DashMap::new()),
        }
    }

    /// Access the Copilot token cache.
    pub fn token_cache(&self) -> &Arc<DashMap<Uuid, (String, String, Instant)>> {
        &self.token_cache
    }

    /// Access the pending device flow map.
    pub fn device_pending(&self) -> &CopilotDevicePendingMap {
        &self.device_pending
    }

    /// Copilot completions URL: `{base_url}/chat/completions` (no /v1/ prefix).
    fn completions_url(ctx: &ProviderContext<'_>) -> String {
        let base = ctx
            .credentials
            .base_url
            .as_deref()
            .unwrap_or("https://api.githubcopilot.com");
        format!("{}/chat/completions", base)
    }

    /// Strip version suffixes from Claude model names for Copilot compatibility.
    fn normalize_model_name(model: &str) -> String {
        if model.starts_with("claude-sonnet-4-") {
            return "claude-sonnet-4".to_string();
        }
        if model.starts_with("claude-opus-4-") {
            return "claude-opus-4".to_string();
        }
        model.to_string()
    }

    /// Detect if any message contains image_url content parts.
    fn has_vision_content(body: &Value) -> bool {
        if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
            for msg in messages {
                if let Some(content) = msg.get("content").and_then(|c| c.as_array()) {
                    for part in content {
                        if part.get("type").and_then(|t| t.as_str()) == Some("image_url") {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Build and send a request with Copilot-specific headers.
    async fn send_request(
        &self,
        ctx: &ProviderContext<'_>,
        mut body: Value,
        stream: bool,
    ) -> Result<reqwest::Response, ApiError> {
        let url = Self::completions_url(ctx);
        let model = Self::normalize_model_name(ctx.model);
        body["model"] = json!(model);
        body["stream"] = json!(stream);

        let has_vision = Self::has_vision_content(&body);

        let client = if stream {
            &self.streaming_client
        } else {
            &self.client
        };

        let mut builder = client
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", ctx.credentials.access_token),
            )
            .header("content-type", "application/json")
            .header("copilot-integration-id", "vscode-chat")
            .header("editor-version", "vscode/1.104.1")
            .header("editor-plugin-version", "copilot-chat/0.26.7")
            .header("user-agent", "GitHubCopilotChat/0.26.7")
            .header("openai-intent", "conversation-panel")
            .header("x-github-api-version", "2025-04-01")
            .header("x-request-id", Uuid::new_v4().to_string());

        if has_vision {
            builder = builder.header("copilot-vision-request", "true");
        }

        let response =
            builder.json(&body).send().await.map_err(|e| {
                ApiError::Internal(anyhow::anyhow!("Copilot request failed: {}", e))
            })?;

        let status = response.status().as_u16();
        if !response.status().is_success() {
            let resp_headers = response.headers().clone();
            let error_text = response.text().await.unwrap_or_default();
            return Err(ApiError::ProviderApiError {
                provider: "copilot".to_string(),
                status,
                message: error_text,
                headers: Some(resp_headers),
            });
        }

        Ok(response)
    }
}

impl Default for CopilotProvider {
    fn default() -> Self {
        Self::new(reqwest::Client::new(), reqwest::Client::new())
    }
}

#[async_trait]
impl Provider for CopilotProvider {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn id(&self) -> ProviderId {
        ProviderId::Copilot
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
            ApiError::Internal(anyhow::anyhow!("Failed to parse Copilot response: {}", e))
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
            ApiError::Internal(anyhow::anyhow!("Failed to parse Copilot response: {}", e))
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
        let sse_values = crate::streaming::sse::parse_sse_stream(byte_stream);
        let stream =
            crate::streaming::cross_format::wrap_openai_stream_as_anthropic(sse_values, &req.model);
        Ok(ProviderStreamResponse { headers, stream })
    }

    /// Copilot (OpenAI-compatible) responses need conversion for the Anthropic endpoint.
    fn normalize_response_for_anthropic(&self, model: &str, body: Value) -> Value {
        // Reuse the same OpenAI→Anthropic conversion as OpenAICodex
        crate::providers::openai_codex::openai_response_to_anthropic(model, &body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::anthropic::{AnthropicMessage, AnthropicMessagesRequest};
    use crate::providers::anthropic_to_openai_body;
    use crate::providers::types::ProviderCredentials;

    #[test]
    fn test_copilot_provider_id() {
        assert_eq!(CopilotProvider::default().id(), ProviderId::Copilot);
    }

    #[test]
    fn test_completions_url_from_base_url() {
        let creds = ProviderCredentials {
            provider: ProviderId::Copilot,
            access_token: "tok".to_string(),
            base_url: Some("https://api.githubcopilot.com".to_string()),
            account_label: "default".to_string(),
        };
        let model = "gpt-4o".to_string();
        let ctx = ProviderContext {
            credentials: &creds,
            model: &model,
        };
        assert_eq!(
            CopilotProvider::completions_url(&ctx),
            "https://api.githubcopilot.com/chat/completions"
        );
    }

    #[test]
    fn test_completions_url_business() {
        let creds = ProviderCredentials {
            provider: ProviderId::Copilot,
            access_token: "tok".to_string(),
            base_url: Some("https://api.business.githubcopilot.com".to_string()),
            account_label: "default".to_string(),
        };
        let model = "gpt-4o".to_string();
        let ctx = ProviderContext {
            credentials: &creds,
            model: &model,
        };
        assert_eq!(
            CopilotProvider::completions_url(&ctx),
            "https://api.business.githubcopilot.com/chat/completions"
        );
    }

    #[test]
    fn test_completions_url_default_when_none() {
        let creds = ProviderCredentials {
            provider: ProviderId::Copilot,
            access_token: "tok".to_string(),
            base_url: None,
            account_label: "default".to_string(),
        };
        let model = "gpt-4o".to_string();
        let ctx = ProviderContext {
            credentials: &creds,
            model: &model,
        };
        assert_eq!(
            CopilotProvider::completions_url(&ctx),
            "https://api.githubcopilot.com/chat/completions"
        );
    }

    #[test]
    fn test_normalize_model_name_strips_claude_suffix() {
        assert_eq!(
            CopilotProvider::normalize_model_name("claude-sonnet-4-20250514"),
            "claude-sonnet-4"
        );
        assert_eq!(
            CopilotProvider::normalize_model_name("claude-opus-4-20250514"),
            "claude-opus-4"
        );
    }

    #[test]
    fn test_normalize_model_name_passthrough() {
        assert_eq!(CopilotProvider::normalize_model_name("gpt-4o"), "gpt-4o");
        assert_eq!(
            CopilotProvider::normalize_model_name("claude-sonnet-4"),
            "claude-sonnet-4"
        );
        assert_eq!(CopilotProvider::normalize_model_name("o4-mini"), "o4-mini");
    }

    #[test]
    fn test_has_vision_content_true() {
        let body = json!({
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "What is this?" },
                    { "type": "image_url", "image_url": { "url": "data:image/png;base64,..." } }
                ]
            }]
        });
        assert!(CopilotProvider::has_vision_content(&body));
    }

    #[test]
    fn test_has_vision_content_false_text_only() {
        let body = json!({
            "messages": [{
                "role": "user",
                "content": "Hello"
            }]
        });
        assert!(!CopilotProvider::has_vision_content(&body));
    }

    #[test]
    fn test_has_vision_content_false_no_image_url() {
        let body = json!({
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "Hello" }
                ]
            }]
        });
        assert!(!CopilotProvider::has_vision_content(&body));
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

    #[test]
    fn test_anthropic_to_openai_body_with_system_blocks() {
        let req = AnthropicMessagesRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: json!("Hi"),
            }],
            max_tokens: 100,
            system: Some(json!([
                { "type": "text", "text": "First" },
                { "type": "text", "text": "Second" }
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
        assert_eq!(body["messages"][0]["content"], "First\nSecond");
    }

    #[test]
    fn test_has_vision_content_empty_messages() {
        let body = json!({ "messages": [] });
        assert!(!CopilotProvider::has_vision_content(&body));
    }

    #[test]
    fn test_has_vision_content_no_messages_key() {
        let body = json!({ "model": "gpt-4o" });
        assert!(!CopilotProvider::has_vision_content(&body));
    }

    #[test]
    fn test_has_vision_content_multiple_messages_one_with_image() {
        let body = json!({
            "messages": [
                { "role": "user", "content": "Hello" },
                { "role": "user", "content": [
                    { "type": "image_url", "image_url": { "url": "data:..." } }
                ]}
            ]
        });
        assert!(CopilotProvider::has_vision_content(&body));
    }

    #[test]
    fn test_normalize_model_name_claude_haiku_passthrough() {
        // Only claude-sonnet-4- and claude-opus-4- get stripped
        assert_eq!(
            CopilotProvider::normalize_model_name("claude-3-5-haiku-20241022"),
            "claude-3-5-haiku-20241022"
        );
    }

    #[test]
    fn test_anthropic_to_openai_body_zero_max_tokens_passed_through() {
        let req = AnthropicMessagesRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: json!("Hi"),
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
        // Full converter passes through max_tokens as-is
        assert_eq!(body["max_tokens"], 0);
    }

    #[test]
    fn test_anthropic_to_openai_body_with_temperature() {
        let req = AnthropicMessagesRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: json!("Hi"),
            }],
            max_tokens: 100,
            system: None,
            stream: false,
            tools: None,
            tool_choice: None,
            temperature: Some(0.7),
            top_p: None,
            top_k: None,
            stop_sequences: None,
            metadata: None,
            thinking: None,
            disable_parallel_tool_use: None,
        };
        let body = anthropic_to_openai_body(&req);
        let temp = body["temperature"].as_f64().unwrap();
        assert!((temp - 0.7).abs() < 0.001);
    }

    #[test]
    fn test_anthropic_to_openai_body_empty_system_string_omitted() {
        let req = AnthropicMessagesRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: json!("Hi"),
            }],
            max_tokens: 100,
            system: Some(json!("")),
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
        // Empty system string should not produce a system message
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn test_anthropic_to_openai_body_multi_turn() {
        let req = AnthropicMessagesRequest {
            model: "claude-sonnet-4".to_string(),
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
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[2]["content"], "How are you?");
    }

    #[test]
    fn test_completions_url_enterprise() {
        let creds = ProviderCredentials {
            provider: ProviderId::Copilot,
            access_token: "tok".to_string(),
            base_url: Some("https://api.enterprise.githubcopilot.com".to_string()),
            account_label: "default".to_string(),
        };
        let model = "gpt-4o".to_string();
        let ctx = ProviderContext {
            credentials: &creds,
            model: &model,
        };
        assert_eq!(
            CopilotProvider::completions_url(&ctx),
            "https://api.enterprise.githubcopilot.com/chat/completions"
        );
    }

    #[test]
    fn test_copilot_provider_default() {
        let provider = CopilotProvider::default();
        assert_eq!(provider.id(), ProviderId::Copilot);
    }
}
