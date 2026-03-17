/// QwenProvider — direct calls to Alibaba's Qwen Coder OpenAI-compatible API.
///
/// Qwen uses per-user base URLs (from `resource_url` in OAuth token response)
/// and requires Qwen-specific headers. Handles both OpenAI-format (pass-through)
/// and Anthropic-format (converted to OpenAI) inputs.
///
/// Qwen3 streaming workaround: injects a dummy tool definition when streaming
/// without tools to work around a known Qwen3 streaming bug.
use std::collections::VecDeque;
use std::pin::Pin;
use std::time::Instant;

use async_trait::async_trait;
use dashmap::DashMap;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};
use uuid::Uuid;

use std::sync::Arc;

use crate::error::ApiError;
use crate::models::anthropic::AnthropicMessagesRequest;
use crate::models::openai::ChatCompletionRequest;
use crate::providers::traits::Provider;
use crate::providers::types::{ProviderContext, ProviderId, ProviderResponse, ProviderStreamItem};
use crate::web_ui::qwen_auth::QwenDevicePendingMap;

/// Default Qwen API base URL.
const DEFAULT_BASE_URL: &str = "https://chat.qwen.ai/api";

/// Maximum requests per credential per sliding window.
const RATE_LIMIT_MAX_REQUESTS: usize = 60;

/// Sliding window duration in seconds.
const RATE_LIMIT_WINDOW_SECS: u64 = 60;

pub struct QwenProvider {
    client: reqwest::Client,
    /// Per-credential sliding window rate limiter: access_token_hash -> timestamps.
    /// TODO: Add periodic cleanup of stale entries to prevent unbounded memory growth.
    rate_limiter: DashMap<String, VecDeque<Instant>>,
    /// Pending Qwen device flow states: device_code → QwenDevicePending
    device_pending: QwenDevicePendingMap,
}

impl QwenProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            rate_limiter: DashMap::new(),
            device_pending: Arc::new(DashMap::new()),
        }
    }

    /// Access the pending device flow map.
    pub fn device_pending(&self) -> &QwenDevicePendingMap {
        &self.device_pending
    }

    /// Qwen completions URL: `{base_url}/v1/chat/completions`.
    fn completions_url(ctx: &ProviderContext<'_>) -> String {
        let base = ctx
            .credentials
            .base_url
            .as_deref()
            .unwrap_or(DEFAULT_BASE_URL);
        format!("{}/v1/chat/completions", base)
    }

    /// Detect if any message contains image_url content parts (for qwen-vl-* models).
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

    /// Inject Qwen3 dummy tool for streaming when no tools are defined.
    ///
    /// Qwen3 models have a known streaming bug where responses may be malformed
    /// when no tools are present. Injecting a dummy tool definition works around this.
    fn inject_dummy_tool_if_needed(body: &mut Value, stream: bool) {
        if !stream {
            return;
        }
        // Only inject if no tools are already defined
        let has_tools = body
            .get("tools")
            .and_then(|t| t.as_array())
            .is_some_and(|a| !a.is_empty());
        if has_tools {
            return;
        }
        body["tools"] = json!([{
            "type": "function",
            "function": {
                "name": "_qwen_dummy",
                "description": "Dummy tool for Qwen3 streaming compatibility",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }]);
    }

    /// Check per-credential rate limit (60 req/min sliding window).
    /// Returns Err(ApiError::RateLimited) if the limit is exceeded.
    fn check_rate_limit(&self, access_token: &str) -> Result<(), ApiError> {
        // Use first 16 chars of token as key to avoid storing full tokens
        let key = &access_token[..access_token.len().min(16)];
        let now = Instant::now();
        let window = std::time::Duration::from_secs(RATE_LIMIT_WINDOW_SECS);

        let mut entry = self.rate_limiter.entry(key.to_string()).or_default();

        // Evict timestamps older than the window
        while entry
            .front()
            .is_some_and(|t| now.duration_since(*t) > window)
        {
            entry.pop_front();
        }

        if entry.len() >= RATE_LIMIT_MAX_REQUESTS {
            // Calculate retry-after from the oldest entry in the window
            let oldest = entry.front().unwrap();
            let wait = window
                .checked_sub(now.duration_since(*oldest))
                .unwrap_or(std::time::Duration::from_secs(1));
            return Err(ApiError::RateLimited {
                provider: "qwen".to_string(),
                retry_after_secs: wait.as_secs().max(1),
            });
        }

        entry.push_back(now);
        Ok(())
    }

    /// Check if a 403 response body contains a quota error, and map it to 429.
    fn check_quota_error(status: u16, error_text: &str) -> ApiError {
        if status == 403 {
            // Try to parse as JSON and check for quota error codes
            if let Ok(body) = serde_json::from_str::<Value>(error_text) {
                let error_code = body
                    .get("error")
                    .and_then(|e| e.get("code"))
                    .and_then(|c| c.as_str())
                    .or_else(|| body.get("error_code").and_then(|c| c.as_str()))
                    .or_else(|| body.get("code").and_then(|c| c.as_str()))
                    .unwrap_or("");
                if error_code == "insufficient_quota" || error_code == "quota_exceeded" {
                    return ApiError::RateLimited {
                        provider: "qwen".to_string(),
                        retry_after_secs: 60,
                    };
                }
            }
        }
        ApiError::ProviderApiError {
            provider: "qwen".to_string(),
            status,
            message: error_text.to_string(),
        }
    }

    /// Build and send a request with Qwen-specific headers.
    async fn send_request(
        &self,
        ctx: &ProviderContext<'_>,
        mut body: Value,
        stream: bool,
    ) -> Result<reqwest::Response, ApiError> {
        // Check rate limit before sending
        self.check_rate_limit(&ctx.credentials.access_token)?;

        let url = Self::completions_url(ctx);
        body["stream"] = json!(stream);

        // Inject dummy tool for Qwen3 streaming workaround
        Self::inject_dummy_tool_if_needed(&mut body, stream);

        // Add stream_options for usage tracking on streaming requests
        if stream {
            body["stream_options"] = json!({ "include_usage": true });
        }

        let has_vision = Self::has_vision_content(&body);

        let mut builder = self
            .client
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", ctx.credentials.access_token),
            )
            .header("content-type", "application/json")
            .header("user-agent", "QwenCoder/1.0")
            .header("x-dashscope-client", "harbangan")
            .header("x-request-id", Uuid::new_v4().to_string());

        if has_vision {
            builder = builder.header("x-dashscope-vision", "true");
        }

        let response = builder
            .json(&body)
            .send()
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Qwen request failed: {}", e)))?;

        let status = response.status().as_u16();
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(Self::check_quota_error(status, &error_text));
        }

        Ok(response)
    }
}

impl Default for QwenProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for QwenProvider {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn id(&self) -> ProviderId {
        ProviderId::Qwen
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
            ApiError::Internal(anyhow::anyhow!("Failed to parse Qwen response: {}", e))
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
            ApiError::Internal(anyhow::anyhow!("Failed to parse Qwen response: {}", e))
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
        let sse_values = crate::streaming::sse::parse_sse_stream(byte_stream);
        Ok(crate::streaming::cross_format::wrap_openai_stream_as_anthropic(sse_values, &req.model))
    }

    /// Qwen (OpenAI-compatible) responses need conversion for the Anthropic endpoint.
    fn normalize_response_for_anthropic(&self, model: &str, body: Value) -> Value {
        // Reuse the same OpenAI→Anthropic conversion as OpenAICodex
        crate::providers::openai_codex::openai_response_to_anthropic(model, &body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::anthropic::{AnthropicMessage, AnthropicMessagesRequest};
    use crate::providers::types::ProviderCredentials;

    #[test]
    fn test_qwen_provider_id() {
        assert_eq!(QwenProvider::new().id(), ProviderId::Qwen);
    }

    #[test]
    fn test_completions_url_default() {
        let creds = ProviderCredentials {
            provider: ProviderId::Qwen,
            access_token: "tok".to_string(),
            base_url: None,
            account_label: "default".to_string(),
        };
        let model = "qwen-coder-plus".to_string();
        let ctx = ProviderContext {
            credentials: &creds,
            model: &model,
        };
        assert_eq!(
            QwenProvider::completions_url(&ctx),
            "https://chat.qwen.ai/api/v1/chat/completions"
        );
    }

    #[test]
    fn test_completions_url_resource_url_override() {
        let creds = ProviderCredentials {
            provider: ProviderId::Qwen,
            access_token: "tok".to_string(),
            base_url: Some("https://custom.qwen.ai/api".to_string()),
            account_label: "default".to_string(),
        };
        let model = "qwen-coder-plus".to_string();
        let ctx = ProviderContext {
            credentials: &creds,
            model: &model,
        };
        assert_eq!(
            QwenProvider::completions_url(&ctx),
            "https://custom.qwen.ai/api/v1/chat/completions"
        );
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
        assert!(QwenProvider::has_vision_content(&body));
    }

    #[test]
    fn test_has_vision_content_false() {
        let body = json!({
            "messages": [{
                "role": "user",
                "content": "Hello"
            }]
        });
        assert!(!QwenProvider::has_vision_content(&body));
    }

    #[test]
    fn test_has_vision_content_no_image_url() {
        let body = json!({
            "messages": [{
                "role": "user",
                "content": [{ "type": "text", "text": "Hello" }]
            }]
        });
        assert!(!QwenProvider::has_vision_content(&body));
    }

    #[test]
    fn test_inject_dummy_tool_streaming_no_tools() {
        let mut body = json!({ "model": "qwen3-coder", "messages": [] });
        QwenProvider::inject_dummy_tool_if_needed(&mut body, true);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], "_qwen_dummy");
    }

    #[test]
    fn test_inject_dummy_tool_not_streaming() {
        let mut body = json!({ "model": "qwen3-coder", "messages": [] });
        QwenProvider::inject_dummy_tool_if_needed(&mut body, false);
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn test_inject_dummy_tool_existing_tools_preserved() {
        let mut body = json!({
            "model": "qwen3-coder",
            "messages": [],
            "tools": [{ "type": "function", "function": { "name": "real_tool" } }]
        });
        QwenProvider::inject_dummy_tool_if_needed(&mut body, true);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], "real_tool");
    }

    #[test]
    fn test_anthropic_to_openai_body_basic() {
        let req = AnthropicMessagesRequest {
            model: "qwen-coder-plus".to_string(),
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

        let body = QwenProvider::anthropic_to_openai_body(&req);
        assert_eq!(body["model"], "qwen-coder-plus");
        assert_eq!(body["max_tokens"], 1000);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "Hello");
    }

    #[test]
    fn test_anthropic_to_openai_body_with_system() {
        let req = AnthropicMessagesRequest {
            model: "qwen-coder-plus".to_string(),
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

        let body = QwenProvider::anthropic_to_openai_body(&req);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "Be helpful");
        assert_eq!(body["messages"][1]["role"], "user");
    }

    #[test]
    fn test_anthropic_to_openai_body_with_system_blocks() {
        let req = AnthropicMessagesRequest {
            model: "qwen-coder-plus".to_string(),
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

        let body = QwenProvider::anthropic_to_openai_body(&req);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "First\nSecond");
    }

    #[test]
    fn test_qwen_provider_default() {
        let provider = QwenProvider::default();
        assert_eq!(provider.id(), ProviderId::Qwen);
    }

    // ── Rate limiter tests ──────────────────────────────────────────

    #[test]
    fn test_rate_limit_allows_under_limit() {
        let provider = QwenProvider::new();
        for _ in 0..59 {
            assert!(provider.check_rate_limit("test-token-abcdef").is_ok());
        }
    }

    #[test]
    fn test_rate_limit_blocks_at_limit() {
        let provider = QwenProvider::new();
        for _ in 0..RATE_LIMIT_MAX_REQUESTS {
            provider.check_rate_limit("test-token-abcdef").unwrap();
        }
        let err = provider.check_rate_limit("test-token-abcdef").unwrap_err();
        match err {
            ApiError::RateLimited {
                provider: p,
                retry_after_secs,
            } => {
                assert_eq!(p, "qwen");
                assert!(retry_after_secs >= 1);
                assert!(retry_after_secs <= RATE_LIMIT_WINDOW_SECS);
            }
            other => panic!("Expected RateLimited, got: {:?}", other),
        }
    }

    #[test]
    fn test_rate_limit_different_credentials_independent() {
        let provider = QwenProvider::new();
        // Fill up one credential
        for _ in 0..RATE_LIMIT_MAX_REQUESTS {
            provider.check_rate_limit("credential-aaaa").unwrap();
        }
        assert!(provider.check_rate_limit("credential-aaaa").is_err());
        // Different credential should still work
        assert!(provider.check_rate_limit("credential-bbbb").is_ok());
    }

    #[test]
    fn test_rate_limit_sliding_window_eviction() {
        let provider = QwenProvider::new();
        let key = "test-token-abcdef"[..16].to_string();

        // Manually insert old timestamps that are past the window
        {
            let mut timestamps = VecDeque::new();
            let old = Instant::now() - std::time::Duration::from_secs(RATE_LIMIT_WINDOW_SECS + 1);
            for _ in 0..RATE_LIMIT_MAX_REQUESTS {
                timestamps.push_back(old);
            }
            provider.rate_limiter.insert(key, timestamps);
        }

        // Should succeed because old entries get evicted
        assert!(provider.check_rate_limit("test-token-abcdef").is_ok());
    }

    // ── Quota error detection tests ─────────────────────────────────

    #[test]
    fn test_quota_error_insufficient_quota() {
        let body =
            r#"{"error":{"code":"insufficient_quota","message":"You have exceeded your quota"}}"#;
        let err = QwenProvider::check_quota_error(403, body);
        match err {
            ApiError::RateLimited {
                provider,
                retry_after_secs,
            } => {
                assert_eq!(provider, "qwen");
                assert_eq!(retry_after_secs, 60);
            }
            other => panic!("Expected RateLimited, got: {:?}", other),
        }
    }

    #[test]
    fn test_quota_error_quota_exceeded() {
        let body = r#"{"error_code":"quota_exceeded"}"#;
        let err = QwenProvider::check_quota_error(403, body);
        match err {
            ApiError::RateLimited { provider, .. } => assert_eq!(provider, "qwen"),
            other => panic!("Expected RateLimited, got: {:?}", other),
        }
    }

    #[test]
    fn test_quota_error_top_level_code() {
        let body = r#"{"code":"insufficient_quota"}"#;
        let err = QwenProvider::check_quota_error(403, body);
        match err {
            ApiError::RateLimited { provider, .. } => assert_eq!(provider, "qwen"),
            other => panic!("Expected RateLimited, got: {:?}", other),
        }
    }

    #[test]
    fn test_quota_error_403_other_code_passthrough() {
        let body = r#"{"error":{"code":"access_denied","message":"Forbidden"}}"#;
        let err = QwenProvider::check_quota_error(403, body);
        match err {
            ApiError::ProviderApiError {
                provider,
                status,
                message,
            } => {
                assert_eq!(provider, "qwen");
                assert_eq!(status, 403);
                assert!(message.contains("access_denied"));
            }
            other => panic!("Expected ProviderApiError, got: {:?}", other),
        }
    }

    #[test]
    fn test_quota_error_non_403_passthrough() {
        let body = r#"{"error":{"code":"insufficient_quota"}}"#;
        // Non-403 status should NOT be mapped to RateLimited
        let err = QwenProvider::check_quota_error(500, body);
        match err {
            ApiError::ProviderApiError { status, .. } => assert_eq!(status, 500),
            other => panic!("Expected ProviderApiError, got: {:?}", other),
        }
    }

    #[test]
    fn test_quota_error_invalid_json_passthrough() {
        let body = "not json at all";
        let err = QwenProvider::check_quota_error(403, body);
        match err {
            ApiError::ProviderApiError {
                provider, status, ..
            } => {
                assert_eq!(provider, "qwen");
                assert_eq!(status, 403);
            }
            other => panic!("Expected ProviderApiError, got: {:?}", other),
        }
    }

    // ── 6.1: Additional QwenProvider helper tests ───────────────────

    #[test]
    fn test_completions_url_trailing_slash_in_base_url() {
        // base_url with trailing slash should still produce valid URL
        let creds = ProviderCredentials {
            provider: ProviderId::Qwen,
            access_token: "tok".to_string(),
            base_url: Some("https://custom.qwen.ai/api/".to_string()),
            account_label: "default".to_string(),
        };
        let model = "qwen-coder-plus".to_string();
        let ctx = ProviderContext {
            credentials: &creds,
            model: &model,
        };
        // The function just concatenates, so trailing slash produces double slash
        // This verifies the current behavior
        let url = QwenProvider::completions_url(&ctx);
        assert!(url.contains("v1/chat/completions"));
    }

    #[test]
    fn test_anthropic_to_openai_body_multi_turn() {
        let req = AnthropicMessagesRequest {
            model: "qwen-coder-plus".to_string(),
            messages: vec![
                AnthropicMessage {
                    role: "user".to_string(),
                    content: json!("What is Rust?"),
                },
                AnthropicMessage {
                    role: "assistant".to_string(),
                    content: json!("Rust is a systems programming language."),
                },
                AnthropicMessage {
                    role: "user".to_string(),
                    content: json!("Tell me more."),
                },
            ],
            max_tokens: 500,
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

        let body = QwenProvider::anthropic_to_openai_body(&req);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"], "Tell me more.");
    }

    #[test]
    fn test_anthropic_to_openai_body_with_temperature() {
        let req = AnthropicMessagesRequest {
            model: "qwen-coder-plus".to_string(),
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

        let body = QwenProvider::anthropic_to_openai_body(&req);
        let temp = body["temperature"].as_f64().unwrap();
        assert!(
            (temp - 0.7).abs() < 0.001,
            "temperature should be ~0.7, got {temp}"
        );
    }

    #[test]
    fn test_anthropic_to_openai_body_max_tokens_zero_omitted() {
        let req = AnthropicMessagesRequest {
            model: "qwen-coder-plus".to_string(),
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

        let body = QwenProvider::anthropic_to_openai_body(&req);
        // max_tokens=0 should NOT be included in the body
        assert!(body.get("max_tokens").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_body_empty_system_string() {
        let req = AnthropicMessagesRequest {
            model: "qwen-coder-plus".to_string(),
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

        let body = QwenProvider::anthropic_to_openai_body(&req);
        // Empty system string should not produce a system message
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
    }

    #[test]
    fn test_anthropic_to_openai_body_content_array_passthrough() {
        // When content is an array (e.g., multimodal), it should pass through as-is
        let req = AnthropicMessagesRequest {
            model: "qwen-vl-plus".to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: json!([
                    { "type": "text", "text": "What is this?" },
                    { "type": "image_url", "image_url": { "url": "data:image/png;base64,abc" } }
                ]),
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

        let body = QwenProvider::anthropic_to_openai_body(&req);
        let content = &body["messages"][0]["content"];
        assert!(content.is_array());
        assert_eq!(content.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_anthropic_to_openai_body_stream_always_false() {
        // anthropic_to_openai_body always sets stream=false (send_request overrides it)
        let req = AnthropicMessagesRequest {
            model: "qwen-coder-plus".to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: json!("Hi"),
            }],
            max_tokens: 100,
            system: None,
            stream: true, // Even if stream=true in the request
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

        let body = QwenProvider::anthropic_to_openai_body(&req);
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn test_anthropic_to_openai_body_no_messages() {
        let req = AnthropicMessagesRequest {
            model: "qwen-coder-plus".to_string(),
            messages: vec![],
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

        let body = QwenProvider::anthropic_to_openai_body(&req);
        let messages = body["messages"].as_array().unwrap();
        assert!(messages.is_empty());
    }

    // ── 6.3: Additional dummy tool injection + stream_options tests ──

    #[test]
    fn test_inject_dummy_tool_empty_tools_array() {
        // Empty tools array should trigger injection (same as no tools)
        let mut body = json!({ "model": "qwen3-coder", "messages": [], "tools": [] });
        QwenProvider::inject_dummy_tool_if_needed(&mut body, true);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], "_qwen_dummy");
    }

    #[test]
    fn test_inject_dummy_tool_structure() {
        let mut body = json!({ "model": "qwen3-coder", "messages": [] });
        QwenProvider::inject_dummy_tool_if_needed(&mut body, true);
        let tool = &body["tools"][0];
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["function"]["name"], "_qwen_dummy");
        assert_eq!(tool["function"]["parameters"]["type"], "object");
        assert!(tool["function"]["description"]
            .as_str()
            .unwrap()
            .contains("Qwen3"));
    }

    #[test]
    fn test_inject_dummy_tool_multiple_existing_tools() {
        let mut body = json!({
            "model": "qwen3-coder",
            "messages": [],
            "tools": [
                { "type": "function", "function": { "name": "tool_a" } },
                { "type": "function", "function": { "name": "tool_b" } }
            ]
        });
        QwenProvider::inject_dummy_tool_if_needed(&mut body, true);
        let tools = body["tools"].as_array().unwrap();
        // Should preserve both existing tools, not inject dummy
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["function"]["name"], "tool_a");
        assert_eq!(tools[1]["function"]["name"], "tool_b");
    }

    // ── 6.4: Additional rate limiting edge case tests ───────────────

    #[test]
    fn test_rate_limit_exact_boundary() {
        let provider = QwenProvider::new();
        // 60th request should succeed (fills the window)
        for i in 0..RATE_LIMIT_MAX_REQUESTS {
            let result = provider.check_rate_limit("test-token-abcdef");
            assert!(result.is_ok(), "Request {} should succeed", i);
        }
        // 61st request should fail
        assert!(provider.check_rate_limit("test-token-abcdef").is_err());
    }

    #[test]
    fn test_rate_limit_short_token() {
        // Token shorter than 16 chars should still work (uses min(len, 16))
        let provider = QwenProvider::new();
        assert!(provider.check_rate_limit("short").is_ok());
    }

    #[test]
    fn test_rate_limit_single_char_token() {
        let provider = QwenProvider::new();
        assert!(provider.check_rate_limit("x").is_ok());
    }

    #[test]
    fn test_rate_limit_tokens_sharing_prefix_share_bucket() {
        // Two tokens with the same first 16 chars share a rate limit bucket
        let provider = QwenProvider::new();
        let token_a = "abcdefghijklmnop_suffix_a";
        let token_b = "abcdefghijklmnop_suffix_b";
        for _ in 0..RATE_LIMIT_MAX_REQUESTS {
            provider.check_rate_limit(token_a).unwrap();
        }
        // token_b shares the same 16-char prefix, so it should be rate limited too
        assert!(provider.check_rate_limit(token_b).is_err());
    }

    #[test]
    fn test_rate_limit_retry_after_is_positive() {
        let provider = QwenProvider::new();
        for _ in 0..RATE_LIMIT_MAX_REQUESTS {
            provider.check_rate_limit("test-token-abcdef").unwrap();
        }
        match provider.check_rate_limit("test-token-abcdef").unwrap_err() {
            ApiError::RateLimited {
                retry_after_secs, ..
            } => {
                assert!(retry_after_secs >= 1, "retry_after should be at least 1");
            }
            other => panic!("Expected RateLimited, got: {:?}", other),
        }
    }

    // ── 6.8: Additional vision detection tests ──────────────────────

    #[test]
    fn test_has_vision_content_multiple_messages_mixed() {
        // Vision content in second message should still be detected
        let body = json!({
            "messages": [
                { "role": "user", "content": "Hello" },
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "What is this?" },
                        { "type": "image_url", "image_url": { "url": "https://example.com/img.png" } }
                    ]
                }
            ]
        });
        assert!(QwenProvider::has_vision_content(&body));
    }

    #[test]
    fn test_has_vision_content_empty_messages() {
        let body = json!({ "messages": [] });
        assert!(!QwenProvider::has_vision_content(&body));
    }

    #[test]
    fn test_has_vision_content_no_messages_key() {
        let body = json!({ "model": "qwen-vl-plus" });
        assert!(!QwenProvider::has_vision_content(&body));
    }

    #[test]
    fn test_has_vision_content_multiple_images() {
        let body = json!({
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image_url", "image_url": { "url": "data:image/png;base64,aaa" } },
                    { "type": "text", "text": "Compare these" },
                    { "type": "image_url", "image_url": { "url": "data:image/png;base64,bbb" } }
                ]
            }]
        });
        assert!(QwenProvider::has_vision_content(&body));
    }

    #[test]
    fn test_has_vision_content_only_text_parts() {
        let body = json!({
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "Hello" },
                    { "type": "text", "text": "World" }
                ]
            }]
        });
        assert!(!QwenProvider::has_vision_content(&body));
    }

    // ── 6.1: Quota error edge cases ─────────────────────────────────

    #[test]
    fn test_quota_error_empty_body() {
        let err = QwenProvider::check_quota_error(403, "");
        match err {
            ApiError::ProviderApiError { status, .. } => assert_eq!(status, 403),
            other => panic!("Expected ProviderApiError, got: {:?}", other),
        }
    }

    #[test]
    fn test_quota_error_empty_json_object() {
        let err = QwenProvider::check_quota_error(403, "{}");
        match err {
            ApiError::ProviderApiError { status, .. } => assert_eq!(status, 403),
            other => panic!("Expected ProviderApiError, got: {:?}", other),
        }
    }

    #[test]
    fn test_check_quota_error_preserves_message() {
        let body = r#"{"error":{"code":"server_error","message":"Internal failure"}}"#;
        let err = QwenProvider::check_quota_error(500, body);
        match err {
            ApiError::ProviderApiError { message, .. } => {
                assert!(message.contains("server_error"));
            }
            other => panic!("Expected ProviderApiError, got: {:?}", other),
        }
    }
}
