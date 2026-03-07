/// GeminiProvider — direct calls to generativelanguage.googleapis.com.
///
/// Converts both OpenAI and Anthropic format inputs to Gemini's generateContent format.
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

const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com";

pub struct GeminiProvider {
    client: reqwest::Client,
}

impl GeminiProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    fn base_url<'a>(&self, ctx: &ProviderContext<'a>) -> &'a str {
        ctx.credentials
            .base_url
            .as_deref()
            .unwrap_or(GEMINI_API_BASE)
    }

    fn generate_url(&self, ctx: &ProviderContext<'_>, stream: bool) -> String {
        let method = if stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        let base = self.base_url(ctx);
        let model = ctx.model;
        let key = &ctx.credentials.access_token;
        if stream {
            format!(
                "{}/v1beta/models/{}:{}?alt=sse&key={}",
                base, model, method, key
            )
        } else {
            format!("{}/v1beta/models/{}:{}?key={}", base, model, method, key)
        }
    }

    /// Convert OpenAI message array to Gemini contents format.
    /// Returns (contents, system_instruction_text).
    fn openai_to_gemini_body(req: &ChatCompletionRequest) -> Value {
        let mut system_instruction: Option<String> = None;
        let mut contents: Vec<Value> = Vec::new();

        for msg in &req.messages {
            let text = msg
                .content
                .as_ref()
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();

            match msg.role.as_str() {
                "system" => {
                    system_instruction = Some(text);
                }
                "assistant" => {
                    contents.push(json!({
                        "role": "model",
                        "parts": [{ "text": text }]
                    }));
                }
                _ => {
                    contents.push(json!({
                        "role": "user",
                        "parts": [{ "text": text }]
                    }));
                }
            }
        }

        let mut body = json!({ "contents": contents });

        if let Some(sys) = system_instruction {
            body["systemInstruction"] = json!({
                "parts": [{ "text": sys }]
            });
        }

        let mut gen_config = json!({});
        if let Some(max_tokens) = req.max_tokens {
            gen_config["maxOutputTokens"] = json!(max_tokens);
        }
        if let Some(temp) = req.temperature {
            gen_config["temperature"] = json!(temp);
        }
        if gen_config
            .as_object()
            .map(|m| !m.is_empty())
            .unwrap_or(false)
        {
            body["generationConfig"] = gen_config;
        }

        body
    }

    fn anthropic_to_gemini_body(req: &AnthropicMessagesRequest) -> Value {
        let mut contents: Vec<Value> = Vec::new();

        for msg in &req.messages {
            let text = msg
                .content
                .as_str()
                .map(String::from)
                .or_else(|| {
                    msg.content.as_array().map(|blocks| {
                        blocks
                            .iter()
                            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                            .collect::<Vec<_>>()
                            .join("")
                    })
                })
                .unwrap_or_default();

            let role = if msg.role == "assistant" {
                "model"
            } else {
                "user"
            };
            contents.push(json!({
                "role": role,
                "parts": [{ "text": text }]
            }));
        }

        let mut body = json!({ "contents": contents });

        if let Some(system) = &req.system {
            let sys_text = system
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
            if !sys_text.is_empty() {
                body["systemInstruction"] = json!({
                    "parts": [{ "text": sys_text }]
                });
            }
        }

        if req.max_tokens > 0 {
            body["generationConfig"] = json!({ "maxOutputTokens": req.max_tokens });
        }

        body
    }

    async fn send_request(&self, url: &str, body: Value) -> Result<reqwest::Response, ApiError> {
        let response = self
            .client
            .post(url)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Gemini request failed: {}", e)))?;

        let status = response.status().as_u16();
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(ApiError::ProviderApiError {
                provider: "gemini".to_string(),
                status,
                message: error_text,
            });
        }

        Ok(response)
    }
}

impl Default for GeminiProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for GeminiProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Gemini
    }

    async fn execute_openai(
        &self,
        ctx: &ProviderContext<'_>,
        req: &ChatCompletionRequest,
    ) -> Result<ProviderResponse, ApiError> {
        let url = self.generate_url(ctx, false);
        let body = Self::openai_to_gemini_body(req);
        let response = self.send_request(&url, body).await?;
        let status = response.status().as_u16();
        let body: Value = response.json().await.map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("Failed to parse Gemini response: {}", e))
        })?;
        Ok(ProviderResponse { status, body })
    }

    async fn stream_openai(
        &self,
        ctx: &ProviderContext<'_>,
        req: &ChatCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>>, ApiError> {
        let url = self.generate_url(ctx, true);
        let body = Self::openai_to_gemini_body(req);
        let response = self.send_request(&url, body).await?;
        let byte_stream = response.bytes_stream();
        let stream = parse_sse_stream(byte_stream).map(|item| match item {
            Ok(v) => {
                let line = format!("data: {}\n\n", v);
                Ok(Bytes::from(line))
            }
            Err(e) => Err(e),
        });
        Ok(Box::pin(stream))
    }

    async fn execute_anthropic(
        &self,
        ctx: &ProviderContext<'_>,
        req: &AnthropicMessagesRequest,
    ) -> Result<ProviderResponse, ApiError> {
        let url = self.generate_url(ctx, false);
        let body = Self::anthropic_to_gemini_body(req);
        let response = self.send_request(&url, body).await?;
        let status = response.status().as_u16();
        let body: Value = response.json().await.map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("Failed to parse Gemini response: {}", e))
        })?;
        Ok(ProviderResponse { status, body })
    }

    async fn stream_anthropic(
        &self,
        ctx: &ProviderContext<'_>,
        req: &AnthropicMessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>>, ApiError> {
        let url = self.generate_url(ctx, true);
        let body = Self::anthropic_to_gemini_body(req);
        let response = self.send_request(&url, body).await?;
        let byte_stream = response.bytes_stream();
        let stream = parse_sse_stream(byte_stream).map(|item| match item {
            Ok(v) => {
                let line = format!("data: {}\n\n", v);
                Ok(Bytes::from(line))
            }
            Err(e) => Err(e),
        });
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::anthropic::{AnthropicMessage, AnthropicMessagesRequest};
    use crate::models::openai::{ChatCompletionRequest, ChatMessage};
    use crate::providers::types::ProviderCredentials;

    fn make_ctx_parts(key: &str, model: &str) -> (ProviderCredentials, String) {
        (
            ProviderCredentials {
                provider: ProviderId::Gemini,
                access_token: key.to_string(),
                base_url: None,
            },
            model.to_string(),
        )
    }

    #[test]
    fn test_gemini_provider_id() {
        assert_eq!(GeminiProvider::new().id(), ProviderId::Gemini);
    }

    #[test]
    fn test_generate_url_non_streaming() {
        let provider = GeminiProvider::new();
        let (creds, model) = make_ctx_parts("AIzaTestKey123", "gemini-2.5-pro");
        let ctx = ProviderContext {
            credentials: &creds,
            model: &model,
        };
        let url = provider.generate_url(&ctx, false);
        assert!(url.contains("generateContent"));
        assert!(url.contains("gemini-2.5-pro"));
        assert!(url.contains("AIzaTestKey123"));
        assert!(!url.contains("alt=sse"));
    }

    #[test]
    fn test_generate_url_streaming() {
        let provider = GeminiProvider::new();
        let (creds, model) = make_ctx_parts("AIzaTestKey123", "gemini-2.5-flash");
        let ctx = ProviderContext {
            credentials: &creds,
            model: &model,
        };
        let url = provider.generate_url(&ctx, true);
        assert!(url.contains("streamGenerateContent"));
        assert!(url.contains("alt=sse"));
        assert!(url.contains("AIzaTestKey123"));
    }

    #[test]
    fn test_openai_to_gemini_body_basic() {
        let req = ChatCompletionRequest {
            model: "gemini-2.5-pro".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some(json!("Hello")),
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
        };

        let body = GeminiProvider::openai_to_gemini_body(&req);
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "Hello");
    }

    #[test]
    fn test_openai_to_gemini_body_system_extracted() {
        let req = ChatCompletionRequest {
            model: "gemini-2.5-pro".to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: Some(json!("Be concise")),
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
        };

        let body = GeminiProvider::openai_to_gemini_body(&req);
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "Be concise");
        // System should not be in contents
        assert_eq!(body["contents"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_openai_to_gemini_assistant_becomes_model_role() {
        let req = ChatCompletionRequest {
            model: "gemini-2.5-pro".to_string(),
            messages: vec![
                ChatMessage {
                    role: "user".to_string(),
                    content: Some(json!("Hi")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: "assistant".to_string(),
                    content: Some(json!("Hello!")),
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
        };

        let body = GeminiProvider::openai_to_gemini_body(&req);
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents[1]["role"], "model");
    }

    #[test]
    fn test_anthropic_to_gemini_body_basic() {
        let req = AnthropicMessagesRequest {
            model: "gemini-2.5-pro".to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: json!("Hello"),
            }],
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
        };

        let body = GeminiProvider::anthropic_to_gemini_body(&req);
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "Hello");
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 500);
    }
}
