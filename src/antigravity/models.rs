//! Antigravity model definitions and registration.
//!
//! Defines the set of models available through the Cloud Code backend
//! and provides helpers to register them in the shared model cache.

use crate::cache::ModelCache;

/// All models available through the Antigravity (Cloud Code) backend.
pub const ANTIGRAVITY_MODELS: &[AntigravityModel] = &[
    AntigravityModel {
        id: "claude-opus-4-6-thinking",
        display_name: "Claude Opus 4.6 (Thinking)",
        max_input_tokens: 200_000,
    },
    AntigravityModel {
        id: "claude-sonnet-4-5-thinking",
        display_name: "Claude Sonnet 4.5 (Thinking)",
        max_input_tokens: 200_000,
    },
    AntigravityModel {
        id: "claude-sonnet-4-5",
        display_name: "Claude Sonnet 4.5",
        max_input_tokens: 200_000,
    },
    AntigravityModel {
        id: "gemini-3-flash",
        display_name: "Gemini 3 Flash",
        max_input_tokens: 1_000_000,
    },
    AntigravityModel {
        id: "gemini-3-pro-low",
        display_name: "Gemini 3 Pro (Low)",
        max_input_tokens: 1_000_000,
    },
    AntigravityModel {
        id: "gemini-3-pro-high",
        display_name: "Gemini 3 Pro (High)",
        max_input_tokens: 1_000_000,
    },
];

/// Definition of a single Antigravity model.
#[derive(Debug, Clone)]
pub struct AntigravityModel {
    /// Model identifier used in API requests.
    pub id: &'static str,
    /// Human-readable display name.
    pub display_name: &'static str,
    /// Maximum input token limit.
    pub max_input_tokens: i64,
}

/// Returns true if the given model ID is an Antigravity model.
pub fn is_antigravity_model(model_id: &str) -> bool {
    ANTIGRAVITY_MODELS.iter().any(|m| m.id == model_id)
}

/// Returns all antigravity model IDs as a Vec.
pub fn antigravity_model_ids() -> Vec<&'static str> {
    ANTIGRAVITY_MODELS.iter().map(|m| m.id).collect()
}

/// Registers all Antigravity models into the shared model cache.
///
/// Models are added as hidden models so they appear in `/v1/models`
/// but use their own ID as the internal ID (routed to Cloud Code, not Kiro).
pub fn register_antigravity_models(cache: &ModelCache) {
    for model in ANTIGRAVITY_MODELS {
        // Use add_hidden_model so it doesn't overwrite if already present
        // from the Kiro API response.
        if !cache.is_valid_model(model.id) {
            cache.add_hidden_model(model.id, model.id);
            tracing::debug!(model = model.id, "Registered antigravity model");
        }
    }

    tracing::info!(
        count = ANTIGRAVITY_MODELS.len(),
        "Registered antigravity models in cache"
    );
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_antigravity_models_count() {
        assert_eq!(ANTIGRAVITY_MODELS.len(), 6);
    }

    #[test]
    fn test_is_antigravity_model() {
        assert!(is_antigravity_model("claude-opus-4-6-thinking"));
        assert!(is_antigravity_model("claude-sonnet-4-5-thinking"));
        assert!(is_antigravity_model("claude-sonnet-4-5"));
        assert!(is_antigravity_model("gemini-3-flash"));
        assert!(is_antigravity_model("gemini-3-pro-low"));
        assert!(is_antigravity_model("gemini-3-pro-high"));
    }

    #[test]
    fn test_is_not_antigravity_model() {
        assert!(!is_antigravity_model("claude-sonnet-4"));
        assert!(!is_antigravity_model("claude-haiku-4.5"));
        assert!(!is_antigravity_model("gpt-4"));
        assert!(!is_antigravity_model(""));
    }

    #[test]
    fn test_antigravity_model_ids() {
        let ids = antigravity_model_ids();
        assert_eq!(ids.len(), 6);
        assert!(ids.contains(&"claude-opus-4-6-thinking"));
        assert!(ids.contains(&"gemini-3-flash"));
    }

    #[test]
    fn test_register_antigravity_models() {
        let cache = ModelCache::new(3600);
        register_antigravity_models(&cache);

        for model in ANTIGRAVITY_MODELS {
            assert!(
                cache.is_valid_model(model.id),
                "Model {} should be in cache",
                model.id
            );
        }
    }

    #[test]
    fn test_register_does_not_overwrite_existing() {
        let cache = ModelCache::new(3600);

        // Pre-populate one model
        cache.add_hidden_model("gemini-3-flash", "CUSTOM_ID");

        register_antigravity_models(&cache);

        // The pre-existing entry should still be there
        assert!(cache.is_valid_model("gemini-3-flash"));
        // Other models should also be registered
        assert!(cache.is_valid_model("claude-opus-4-6-thinking"));
    }
}
