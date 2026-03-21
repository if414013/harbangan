use std::collections::HashMap;
use std::sync::Arc;

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

/// Convert Anthropic messages format to OpenAI chat completions format.
///
/// Shared by all OpenAI-compatible providers (OpenAICodex, Copilot, Custom).
pub fn anthropic_to_openai_body(req: &AnthropicMessagesRequest) -> Value {
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
        ProviderId::Custom,
        Arc::new(custom::CustomProvider::new()) as Arc<dyn Provider>,
    );
    Arc::new(map)
}
