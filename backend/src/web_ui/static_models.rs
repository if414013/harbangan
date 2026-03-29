use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use crate::web_ui::config_db::RegistryModel;
use crate::web_ui::model_registry::generate_prefixed_id;

fn model(provider: &str, id: &str, display: &str, ctx: i32, out: i32) -> RegistryModel {
    let now = Utc::now();
    RegistryModel {
        id: Uuid::new_v4(),
        provider_id: provider.to_string(),
        model_id: id.to_string(),
        display_name: display.to_string(),
        prefixed_id: generate_prefixed_id(provider, id),
        context_length: ctx,
        max_output_tokens: out,
        capabilities: json!({}),
        enabled: false,
        source: "static".to_string(),
        upstream_meta: None,
        created_at: now,
        updated_at: now,
    }
}

/// Static Anthropic model definitions (fallback when API call fails).
pub fn static_anthropic_models() -> Vec<RegistryModel> {
    vec![
        model(
            "anthropic",
            "claude-haiku-4-5-20251001",
            "Claude 4.5 Haiku",
            200_000,
            64_000,
        ),
        model(
            "anthropic",
            "claude-sonnet-4-5-20250929",
            "Claude 4.5 Sonnet",
            200_000,
            64_000,
        ),
        model(
            "anthropic",
            "claude-sonnet-4-6",
            "Claude 4.6 Sonnet",
            200_000,
            64_000,
        ),
        model(
            "anthropic",
            "claude-opus-4-6",
            "Claude 4.6 Opus",
            1_000_000,
            128_000,
        ),
        model(
            "anthropic",
            "claude-opus-4-5-20251101",
            "Claude 4.5 Opus",
            200_000,
            64_000,
        ),
        model(
            "anthropic",
            "claude-opus-4-1-20250805",
            "Claude 4.1 Opus",
            200_000,
            32_000,
        ),
        model(
            "anthropic",
            "claude-opus-4-20250514",
            "Claude 4 Opus",
            200_000,
            32_000,
        ),
        model(
            "anthropic",
            "claude-sonnet-4-20250514",
            "Claude 4 Sonnet",
            200_000,
            64_000,
        ),
        model(
            "anthropic",
            "claude-3-7-sonnet-20250219",
            "Claude 3.7 Sonnet",
            128_000,
            8_192,
        ),
        model(
            "anthropic",
            "claude-3-5-haiku-20241022",
            "Claude 3.5 Haiku",
            128_000,
            8_192,
        ),
    ]
}

/// Static OpenAI Codex model definitions (fallback when API call fails).
pub fn static_openai_codex_models() -> Vec<RegistryModel> {
    vec![
        model("openai_codex", "gpt-5", "GPT 5", 400_000, 128_000),
        model(
            "openai_codex",
            "gpt-5-codex",
            "GPT 5 Codex",
            400_000,
            128_000,
        ),
        model(
            "openai_codex",
            "gpt-5-codex-mini",
            "GPT 5 Codex Mini",
            400_000,
            128_000,
        ),
        model("openai_codex", "gpt-5.1", "GPT 5.1", 400_000, 128_000),
        model(
            "openai_codex",
            "gpt-5.1-codex",
            "GPT 5.1 Codex",
            400_000,
            128_000,
        ),
        model(
            "openai_codex",
            "gpt-5.1-codex-mini",
            "GPT 5.1 Codex Mini",
            400_000,
            128_000,
        ),
        model(
            "openai_codex",
            "gpt-5.1-codex-max",
            "GPT 5.1 Codex Max",
            400_000,
            128_000,
        ),
        model("openai_codex", "gpt-5.2", "GPT 5.2", 400_000, 128_000),
        model(
            "openai_codex",
            "gpt-5.2-codex",
            "GPT 5.2 Codex",
            400_000,
            128_000,
        ),
        model(
            "openai_codex",
            "gpt-5.3-codex",
            "GPT 5.3 Codex",
            400_000,
            128_000,
        ),
        model(
            "openai_codex",
            "gpt-5.3-codex-spark",
            "GPT 5.3 Codex Spark",
            128_000,
            128_000,
        ),
        model("openai_codex", "gpt-5.4", "GPT 5.4", 1_050_000, 128_000),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_anthropic_models_not_empty() {
        let models = static_anthropic_models();
        assert!(!models.is_empty());
        assert!(models.len() >= 10);
        for m in &models {
            assert_eq!(m.provider_id, "anthropic");
            assert_eq!(m.source, "static");
            assert!(!m.enabled);
            assert!(m.context_length > 0);
            assert!(m.prefixed_id.starts_with("anthropic/"));
        }
    }

    #[test]
    fn test_static_openai_codex_models_not_empty() {
        let models = static_openai_codex_models();
        assert!(!models.is_empty());
        assert!(models.len() >= 10);
        for m in &models {
            assert_eq!(m.provider_id, "openai_codex");
            assert_eq!(m.source, "static");
            assert!(!m.enabled);
            assert!(m.context_length > 0);
            assert!(m.prefixed_id.starts_with("openai_codex/"));
        }
    }

    #[test]
    fn test_static_models_unique_ids() {
        let anthropic = static_anthropic_models();
        let codex = static_openai_codex_models();
        let mut seen = std::collections::HashSet::new();
        for m in anthropic.iter().chain(codex.iter()) {
            assert!(seen.insert(&m.prefixed_id), "duplicate: {}", m.prefixed_id);
        }
    }
}
