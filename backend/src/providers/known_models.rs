/// Static known model lists per provider for proxy mode.
///
/// In proxy mode there's no DB to query for available models, so we expose
/// a curated list of well-known models for each configured provider.
/// Users can always use explicit prefix notation (e.g. `anthropic/claude-opus-4-6`)
/// for unlisted models.
use crate::providers::types::ProviderId;

pub fn known_models_for_provider(provider: &ProviderId) -> &'static [&'static str] {
    match provider {
        ProviderId::Anthropic => &[
            "claude-opus-4-6",
            "claude-sonnet-4-6",
            "claude-haiku-4-5",
            "claude-sonnet-4-5-20250514",
        ],
        ProviderId::OpenAICodex => &[
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4.1",
            "gpt-4.1-mini",
            "gpt-4.1-nano",
            "o3",
            "o3-mini",
            "o4-mini",
        ],
        ProviderId::Copilot => &["claude-sonnet-4", "gpt-4o", "o3-mini"],
        ProviderId::Qwen => &["qwen-coder-plus", "qwen3-coder"],
        ProviderId::Custom | ProviderId::Kiro => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_models_anthropic() {
        let models = known_models_for_provider(&ProviderId::Anthropic);
        assert!(!models.is_empty());
        assert!(models.contains(&"claude-opus-4-6"));
        assert!(models.contains(&"claude-sonnet-4-6"));
    }

    #[test]
    fn test_known_models_openai() {
        let models = known_models_for_provider(&ProviderId::OpenAICodex);
        assert!(!models.is_empty());
        assert!(models.contains(&"gpt-4o"));
        assert!(models.contains(&"o3"));
    }

    #[test]
    fn test_known_models_copilot() {
        let models = known_models_for_provider(&ProviderId::Copilot);
        assert!(!models.is_empty());
    }

    #[test]
    fn test_known_models_qwen() {
        let models = known_models_for_provider(&ProviderId::Qwen);
        assert!(!models.is_empty());
    }

    #[test]
    fn test_known_models_custom_empty() {
        let models = known_models_for_provider(&ProviderId::Custom);
        assert!(models.is_empty());
    }

    #[test]
    fn test_known_models_kiro_empty() {
        let models = known_models_for_provider(&ProviderId::Kiro);
        assert!(models.is_empty());
    }
}
