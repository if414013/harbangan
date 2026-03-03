use dashmap::DashMap;
use serde_json::Value;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_MAX_INPUT_TOKENS: i32 = 200_000;

/// Thread-safe cache for storing model metadata
pub struct ModelCache {
    /// Model data indexed by model ID
    cache: Arc<DashMap<String, Value>>,

    /// Last update timestamp
    last_update: Arc<dashmap::DashMap<(), u64>>,

    /// Cache TTL in seconds
    cache_ttl: u64,
}

impl ModelCache {
    /// Create a new model cache
    pub fn new(cache_ttl: u64) -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
            last_update: Arc::new(DashMap::new()),
            cache_ttl,
        }
    }

    /// Update the cache with new model data
    pub fn update(&self, models_data: Vec<Value>) {
        tracing::info!("Updating model cache. Found {} models.", models_data.len());

        // Clear existing cache
        self.cache.clear();

        // Add new models
        for model in models_data {
            if let Some(model_id) = model.get("modelId").and_then(|v| v.as_str()) {
                self.cache.insert(model_id.to_string(), model);
            }
        }

        // Update timestamp
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.last_update.insert((), now);
    }

    /// Get model information by ID
    #[allow(dead_code)]
    pub fn get(&self, model_id: &str) -> Option<Value> {
        self.cache.get(model_id).map(|entry| entry.value().clone())
    }

    /// Check if a model exists in the cache
    pub fn is_valid_model(&self, model_id: &str) -> bool {
        self.cache.contains_key(model_id)
    }

    /// Add a hidden model to the cache
    pub fn add_hidden_model(&self, display_name: &str, internal_id: &str) {
        if !self.cache.contains_key(display_name) {
            let model_data = serde_json::json!({
                "modelId": display_name,
                "modelName": display_name,
                "description": format!("Hidden model (internal: {})", internal_id),
                "tokenLimits": {
                    "maxInputTokens": DEFAULT_MAX_INPUT_TOKENS
                },
                "_internal_id": internal_id,
                "_is_hidden": true
            });

            self.cache.insert(display_name.to_string(), model_data);
            tracing::debug!("Added hidden model: {} → {}", display_name, internal_id);
        }
    }

    /// Get maximum input tokens for a model
    #[allow(dead_code)]
    pub fn get_max_input_tokens(&self, model_id: &str) -> i32 {
        self.cache
            .get(model_id)
            .and_then(|entry| {
                entry
                    .get("tokenLimits")
                    .and_then(|limits| limits.get("maxInputTokens"))
                    .and_then(|v| v.as_i64())
                    .map(|v| v as i32)
            })
            .unwrap_or(DEFAULT_MAX_INPUT_TOKENS)
    }

    /// Check if the cache is empty
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Check if the cache is stale
    #[allow(dead_code)]
    pub fn is_stale(&self) -> bool {
        if let Some(entry) = self.last_update.get(&()) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let age = now - *entry.value();
            age > self.cache_ttl
        } else {
            true // No update yet, consider stale
        }
    }

    /// Get all model IDs
    pub fn get_all_model_ids(&self) -> Vec<String> {
        self.cache.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Get all models as a list
    #[allow(dead_code)]
    pub fn get_all_models(&self) -> Vec<Value> {
        self.cache
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }
}

impl Clone for ModelCache {
    fn clone(&self) -> Self {
        Self {
            cache: Arc::clone(&self.cache),
            last_update: Arc::clone(&self.last_update),
            cache_ttl: self.cache_ttl,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_cache_basic() {
        let cache = ModelCache::new(3600);

        // Initially empty
        assert!(cache.is_empty());
        assert!(cache.is_stale());

        // Add models
        let models = vec![
            serde_json::json!({
                "modelId": "claude-sonnet-4",
                "modelName": "Claude Sonnet 4",
                "tokenLimits": {"maxInputTokens": 200000}
            }),
            serde_json::json!({
                "modelId": "claude-haiku-4",
                "modelName": "Claude Haiku 4",
                "tokenLimits": {"maxInputTokens": 200000}
            }),
        ];

        cache.update(models);

        // No longer empty
        assert!(!cache.is_empty());
        assert!(!cache.is_stale());

        // Can retrieve models
        assert!(cache.is_valid_model("claude-sonnet-4"));
        assert!(cache.is_valid_model("claude-haiku-4"));
        assert!(!cache.is_valid_model("gpt-4"));

        // Can get model data
        let model = cache.get("claude-sonnet-4").unwrap();
        assert_eq!(model["modelName"], "Claude Sonnet 4");

        // Can get max tokens
        assert_eq!(cache.get_max_input_tokens("claude-sonnet-4"), 200000);
        assert_eq!(
            cache.get_max_input_tokens("unknown"),
            DEFAULT_MAX_INPUT_TOKENS
        );
    }

    #[test]
    fn test_hidden_models() {
        let cache = ModelCache::new(3600);

        cache.add_hidden_model("claude-3.7-sonnet", "CLAUDE_3_7_SONNET_20250219_V1_0");

        assert!(cache.is_valid_model("claude-3.7-sonnet"));

        let model = cache.get("claude-3.7-sonnet").unwrap();
        assert_eq!(model["_is_hidden"], true);
        assert_eq!(model["_internal_id"], "CLAUDE_3_7_SONNET_20250219_V1_0");
    }
}
