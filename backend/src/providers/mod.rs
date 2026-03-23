use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::models::anthropic::AnthropicMessagesRequest;
use crate::providers::traits::Provider;
use crate::providers::types::ProviderId;

pub mod anthropic;
pub mod copilot;
pub mod custom;
pub mod kiro;
pub mod openai_codex;
pub mod rate_limiter;
pub mod registry;
pub mod traits;
pub mod types;

/// Convert Anthropic messages format to OpenAI chat completions body (as JSON Value).
///
/// Shared by all OpenAI-compatible providers (OpenAICodex, Copilot, Custom).
/// Delegates to the full `converters::anthropic_to_openai` converter which handles
/// tools, tool_use/tool_result content blocks, and all field mappings.
pub fn anthropic_to_openai_body(req: &AnthropicMessagesRequest) -> Value {
    let openai_req = crate::converters::anthropic_to_openai::anthropic_to_openai(req);
    serde_json::to_value(&openai_req).unwrap_or_else(|_| json!({}))
}

/// Immutable map of provider ID → provider implementation, built once at startup.
pub type ProviderMap = Arc<HashMap<ProviderId, Arc<dyn Provider>>>;

/// Build the provider map with all providers including Kiro.
///
/// Creates a shared `reqwest::Client` with connection pool and timeout settings
/// from the config, then passes it to each direct provider. This avoids each
/// provider creating its own client with separate connection pools.
pub fn build_provider_map(
    http_client: Arc<crate::http_client::KiroHttpClient>,
    auth_manager: Arc<tokio::sync::RwLock<crate::auth::AuthManager>>,
    config: Arc<std::sync::RwLock<crate::config::Config>>,
) -> ProviderMap {
    // Build shared reqwest::Client using config values
    let shared_client = {
        let cfg = config.read().unwrap_or_else(|p| p.into_inner());
        reqwest::Client::builder()
            .pool_max_idle_per_host(cfg.http_max_connections)
            .connect_timeout(Duration::from_secs(cfg.http_connect_timeout))
            .timeout(Duration::from_secs(cfg.http_request_timeout))
            .build()
            .expect("Failed to build shared HTTP client")
    };

    let mut map = HashMap::new();
    map.insert(
        ProviderId::Kiro,
        Arc::new(kiro::KiroProvider::new(http_client, auth_manager, config)) as Arc<dyn Provider>,
    );
    map.insert(
        ProviderId::Anthropic,
        Arc::new(anthropic::AnthropicProvider::new(shared_client.clone())) as Arc<dyn Provider>,
    );
    map.insert(
        ProviderId::OpenAICodex,
        Arc::new(openai_codex::OpenAICodexProvider::new(
            shared_client.clone(),
        )) as Arc<dyn Provider>,
    );
    map.insert(
        ProviderId::Copilot,
        Arc::new(copilot::CopilotProvider::new(shared_client.clone())) as Arc<dyn Provider>,
    );
    map.insert(
        ProviderId::Custom,
        Arc::new(custom::CustomProvider::new(shared_client)) as Arc<dyn Provider>,
    );
    Arc::new(map)
}
