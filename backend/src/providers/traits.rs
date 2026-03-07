use std::pin::Pin;

use async_trait::async_trait;
use futures::stream::Stream;

use crate::error::ApiError;
use crate::models::anthropic::AnthropicMessagesRequest;
use crate::models::openai::ChatCompletionRequest;
use crate::providers::types::{ProviderContext, ProviderId, ProviderResponse, ProviderStreamItem};

/// Trait implemented by each AI provider backend.
///
/// Every provider must be able to handle both OpenAI-format and Anthropic-format inputs.
/// Cross-format conversion is the responsibility of the provider implementation.
#[async_trait]
#[allow(dead_code)]
pub trait Provider: Send + Sync {
    /// The provider identifier.
    fn id(&self) -> ProviderId;

    /// Execute a non-streaming OpenAI-format request.
    async fn execute_openai(
        &self,
        ctx: &ProviderContext<'_>,
        req: &ChatCompletionRequest,
    ) -> Result<ProviderResponse, ApiError>;

    /// Execute a streaming OpenAI-format request.
    async fn stream_openai(
        &self,
        ctx: &ProviderContext<'_>,
        req: &ChatCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>>, ApiError>;

    /// Execute a non-streaming Anthropic-format request.
    async fn execute_anthropic(
        &self,
        ctx: &ProviderContext<'_>,
        req: &AnthropicMessagesRequest,
    ) -> Result<ProviderResponse, ApiError>;

    /// Execute a streaming Anthropic-format request.
    async fn stream_anthropic(
        &self,
        ctx: &ProviderContext<'_>,
        req: &AnthropicMessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>>, ApiError>;
}
