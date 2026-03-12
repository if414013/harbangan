/// KiroProvider — wraps the existing Kiro API pipeline.
///
/// This is the default provider when no matching user provider key exists.
/// It preserves all existing behavior: converter → Kiro API → AWS Event Stream.
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::Value;
use uuid::Uuid;

use crate::auth::AuthManager;
use crate::config::Config;
use crate::converters::anthropic_to_kiro::build_kiro_payload as build_kiro_payload_anthropic;
use crate::converters::openai_to_kiro::build_kiro_payload;
use crate::error::ApiError;
use crate::models::anthropic::AnthropicMessagesRequest;
use crate::models::openai::ChatCompletionRequest;
use crate::providers::traits::Provider;
use crate::providers::types::{ProviderContext, ProviderId, ProviderResponse, ProviderStreamItem};
use crate::tokenizer::{count_anthropic_message_tokens, count_message_tokens, count_tools_tokens};

pub struct KiroProvider {
    http_client: Arc<crate::http_client::KiroHttpClient>,
    auth_manager: Arc<tokio::sync::RwLock<AuthManager>>,
    config: Arc<std::sync::RwLock<Config>>,
}

impl KiroProvider {
    pub fn new(
        http_client: Arc<crate::http_client::KiroHttpClient>,
        auth_manager: Arc<tokio::sync::RwLock<AuthManager>>,
        config: Arc<std::sync::RwLock<Config>>,
    ) -> Self {
        Self {
            http_client,
            auth_manager,
            config,
        }
    }

    pub fn http_client(&self) -> &Arc<crate::http_client::KiroHttpClient> {
        &self.http_client
    }

    fn read_config(&self) -> Config {
        self.config
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Send a Kiro API request using the access token from ProviderContext.
    async fn send_request(
        &self,
        ctx: &ProviderContext<'_>,
        kiro_payload: &Value,
    ) -> Result<reqwest::Response, ApiError> {
        let kiro_api_url = ctx
            .credentials
            .base_url
            .as_deref()
            .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Missing Kiro API URL")))?;

        let req = self
            .http_client
            .client()
            .post(kiro_api_url)
            .header(
                "Authorization",
                format!("Bearer {}", ctx.credentials.access_token),
            )
            .header("Content-Type", "application/json")
            .json(kiro_payload)
            .build()
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to build request: {}", e)))?;

        self.http_client.request_with_retry(req).await
    }

    /// Build Kiro pipeline context: conversation ID, profile ARN, config snapshot.
    async fn pipeline_context(&self) -> Result<(String, String, Config), ApiError> {
        let conversation_id = Uuid::new_v4().to_string();
        let auth = self.auth_manager.read().await;
        let profile_arn = auth.get_profile_arn().await.unwrap_or_default();
        drop(auth);
        let config = self.read_config();
        Ok((conversation_id, profile_arn, config))
    }
}

#[async_trait]
impl Provider for KiroProvider {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn id(&self) -> ProviderId {
        ProviderId::Kiro
    }

    async fn execute_openai(
        &self,
        ctx: &ProviderContext<'_>,
        req: &ChatCompletionRequest,
    ) -> Result<ProviderResponse, ApiError> {
        let (conversation_id, profile_arn, config) = self.pipeline_context().await?;

        let kiro_result = build_kiro_payload(req, &conversation_id, &profile_arn, &config)
            .map_err(ApiError::ValidationError)?;

        let response = self.send_request(ctx, &kiro_result.payload).await?;

        let input_tokens = count_message_tokens(&req.messages, false)
            + count_tools_tokens(req.tools.as_ref(), false);

        let body = crate::streaming::collect_openai_response(
            response,
            &req.model,
            config.first_token_timeout,
            input_tokens,
            config.truncation_recovery,
        )
        .await?;

        Ok(ProviderResponse { status: 200, body })
    }

    async fn stream_openai(
        &self,
        ctx: &ProviderContext<'_>,
        req: &ChatCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>>, ApiError> {
        let (conversation_id, profile_arn, config) = self.pipeline_context().await?;

        let kiro_result = build_kiro_payload(req, &conversation_id, &profile_arn, &config)
            .map_err(ApiError::ValidationError)?;

        let response = self.send_request(ctx, &kiro_result.payload).await?;

        let input_tokens = count_message_tokens(&req.messages, false)
            + count_tools_tokens(req.tools.as_ref(), false);

        let include_usage = req
            .stream_options
            .as_ref()
            .and_then(|opts| opts.get("include_usage"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let sse_stream = crate::streaming::stream_kiro_to_openai(
            response,
            &req.model,
            15,
            input_tokens,
            None,
            include_usage,
            config.truncation_recovery,
        )
        .await?;

        // Convert String stream to Bytes stream
        let byte_stream = sse_stream.map(|r| r.map(Bytes::from));
        Ok(Box::pin(byte_stream))
    }

    async fn execute_anthropic(
        &self,
        ctx: &ProviderContext<'_>,
        req: &AnthropicMessagesRequest,
    ) -> Result<ProviderResponse, ApiError> {
        let (conversation_id, profile_arn, config) = self.pipeline_context().await?;

        let kiro_result =
            build_kiro_payload_anthropic(req, &conversation_id, &profile_arn, &config)
                .map_err(ApiError::ValidationError)?;

        let response = self.send_request(ctx, &kiro_result.payload).await?;

        let input_tokens =
            count_anthropic_message_tokens(&req.messages, req.system.as_ref(), req.tools.as_ref());

        let body = crate::streaming::collect_anthropic_response(
            response,
            &req.model,
            config.first_token_timeout,
            input_tokens,
            config.truncation_recovery,
        )
        .await?;

        Ok(ProviderResponse { status: 200, body })
    }

    async fn stream_anthropic(
        &self,
        ctx: &ProviderContext<'_>,
        req: &AnthropicMessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>>, ApiError> {
        let (conversation_id, profile_arn, config) = self.pipeline_context().await?;

        let kiro_result =
            build_kiro_payload_anthropic(req, &conversation_id, &profile_arn, &config)
                .map_err(ApiError::ValidationError)?;

        let response = self.send_request(ctx, &kiro_result.payload).await?;

        let input_tokens =
            count_anthropic_message_tokens(&req.messages, req.system.as_ref(), req.tools.as_ref());

        let sse_stream = crate::streaming::stream_kiro_to_anthropic(
            response,
            &req.model,
            config.first_token_timeout,
            input_tokens,
            None,
            config.truncation_recovery,
        )
        .await?;

        let byte_stream = sse_stream.map(|r| r.map(Bytes::from));
        Ok(Box::pin(byte_stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_client::KiroHttpClient;

    fn make_kiro_provider() -> KiroProvider {
        let client = KiroHttpClient::new(10, 30, 300, 3).expect("KiroHttpClient::new");
        let auth_manager = Arc::new(tokio::sync::RwLock::new(
            AuthManager::new_for_testing("test-token".to_string(), "us-east-1".to_string(), 300)
                .unwrap(),
        ));
        let config = Arc::new(std::sync::RwLock::new(Config::with_defaults()));
        KiroProvider::new(Arc::new(client), auth_manager, config)
    }

    #[test]
    fn test_kiro_provider_id() {
        let provider = make_kiro_provider();
        assert_eq!(provider.id(), ProviderId::Kiro);
    }

    #[test]
    fn test_kiro_provider_holds_http_client() {
        let client = Arc::new(KiroHttpClient::new(10, 30, 300, 3).expect("KiroHttpClient::new"));
        let auth_manager = Arc::new(tokio::sync::RwLock::new(
            AuthManager::new_for_testing("test-token".to_string(), "us-east-1".to_string(), 300)
                .unwrap(),
        ));
        let config = Arc::new(std::sync::RwLock::new(Config::with_defaults()));
        let provider = KiroProvider::new(Arc::clone(&client), auth_manager, config);
        assert!(Arc::strong_count(provider.http_client()) >= 1);
    }

    #[test]
    fn test_kiro_provider_read_config() {
        let provider = make_kiro_provider();
        let config = provider.read_config();
        // Verify we can read config without panic
        assert!(config.first_token_timeout > 0);
    }
}
