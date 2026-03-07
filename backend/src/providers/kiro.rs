/// KiroProvider — wraps the existing Kiro API pipeline.
///
/// This is the default provider when no matching user provider key exists.
/// It preserves all existing behavior: converter → Kiro API → AWS Event Stream.
///
/// Phase 4 will wire this into the request flow. For now this provides the
/// structural implementation needed to satisfy the Provider trait.
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::Stream;

use crate::error::ApiError;
use crate::models::anthropic::AnthropicMessagesRequest;
use crate::models::openai::ChatCompletionRequest;
use crate::providers::traits::Provider;
use crate::providers::types::{ProviderContext, ProviderId, ProviderResponse, ProviderStreamItem};

pub struct KiroProvider {
    http_client: Arc<crate::http_client::KiroHttpClient>,
}

impl KiroProvider {
    pub fn new(http_client: Arc<crate::http_client::KiroHttpClient>) -> Self {
        Self { http_client }
    }

    pub fn http_client(&self) -> &Arc<crate::http_client::KiroHttpClient> {
        &self.http_client
    }
}

#[async_trait]
impl Provider for KiroProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Kiro
    }

    /// Execute an OpenAI-format request via the Kiro pipeline.
    ///
    /// Phase 4 wires this to the full pipeline (build_kiro_payload → Kiro API →
    /// AWS Event Stream → kiro_to_openai). For now returns an internal error
    /// since it requires full AppState context that Phase 4 will supply.
    async fn execute_openai(
        &self,
        _ctx: &ProviderContext<'_>,
        _req: &ChatCompletionRequest,
    ) -> Result<ProviderResponse, ApiError> {
        // Phase 4 refactors the handler to call this method with full context.
        // The existing routes/mod.rs handler remains the active path until Phase 4.
        Err(ApiError::Internal(anyhow::anyhow!(
            "KiroProvider::execute_openai requires Phase 4 integration"
        )))
    }

    /// Stream an OpenAI-format request via the Kiro pipeline.
    async fn stream_openai(
        &self,
        _ctx: &ProviderContext<'_>,
        _req: &ChatCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>>, ApiError> {
        Err(ApiError::Internal(anyhow::anyhow!(
            "KiroProvider::stream_openai requires Phase 4 integration"
        )))
    }

    /// Execute an Anthropic-format request via the Kiro pipeline.
    async fn execute_anthropic(
        &self,
        _ctx: &ProviderContext<'_>,
        _req: &AnthropicMessagesRequest,
    ) -> Result<ProviderResponse, ApiError> {
        Err(ApiError::Internal(anyhow::anyhow!(
            "KiroProvider::execute_anthropic requires Phase 4 integration"
        )))
    }

    /// Stream an Anthropic-format request via the Kiro pipeline.
    async fn stream_anthropic(
        &self,
        _ctx: &ProviderContext<'_>,
        _req: &AnthropicMessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>>, ApiError> {
        Err(ApiError::Internal(anyhow::anyhow!(
            "KiroProvider::stream_anthropic requires Phase 4 integration"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_client::KiroHttpClient;

    fn make_kiro_provider() -> KiroProvider {
        // Use default connection settings for tests
        let client = KiroHttpClient::new(10, 30, 300, 3).expect("KiroHttpClient::new");
        KiroProvider::new(Arc::new(client))
    }

    #[test]
    fn test_kiro_provider_id() {
        let provider = make_kiro_provider();
        assert_eq!(provider.id(), ProviderId::Kiro);
    }

    #[test]
    fn test_kiro_provider_holds_http_client() {
        let client = Arc::new(KiroHttpClient::new(10, 30, 300, 3).expect("KiroHttpClient::new"));
        let provider = KiroProvider::new(Arc::clone(&client));
        // Verify the Arc is correctly held (strong_count includes both client + provider)
        assert!(Arc::strong_count(provider.http_client()) >= 1);
    }

    #[tokio::test]
    async fn test_kiro_provider_execute_openai_returns_not_yet_integrated() {
        use crate::models::openai::{ChatCompletionRequest, ChatMessage};
        use crate::providers::types::{ProviderContext, ProviderCredentials};

        let provider = make_kiro_provider();
        let creds = ProviderCredentials {
            provider: ProviderId::Kiro,
            access_token: "test-token".to_string(),
            base_url: None,
        };
        let req = ChatCompletionRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some(serde_json::json!("Hello")),
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
        let model = "claude-sonnet-4".to_string();
        let ctx = ProviderContext {
            credentials: &creds,
            model: &model,
        };
        let result = provider.execute_openai(&ctx, &req).await;
        assert!(result.is_err());
        if let Err(ApiError::Internal(e)) = result {
            assert!(e.to_string().contains("Phase 4"));
        } else {
            panic!("Expected ApiError::Internal");
        }
    }
}
