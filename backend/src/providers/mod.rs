use std::collections::HashMap;
use std::sync::Arc;

use crate::providers::traits::Provider;
use crate::providers::types::ProviderId;

pub mod anthropic;
pub mod copilot;
pub mod custom;
pub mod kiro;
pub mod known_models;
pub mod openai_codex;
pub mod qwen;
pub mod rate_limiter;
pub mod registry;
pub mod traits;
pub mod types;

/// Immutable map of provider ID → provider implementation, built once at startup.
pub type ProviderMap = Arc<HashMap<ProviderId, Arc<dyn Provider>>>;

/// Build the provider map with all providers including Kiro.
pub fn build_provider_map(
    http_client: Arc<crate::http_client::KiroHttpClient>,
    auth_manager: Arc<tokio::sync::RwLock<crate::auth::AuthManager>>,
    config: Arc<std::sync::RwLock<crate::config::Config>>,
) -> ProviderMap {
    let mut map = HashMap::new();
    map.insert(
        ProviderId::Kiro,
        Arc::new(kiro::KiroProvider::new(http_client, auth_manager, config)) as Arc<dyn Provider>,
    );
    map.insert(
        ProviderId::Anthropic,
        Arc::new(anthropic::AnthropicProvider::new()) as Arc<dyn Provider>,
    );
    map.insert(
        ProviderId::OpenAICodex,
        Arc::new(openai_codex::OpenAICodexProvider::new()) as Arc<dyn Provider>,
    );
    map.insert(
        ProviderId::Copilot,
        Arc::new(copilot::CopilotProvider::new()) as Arc<dyn Provider>,
    );
    map.insert(
        ProviderId::Qwen,
        Arc::new(qwen::QwenProvider::new()) as Arc<dyn Provider>,
    );
    map.insert(
        ProviderId::Custom,
        Arc::new(custom::CustomProvider::new()) as Arc<dyn Provider>,
    );
    Arc::new(map)
}
