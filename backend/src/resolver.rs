use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;

use crate::cache::ModelCache;

// Regex patterns for model name normalization
static STANDARD_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(claude-(?:haiku|sonnet|opus)-\d+)-(\d{1,2})(?:-(?:\d{8}|latest|\d+))?$").unwrap()
});

static NO_MINOR_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(claude-(?:haiku|sonnet|opus)-\d+)(?:-\d{8})?$").unwrap());

static LEGACY_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(claude)-(\d+)-(\d+)-(haiku|sonnet|opus)(?:-(?:\d{8}|latest|\d+))?$").unwrap()
});

static DOT_WITH_DATE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(claude-(?:\d+\.\d+-)?(?:haiku|sonnet|opus)(?:-\d+\.\d+)?)-\d{8}$").unwrap()
});

/// Normalize model name to Kiro format
///
/// Transformations:
/// - claude-haiku-4-5 → claude-haiku-4.5 (dash to dot for minor version)
/// - claude-haiku-4-5-20251001 → claude-haiku-4.5 (strip date suffix)
/// - claude-haiku-4-5-latest → claude-haiku-4.5 (strip 'latest' suffix)
/// - claude-sonnet-4-20250514 → claude-sonnet-4 (strip date, no minor)
/// - claude-3-7-sonnet → claude-3.7-sonnet (legacy format normalization)
/// - claude-3-7-sonnet-20250219 → claude-3.7-sonnet (legacy + strip date)
pub fn normalize_model_name(name: &str) -> String {
    if name.is_empty() {
        return name.to_string();
    }

    let name_lower = name.to_lowercase();

    // Pattern 1: Standard format - claude-{family}-{major}-{minor}(-{suffix})?
    if let Some(caps) = STANDARD_PATTERN.captures(&name_lower) {
        let base = caps.get(1).unwrap().as_str();
        let minor = caps.get(2).unwrap().as_str();
        return format!("{}.{}", base, minor);
    }

    // Pattern 2: Standard format without minor - claude-{family}-{major}(-{date})?
    if let Some(caps) = NO_MINOR_PATTERN.captures(&name_lower) {
        return caps.get(1).unwrap().as_str().to_string();
    }

    // Pattern 3: Legacy format - claude-{major}-{minor}-{family}(-{suffix})?
    if let Some(caps) = LEGACY_PATTERN.captures(&name_lower) {
        let prefix = caps.get(1).unwrap().as_str();
        let major = caps.get(2).unwrap().as_str();
        let minor = caps.get(3).unwrap().as_str();
        let family = caps.get(4).unwrap().as_str();
        return format!("{}-{}.{}-{}", prefix, major, minor, family);
    }

    // Pattern 4: Already normalized with dot but has date suffix
    if let Some(caps) = DOT_WITH_DATE_PATTERN.captures(&name_lower) {
        return caps.get(1).unwrap().as_str().to_string();
    }

    // No transformation needed - return as-is (preserving original case)
    name.to_string()
}

/// Extract model family from model name
#[allow(dead_code)]
pub fn extract_model_family(model_name: &str) -> Option<String> {
    let family_regex = Regex::new(r"(haiku|sonnet|opus)").unwrap();
    family_regex
        .find(&model_name.to_lowercase())
        .map(|m| m.as_str().to_string())
}

/// Model resolver with caching
pub struct ModelResolver {
    /// Model cache
    cache: ModelCache,

    /// Hidden models mapping (display name → internal ID)
    hidden_models: Arc<HashMap<String, String>>,
}

impl ModelResolver {
    /// Create a new model resolver
    pub fn new(cache: ModelCache, hidden_models: HashMap<String, String>) -> Self {
        Self {
            cache,
            hidden_models: Arc::new(hidden_models),
        }
    }

    /// Resolve a model name to the internal Kiro ID
    ///
    /// Resolution pipeline:
    /// 1. Normalize the name
    /// 2. Check hidden models
    /// 3. Check dynamic cache
    /// 4. Pass-through (let Kiro decide)
    pub fn resolve(&self, model_name: &str) -> ModelResolution {
        let normalized = normalize_model_name(model_name);

        // Check hidden models first
        if let Some(internal_id) = self.hidden_models.get(&normalized) {
            return ModelResolution {
                internal_id: internal_id.clone(),
                source: "hidden".to_string(),
                original_request: model_name.to_string(),
                normalized: normalized.clone(),
                is_verified: true,
            };
        }

        // Check dynamic cache
        if self.cache.is_valid_model(&normalized) {
            return ModelResolution {
                internal_id: normalized.clone(),
                source: "cache".to_string(),
                original_request: model_name.to_string(),
                normalized: normalized.clone(),
                is_verified: true,
            };
        }

        // Pass-through - let Kiro decide
        tracing::debug!(
            "Model '{}' not found in cache or hidden models, passing through as '{}'",
            model_name,
            normalized
        );

        ModelResolution {
            internal_id: normalized.clone(),
            source: "passthrough".to_string(),
            original_request: model_name.to_string(),
            normalized,
            is_verified: false,
        }
    }

    /// Get model ID for Kiro API (simple helper)
    #[allow(dead_code)]
    pub fn get_model_id_for_kiro(&self, model_name: &str) -> String {
        self.resolve(model_name).internal_id
    }
}

impl Clone for ModelResolver {
    fn clone(&self) -> Self {
        Self {
            cache: self.cache.clone(),
            hidden_models: Arc::clone(&self.hidden_models),
        }
    }
}

/// Result of model resolution
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ModelResolution {
    /// ID to send to Kiro API
    pub internal_id: String,

    /// Resolution source - "cache", "hidden", or "passthrough"
    pub source: String,

    /// What client originally sent
    pub original_request: String,

    /// Model name after normalization
    pub normalized: String,

    /// True if found in cache/hidden, False if passthrough
    pub is_verified: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_model_name() {
        // Standard format with minor version
        assert_eq!(normalize_model_name("claude-haiku-4-5"), "claude-haiku-4.5");
        assert_eq!(
            normalize_model_name("claude-sonnet-4-5"),
            "claude-sonnet-4.5"
        );
        assert_eq!(normalize_model_name("claude-opus-4-5"), "claude-opus-4.5");

        // Standard format with date suffix
        assert_eq!(
            normalize_model_name("claude-haiku-4-5-20251001"),
            "claude-haiku-4.5"
        );
        assert_eq!(
            normalize_model_name("claude-sonnet-4-5-20250514"),
            "claude-sonnet-4.5"
        );

        // Standard format with 'latest' suffix
        assert_eq!(
            normalize_model_name("claude-haiku-4-5-latest"),
            "claude-haiku-4.5"
        );

        // Standard format without minor version
        assert_eq!(normalize_model_name("claude-sonnet-4"), "claude-sonnet-4");
        assert_eq!(
            normalize_model_name("claude-sonnet-4-20250514"),
            "claude-sonnet-4"
        );

        // Legacy format
        assert_eq!(
            normalize_model_name("claude-3-7-sonnet"),
            "claude-3.7-sonnet"
        );
        assert_eq!(
            normalize_model_name("claude-3-7-sonnet-20250219"),
            "claude-3.7-sonnet"
        );

        // Already normalized with dot
        assert_eq!(normalize_model_name("claude-haiku-4.5"), "claude-haiku-4.5");
        assert_eq!(
            normalize_model_name("claude-haiku-4.5-20251001"),
            "claude-haiku-4.5"
        );

        // Pass-through (no transformation)
        assert_eq!(normalize_model_name("auto"), "auto");
        assert_eq!(normalize_model_name("gpt-4"), "gpt-4");
    }

    #[test]
    fn test_extract_model_family() {
        assert_eq!(
            extract_model_family("claude-haiku-4.5"),
            Some("haiku".to_string())
        );
        assert_eq!(
            extract_model_family("claude-sonnet-4-5"),
            Some("sonnet".to_string())
        );
        assert_eq!(
            extract_model_family("claude-3.7-sonnet"),
            Some("sonnet".to_string())
        );
        assert_eq!(extract_model_family("gpt-4"), None);
    }

    #[test]
    fn test_model_resolver() {
        let cache = ModelCache::new(3600);

        // Add a model to cache
        cache.update(vec![serde_json::json!({
            "modelId": "claude-sonnet-4.5",
            "modelName": "Claude Sonnet 4.5"
        })]);

        // Add hidden model
        let mut hidden_models = HashMap::new();
        hidden_models.insert(
            "claude-3.7-sonnet".to_string(),
            "CLAUDE_3_7_SONNET_20250219_V1_0".to_string(),
        );

        let resolver = ModelResolver::new(cache, hidden_models);

        // Test cache resolution
        let result = resolver.resolve("claude-sonnet-4-5-20251001");
        assert_eq!(result.internal_id, "claude-sonnet-4.5");
        assert_eq!(result.source, "cache");
        assert!(result.is_verified);

        // Test hidden model resolution
        let result = resolver.resolve("claude-3-7-sonnet");
        assert_eq!(result.internal_id, "CLAUDE_3_7_SONNET_20250219_V1_0");
        assert_eq!(result.source, "hidden");
        assert!(result.is_verified);

        // Test pass-through
        let result = resolver.resolve("gpt-4");
        assert_eq!(result.internal_id, "gpt-4");
        assert_eq!(result.source, "passthrough");
        assert!(!result.is_verified);
    }
}
