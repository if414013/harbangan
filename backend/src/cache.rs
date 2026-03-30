use dashmap::DashMap;
use serde_json::Value;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::web_ui::config_db::{ConfigDb, RegistryModel};

const DEFAULT_MAX_INPUT_TOKENS: i32 = 200_000;

/// Thread-safe cache for storing model metadata.
///
/// Supports two data sources:
/// - Kiro API models (legacy `update()` path, keyed by internal modelId)
/// - Model registry DB (new `load_from_registry()` path, keyed by prefixed_id)
pub struct ModelCache {
    /// Kiro model data indexed by model ID (legacy)
    cache: Arc<DashMap<String, Value>>,

    /// Registry models indexed by prefixed_id
    registry_cache: Arc<DashMap<String, RegistryModel>>,

    /// Last update timestamp
    last_update: Arc<dashmap::DashMap<(), u64>>,

    /// Last registry load timestamp
    registry_last_update: Arc<dashmap::DashMap<(), u64>>,

    /// Cache TTL in seconds
    cache_ttl: u64,

    /// Optional DB for registry-backed lookups
    config_db: Option<Arc<ConfigDb>>,
}

impl ModelCache {
    /// Create a new model cache
    pub fn new(cache_ttl: u64) -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
            registry_cache: Arc::new(DashMap::new()),
            last_update: Arc::new(DashMap::new()),
            registry_last_update: Arc::new(DashMap::new()),
            cache_ttl,
            config_db: None,
        }
    }

    /// Create a model cache with a DB reference for registry-backed lookups.
    #[allow(dead_code)]
    pub fn with_db(cache_ttl: u64, db: Arc<ConfigDb>) -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
            registry_cache: Arc::new(DashMap::new()),
            last_update: Arc::new(DashMap::new()),
            registry_last_update: Arc::new(DashMap::new()),
            cache_ttl,
            config_db: Some(db),
        }
    }

    /// Set the DB reference after construction.
    #[allow(dead_code)]
    pub fn set_db(&mut self, db: Arc<ConfigDb>) {
        self.config_db = Some(db);
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

    // ── Registry-backed methods ──────────────────────────────

    /// Load all models (enabled and disabled) from the DB registry into the in-memory registry cache.
    #[allow(dead_code)]
    pub async fn load_from_registry(&self) -> Result<usize, anyhow::Error> {
        let db = self
            .config_db
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No config_db configured"))?;

        let models = db.get_all_registry_models().await?;
        let count = models.len();

        self.registry_cache.clear();
        for m in models {
            self.registry_cache.insert(m.prefixed_id.clone(), m);
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.registry_last_update.insert((), now);

        tracing::info!(count, "Loaded models from registry into cache");
        Ok(count)
    }

    /// Check if a model exists in the registry cache (by prefixed_id).
    #[allow(dead_code)]
    pub fn is_registry_model(&self, prefixed_id: &str) -> bool {
        self.registry_cache.contains_key(prefixed_id)
    }

    /// Get a registry model by prefixed_id.
    #[allow(dead_code)]
    pub fn get_registry_model(&self, prefixed_id: &str) -> Option<RegistryModel> {
        self.registry_cache
            .get(prefixed_id)
            .map(|entry| entry.value().clone())
    }

    /// Get all registry models (enabled and disabled).
    #[allow(dead_code)]
    pub fn get_all_registry_models(&self) -> Vec<RegistryModel> {
        self.registry_cache
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Get only enabled registry models from the cache.
    #[allow(dead_code)]
    pub fn get_enabled_registry_models(&self) -> Vec<RegistryModel> {
        self.registry_cache
            .iter()
            .filter(|entry| entry.value().enabled)
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Get enabled registry models, also excluding models whose provider is disabled.
    pub fn get_enabled_registry_models_filtered(
        &self,
        disabled_providers: &std::collections::HashSet<crate::providers::types::ProviderId>,
    ) -> Vec<RegistryModel> {
        self.registry_cache
            .iter()
            .filter(|entry| {
                let m = entry.value();
                m.enabled
                    && !m
                        .provider_id
                        .parse::<crate::providers::types::ProviderId>()
                        .map(|pid| disabled_providers.contains(&pid))
                        .unwrap_or(false)
            })
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Check if a model is explicitly disabled in the registry.
    /// Returns `true` only if the model exists in the cache AND has `enabled == false`.
    /// Returns `false` if the model is not in the cache (unknown models pass through).
    #[allow(dead_code)]
    pub fn is_model_disabled(&self, prefixed_id: &str) -> bool {
        self.registry_cache
            .get(prefixed_id)
            .is_some_and(|entry| !entry.value().enabled)
    }

    /// Insert a registry model directly into the cache (test helper).
    #[cfg(test)]
    pub fn insert_registry_model(&self, model: RegistryModel) {
        self.registry_cache.insert(model.prefixed_id.clone(), model);
    }

    /// Check if the registry cache is stale.
    #[allow(dead_code)]
    pub fn is_registry_stale(&self) -> bool {
        if let Some(entry) = self.registry_last_update.get(&()) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let age = now - *entry.value();
            age > self.cache_ttl
        } else {
            true
        }
    }
}

impl Clone for ModelCache {
    fn clone(&self) -> Self {
        Self {
            cache: Arc::clone(&self.cache),
            registry_cache: Arc::clone(&self.registry_cache),
            last_update: Arc::clone(&self.last_update),
            registry_last_update: Arc::clone(&self.registry_last_update),
            cache_ttl: self.cache_ttl,
            config_db: self.config_db.clone(),
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
