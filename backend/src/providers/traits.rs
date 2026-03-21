use std::any::Any;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::ApiError;
use crate::models::anthropic::AnthropicMessagesRequest;
use crate::models::openai::ChatCompletionRequest;
use crate::providers::types::{
    ProviderContext, ProviderId, ProviderResponse, ProviderStreamResponse,
};

/// Trait implemented by each AI provider backend.
///
/// Every provider must be able to handle both OpenAI-format and Anthropic-format inputs.
/// Cross-format conversion is the responsibility of the provider implementation.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Downcast to concrete type for accessing provider-specific state.
    fn as_any(&self) -> &dyn Any;
    /// The provider identifier.
    #[allow(dead_code)]
    fn id(&self) -> ProviderId;

    /// Execute a non-streaming OpenAI-format request.
    async fn execute_openai(
        &self,
        ctx: &ProviderContext<'_>,
        req: &ChatCompletionRequest,
    ) -> Result<ProviderResponse, ApiError>;

    /// Execute a streaming OpenAI-format request.
    ///
    /// Returns a `ProviderStreamResponse` containing the initial response headers
    /// and the streaming body, allowing callers to extract rate-limit signals.
    async fn stream_openai(
        &self,
        ctx: &ProviderContext<'_>,
        req: &ChatCompletionRequest,
    ) -> Result<ProviderStreamResponse, ApiError>;

    /// Execute a non-streaming Anthropic-format request.
    async fn execute_anthropic(
        &self,
        ctx: &ProviderContext<'_>,
        req: &AnthropicMessagesRequest,
    ) -> Result<ProviderResponse, ApiError>;

    /// Execute a streaming Anthropic-format request.
    ///
    /// Returns a `ProviderStreamResponse` containing the initial response headers
    /// and the streaming body, allowing callers to extract rate-limit signals.
    async fn stream_anthropic(
        &self,
        ctx: &ProviderContext<'_>,
        req: &AnthropicMessagesRequest,
    ) -> Result<ProviderStreamResponse, ApiError>;

    /// Normalize a non-streaming response for the OpenAI endpoint.
    ///
    /// Called after `execute_openai()`. Providers whose native format isn't OpenAI
    /// (e.g. Anthropic) override this to convert their response body.
    /// Default: identity (response is already OpenAI format).
    fn normalize_response_for_openai(&self, _model: &str, body: Value) -> Value {
        body
    }

    /// Normalize a non-streaming response for the Anthropic endpoint.
    ///
    /// Called after `execute_anthropic()`. Providers whose native format isn't Anthropic
    /// (e.g. OpenAI, Copilot) override this to convert their response body.
    /// Default: identity (response is already Anthropic format).
    fn normalize_response_for_anthropic(&self, _model: &str, body: Value) -> Value {
        body
    }
}
