use std::collections::HashMap;
use std::sync::atomic::Ordering;

use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;

use serde_json::{json, Value};
use sysinfo::{Pid, ProcessesToUpdate, System};

use crate::config::parse_debug_mode;
use crate::error::ApiError;
use crate::routes::AppState;
use crate::web_ui::config_api::{
    classify_config_change, get_config_field_descriptions, validate_config_field, ChangeType,
};

/// GET /ui/api/system - System info (CPU, memory, uptime)
pub async fn get_system_info() -> Json<Value> {
    let pid = Pid::from_u32(std::process::id());
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), false);

    let (cpu_usage, memory) = system
        .process(pid)
        .map(|p| (p.cpu_usage(), p.memory()))
        .unwrap_or((0.0, 0));

    let uptime = System::uptime();

    Json(json!({
        "cpu_usage": cpu_usage,
        "memory_bytes": memory,
        "memory_mb": memory as f64 / 1024.0 / 1024.0,
        "uptime_seconds": uptime,
    }))
}

/// GET /ui/api/models - List available models
pub async fn get_models(State(state): State<AppState>) -> Json<Value> {
    let model_ids = state.model_cache.get_all_model_ids();
    Json(json!({ "models": model_ids }))
}

/// Mask a sensitive string: show first 4 and last 4 chars, or "****" if too short.
/// Uses `.chars()` for safe character-boundary slicing (avoids UTF-8 panics).
fn mask_sensitive(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() > 8 {
        let prefix: String = chars[..4].iter().collect();
        let suffix: String = chars[chars.len() - 4..].iter().collect();
        format!("{}...{}", prefix, suffix)
    } else if chars.is_empty() {
        String::new()
    } else {
        "****".to_string()
    }
}

/// GET /ui/api/config - Current configuration (with masked secrets and setup status)
pub async fn get_config(State(state): State<AppState>) -> Json<Value> {
    let setup_complete = state.setup_complete.load(Ordering::Relaxed);

    // Clone the config snapshot — auth toggles are cached here via load_into_config
    let config = state.config.read().unwrap().clone();

    Json(json!({
        "setup_complete": setup_complete,
        "config": {
            "kiro_region": config.kiro_region,
            "streaming_timeout": config.streaming_timeout,
            "token_refresh_threshold": config.token_refresh_threshold,
            "first_token_timeout": config.first_token_timeout,
            "http_max_connections": config.http_max_connections,
            "http_connect_timeout": config.http_connect_timeout,
            "http_request_timeout": config.http_request_timeout,
            "http_max_retries": config.http_max_retries,
            "log_level": config.log_level,
            "debug_mode": format!("{:?}", config.debug_mode).to_lowercase(),
            "fake_reasoning_enabled": config.fake_reasoning_enabled,
            "fake_reasoning_max_tokens": config.fake_reasoning_max_tokens,
            "truncation_recovery": config.truncation_recovery,
            "guardrails_enabled": config.guardrails_enabled,
            "tool_description_max_length": config.tool_description_max_length,
            "anthropic_oauth_client_id": config.anthropic_oauth_client_id,
            "openai_oauth_client_id": config.openai_oauth_client_id,
            "google_client_id": config.google_client_id,
            "google_client_secret": if config.google_client_secret.is_empty() {
                String::new()
            } else {
                // Use "••••" prefix to match sentinel detection in PUT handler
                let chars: Vec<char> = config.google_client_secret.chars().collect();
                if chars.len() > 4 {
                    let suffix: String = chars[chars.len() - 4..].iter().collect();
                    format!("••••{}", suffix)
                } else {
                    "••••".to_string()
                }
            },
            "google_callback_url": config.google_callback_url,
            "auth_google_enabled": config.auth_google_enabled,
            "auth_password_enabled": config.auth_password_enabled,
        }
    }))
}

/// PUT /ui/api/config - Update configuration with validation and hot-reload
pub async fn update_config(
    State(state): State<AppState>,
    Json(updates): Json<HashMap<String, Value>>,
) -> Result<Json<Value>, ApiError> {
    // Validate all fields first
    for (key, value) in &updates {
        validate_config_field(key, value).map_err(ApiError::ValidationError)?;
    }

    // Cross-field validation: prevent disabling both auth methods
    if let Some(ref config_db) = state.config_db {
        // Read current DB state for both toggles
        let current_google_enabled = config_db
            .get("auth_google_enabled")
            .await
            .unwrap_or(None)
            .map(|v| v == "true")
            .unwrap_or(false);
        let current_password_enabled = config_db
            .get("auth_password_enabled")
            .await
            .unwrap_or(None)
            .map(|v| v == "true")
            .unwrap_or(true);

        // Check if Google SSO is fully configured
        let google_configured = {
            let config = state.config.read().unwrap_or_else(|p| p.into_inner());
            !config.google_client_id.is_empty()
                && !config.google_client_secret.is_empty()
                && !config.google_callback_url.is_empty()
        };

        // Apply proposed changes on top of current state
        let mut proposed_google = current_google_enabled;
        let mut proposed_password = current_password_enabled;
        let mut proposed_google_configured = google_configured;

        for (key, value) in &updates {
            let val_str = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            match key.as_str() {
                "auth_google_enabled" => proposed_google = val_str == "true",
                "auth_password_enabled" => proposed_password = val_str == "true",
                // Clearing any SSO credential field breaks google_configured
                "google_client_id" | "google_client_secret" | "google_callback_url" => {
                    if val_str.is_empty() {
                        proposed_google_configured = false;
                    }
                }
                _ => {}
            }
        }

        // Reject if both would be disabled
        if !proposed_password && !proposed_google {
            return Err(ApiError::ValidationError(
                "Cannot disable both authentication methods".to_string(),
            ));
        }

        // Reject if disabling password when Google SSO isn't fully configured
        if !proposed_password && (!proposed_google || !proposed_google_configured) {
            return Err(ApiError::ValidationError(
                "Cannot disable password auth when Google SSO is not fully configured and enabled"
                    .to_string(),
            ));
        }

        // Reject clearing SSO fields when password is disabled
        if !proposed_password && !proposed_google_configured && google_configured {
            return Err(ApiError::ValidationError(
                "Cannot clear Google SSO credentials when password auth is disabled".to_string(),
            ));
        }
    }

    let mut updated = Vec::new();
    let mut hot_reloaded = Vec::new();
    let mut requires_restart = Vec::new();

    // Persist to DB
    if let Some(ref config_db) = state.config_db {
        for (key, value) in &updates {
            let value_str = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };

            // Skip masked sentinel values for secrets (no-op on re-submit)
            if key == "google_client_secret"
                && (value_str.starts_with("••••") || value_str.starts_with("xxxx"))
            {
                continue;
            }

            // Store google_client_secret encrypted
            if key == "google_client_secret" {
                if let Ok(ek) = crate::web_ui::crypto::load_encryption_key() {
                    config_db
                        .set_encrypted(key, &value_str, &ek, "web_ui")
                        .await
                        .map_err(ApiError::Internal)?;
                } else if value_str.is_empty() {
                    // Allow clearing even without encryption key
                    config_db
                        .set(key, &value_str, "web_ui")
                        .await
                        .map_err(ApiError::Internal)?;
                } else {
                    return Err(ApiError::ValidationError(
                        "CONFIG_ENCRYPTION_KEY must be set to store Google client secret"
                            .to_string(),
                    ));
                }
                continue;
            }

            config_db
                .set(key, &value_str, "web_ui")
                .await
                .map_err(ApiError::Internal)?;
        }
    }

    // Classify and apply changes
    for (key, value) in &updates {
        updated.push(key.clone());

        match classify_config_change(key) {
            ChangeType::HotReload => {
                if apply_config_field(&state, key, value) {
                    hot_reloaded.push(key.clone());
                } else {
                    tracing::warn!(key = %key, value = ?value, "Failed to parse config value for hot-reload; change persisted but not applied at runtime");
                }
            }
            ChangeType::RequiresRestart => {
                requires_restart.push(key.clone());
            }
        }
    }

    Ok(Json(json!({
        "updated": updated,
        "hot_reloaded": hot_reloaded,
        "requires_restart": requires_restart,
    })))
}

/// Apply a single config field update to the runtime Config via write lock.
/// Returns `true` if the field was successfully applied, `false` if parsing failed.
fn apply_config_field(state: &AppState, key: &str, value: &Value) -> bool {
    let value_str = match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };

    let mut config = state.config.write().unwrap_or_else(|p| p.into_inner());
    match key {
        "log_level" => {
            config.log_level = value_str;
            true
        }
        "debug_mode" => {
            config.debug_mode = parse_debug_mode(&value_str);
            true
        }
        "fake_reasoning_enabled" => match value_str.parse() {
            Ok(v) => {
                config.fake_reasoning_enabled = v;
                true
            }
            Err(_) => false,
        },
        "fake_reasoning_max_tokens" => match value_str.parse() {
            Ok(v) => {
                config.fake_reasoning_max_tokens = v;
                true
            }
            Err(_) => false,
        },
        "truncation_recovery" => match value_str.parse() {
            Ok(v) => {
                config.truncation_recovery = v;
                true
            }
            Err(_) => false,
        },
        "guardrails_enabled" => match value_str.parse() {
            Ok(v) => {
                config.guardrails_enabled = v;
                true
            }
            Err(_) => false,
        },
        "tool_description_max_length" => match value_str.parse() {
            Ok(v) => {
                config.tool_description_max_length = v;
                true
            }
            Err(_) => false,
        },
        "first_token_timeout" => match value_str.parse() {
            Ok(v) => {
                config.first_token_timeout = v;
                true
            }
            Err(_) => false,
        },
        "anthropic_oauth_client_id" => {
            config.anthropic_oauth_client_id = value_str;
            true
        }
        "openai_oauth_client_id" => {
            config.openai_oauth_client_id = value_str;
            true
        }
        "auth_google_enabled" => match value_str.parse() {
            Ok(v) => {
                config.auth_google_enabled = v;
                true
            }
            Err(_) => false,
        },
        "auth_password_enabled" => match value_str.parse() {
            Ok(v) => {
                config.auth_password_enabled = v;
                true
            }
            Err(_) => false,
        },
        "google_client_id" => {
            config.google_client_id = value_str;
            true
        }
        "google_client_secret" => {
            // Skip masked sentinel values
            if value_str.starts_with("••••") || value_str.starts_with("xxxx") {
                return true;
            }
            config.google_client_secret = value_str;
            true
        }
        "google_callback_url" => {
            config.google_callback_url = value_str;
            true
        }
        _ => false,
    }
}

/// GET /ui/api/config/schema - Field metadata for the config UI
pub async fn get_config_schema() -> Json<Value> {
    let descriptions = get_config_field_descriptions();

    let mut fields = serde_json::Map::new();
    for (key, description) in &descriptions {
        let change_type = classify_config_change(key);
        let requires_restart = change_type == ChangeType::RequiresRestart;

        let mut field = serde_json::Map::new();
        field.insert("description".to_string(), json!(description));
        field.insert("requires_restart".to_string(), json!(requires_restart));

        match *key {
            "log_level" => {
                field.insert("type".to_string(), json!("string"));
                field.insert(
                    "options".to_string(),
                    json!(["trace", "debug", "info", "warn", "error"]),
                );
            }
            "debug_mode" => {
                field.insert("type".to_string(), json!("string"));
                field.insert("options".to_string(), json!(["off", "errors", "all"]));
            }
            "fake_reasoning_max_tokens"
            | "tool_description_max_length"
            | "first_token_timeout"
            | "streaming_timeout"
            | "token_refresh_threshold"
            | "http_max_connections"
            | "http_connect_timeout"
            | "http_request_timeout"
            | "http_max_retries" => {
                field.insert("type".to_string(), json!("number"));
            }
            "fake_reasoning_enabled"
            | "truncation_recovery"
            | "guardrails_enabled"
            | "auth_google_enabled"
            | "auth_password_enabled" => {
                field.insert("type".to_string(), json!("boolean"));
            }
            "google_client_secret" => {
                field.insert("type".to_string(), json!("password"));
            }
            _ => {
                field.insert("type".to_string(), json!("string"));
            }
        }

        fields.insert(key.to_string(), Value::Object(field));
    }

    Json(json!({ "fields": fields }))
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<usize>,
}

/// Keys whose values must be masked in config history responses.
const SENSITIVE_CONFIG_KEYS: &[&str] = &[
    "kiro_refresh_token",
    "oauth_client_secret",
    "google_client_secret",
];

/// GET /ui/api/config/history - Config change history
pub async fn get_config_history(
    State(state): State<AppState>,
    Query(params): Query<HistoryQuery>,
) -> Result<Json<Value>, ApiError> {
    let limit = params.limit.unwrap_or(50);

    if let Some(ref config_db) = state.config_db {
        let history = config_db
            .get_history(limit)
            .await
            .map_err(ApiError::Internal)?;

        let entries: Vec<Value> = history
            .iter()
            .map(|c| {
                let is_sensitive = SENSITIVE_CONFIG_KEYS.contains(&c.key.as_str());
                let old_value = if is_sensitive {
                    c.old_value.as_deref().map(mask_sensitive)
                } else {
                    c.old_value.clone()
                };
                let new_value = if is_sensitive {
                    mask_sensitive(&c.new_value)
                } else {
                    c.new_value.clone()
                };
                json!({
                    "key": c.key,
                    "old_value": old_value,
                    "new_value": new_value,
                    "changed_at": c.changed_at,
                    "source": c.source,
                })
            })
            .collect();

        return Ok(Json(json!({ "history": entries })));
    }

    Ok(Json(json!({ "history": [] })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_system_info() {
        let result = get_system_info().await;
        let value = result.0;
        assert!(value["cpu_usage"].is_number());
        assert!(value["memory_bytes"].is_number());
        assert!(value["uptime_seconds"].is_number());
    }

    #[test]
    fn test_mask_sensitive() {
        assert_eq!(mask_sensitive("abcdefghij"), "abcd...ghij");
        assert_eq!(mask_sensitive("short"), "****");
        assert_eq!(mask_sensitive(""), "");
        assert_eq!(mask_sensitive("12345678"), "****");
        assert_eq!(mask_sensitive("123456789"), "1234...6789");
    }

    #[tokio::test]
    async fn test_get_config_schema_has_fields() {
        let result = get_config_schema().await;
        let value = result.0;
        let fields = value["fields"].as_object().unwrap();
        assert!(fields.contains_key("log_level"));
        assert!(fields.contains_key("streaming_timeout"));

        let log_level = fields["log_level"].as_object().unwrap();
        assert!(log_level.contains_key("options"));
        assert_eq!(log_level["requires_restart"], false);

        let streaming_timeout = fields["streaming_timeout"].as_object().unwrap();
        assert_eq!(streaming_timeout["requires_restart"], true);

        // server_host/server_port are env-only and must not appear in schema
        assert!(!fields.contains_key("server_host"));
        assert!(!fields.contains_key("server_port"));
    }

    // ── Google SSO masking tests ────────────────────────────────────

    #[test]
    fn test_mask_sensitive_google_client_secret_long() {
        // Secret > 8 chars shows prefix...suffix
        let masked = mask_sensitive("GOCSPX-abcdefgh12345678");
        assert!(masked.starts_with("GOCS"));
        assert!(masked.ends_with("5678"));
        assert!(masked.contains("..."));
        // Must NOT contain the full secret
        assert!(!masked.contains("GOCSPX-abcdefgh12345678"));
    }

    #[test]
    fn test_mask_sensitive_google_client_secret_short() {
        // Secret <= 8 chars shows "****"
        let masked = mask_sensitive("short");
        assert_eq!(masked, "****");
    }

    #[test]
    fn test_mask_sensitive_empty() {
        assert_eq!(mask_sensitive(""), "");
    }

    #[test]
    fn test_sensitive_config_keys_includes_google_secret() {
        assert!(SENSITIVE_CONFIG_KEYS.contains(&"google_client_secret"));
    }

    // ── Google SSO schema tests ─────────────────────────────────────

    #[tokio::test]
    async fn test_config_schema_has_google_sso_fields() {
        let result = get_config_schema().await;
        let value = result.0;
        let fields = value["fields"].as_object().unwrap();

        assert!(fields.contains_key("google_client_id"));
        assert!(fields.contains_key("google_client_secret"));
        assert!(fields.contains_key("google_callback_url"));

        // google_client_secret should have type "password"
        let secret = fields["google_client_secret"].as_object().unwrap();
        assert_eq!(secret["type"], "password");

        // google_client_id should be string type
        let client_id = fields["google_client_id"].as_object().unwrap();
        assert_eq!(client_id["type"], "string");

        // All SSO fields should be hot-reloadable (no restart)
        assert_eq!(client_id["requires_restart"], false);
        assert_eq!(secret["requires_restart"], false);
        let callback = fields["google_callback_url"].as_object().unwrap();
        assert_eq!(callback["requires_restart"], false);
    }

    #[tokio::test]
    async fn test_config_schema_has_auth_toggles() {
        let result = get_config_schema().await;
        let value = result.0;
        let fields = value["fields"].as_object().unwrap();

        assert!(fields.contains_key("auth_google_enabled"));
        assert!(fields.contains_key("auth_password_enabled"));

        let google_toggle = fields["auth_google_enabled"].as_object().unwrap();
        assert_eq!(google_toggle["type"], "boolean");
        assert_eq!(google_toggle["requires_restart"], false);

        let password_toggle = fields["auth_password_enabled"].as_object().unwrap();
        assert_eq!(password_toggle["type"], "boolean");
        assert_eq!(password_toggle["requires_restart"], false);
    }

    // ── apply_config_field tests ────────────────────────────────────

    fn create_test_state() -> AppState {
        use crate::{
            auth::AuthManager, cache::ModelCache, config::Config, http_client::KiroHttpClient,
            resolver::ModelResolver,
        };
        use std::collections::HashMap;
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        let cache = ModelCache::new(3600);
        let http_client = Arc::new(KiroHttpClient::new(20, 30, 300, 3).unwrap());
        let auth_manager = Arc::new(tokio::sync::RwLock::new(
            AuthManager::new_for_testing("test-token".to_string(), "us-east-1".to_string(), 300)
                .unwrap(),
        ));
        let resolver = ModelResolver::new(cache.clone(), HashMap::new());
        let config = Config::with_defaults();
        let config_arc = Arc::new(std::sync::RwLock::new(config));

        AppState {
            proxy_api_key_hash: None,
            model_cache: cache,
            auth_manager: Arc::clone(&auth_manager),
            http_client: Arc::clone(&http_client),
            resolver,
            config: Arc::clone(&config_arc),
            setup_complete: Arc::new(AtomicBool::new(true)),
            config_db: None,
            session_cache: Arc::new(dashmap::DashMap::new()),
            api_key_cache: Arc::new(dashmap::DashMap::new()),
            kiro_token_cache: Arc::new(dashmap::DashMap::new()),
            oauth_pending: Arc::new(dashmap::DashMap::new()),
            guardrails_engine: None,
            provider_registry: Arc::new(crate::providers::registry::ProviderRegistry::new()),
            providers: crate::providers::build_provider_map(http_client, auth_manager, config_arc),
            provider_oauth_pending: Arc::new(dashmap::DashMap::new()),
            token_exchanger: Arc::new(crate::web_ui::provider_oauth::HttpTokenExchanger::new()),
            login_rate_limiter: Arc::new(dashmap::DashMap::new()),
            rate_tracker: Arc::new(crate::providers::rate_limiter::RateLimitTracker::new()),
        }
    }

    #[test]
    fn test_apply_config_field_google_client_id() {
        let state = create_test_state();
        let result = apply_config_field(
            &state,
            "google_client_id",
            &json!("my-client-id.apps.google"),
        );
        assert!(result);
        let config = state.config.read().unwrap();
        assert_eq!(config.google_client_id, "my-client-id.apps.google");
    }

    #[test]
    fn test_apply_config_field_google_client_id_empty() {
        let state = create_test_state();
        // Pre-set a value
        state.config.write().unwrap().google_client_id = "existing-id".to_string();
        // Clear it
        let result = apply_config_field(&state, "google_client_id", &json!(""));
        assert!(result);
        let config = state.config.read().unwrap();
        assert_eq!(config.google_client_id, "");
    }

    #[test]
    fn test_apply_config_field_google_callback_url() {
        let state = create_test_state();
        let url = "http://localhost:9999/_ui/api/auth/google/callback";
        let result = apply_config_field(&state, "google_callback_url", &json!(url));
        assert!(result);
        let config = state.config.read().unwrap();
        assert_eq!(config.google_callback_url, url);
    }

    #[test]
    fn test_apply_config_field_google_client_secret() {
        let state = create_test_state();
        let result =
            apply_config_field(&state, "google_client_secret", &json!("GOCSPX-real-secret"));
        assert!(result);
        let config = state.config.read().unwrap();
        assert_eq!(config.google_client_secret, "GOCSPX-real-secret");
    }

    #[test]
    fn test_apply_config_field_google_client_secret_sentinel_dots_skipped() {
        let state = create_test_state();
        // Pre-set a real secret
        state.config.write().unwrap().google_client_secret = "GOCSPX-real-secret".to_string();
        // Apply a masked sentinel (dots prefix) — should be a no-op
        let result = apply_config_field(&state, "google_client_secret", &json!("••••cret"));
        assert!(result); // returns true (success, not failure)
        let config = state.config.read().unwrap();
        // Secret must remain unchanged
        assert_eq!(config.google_client_secret, "GOCSPX-real-secret");
    }

    #[test]
    fn test_apply_config_field_google_client_secret_sentinel_xxxx_skipped() {
        let state = create_test_state();
        state.config.write().unwrap().google_client_secret = "GOCSPX-real-secret".to_string();
        let result = apply_config_field(&state, "google_client_secret", &json!("xxxxcret"));
        assert!(result);
        let config = state.config.read().unwrap();
        assert_eq!(config.google_client_secret, "GOCSPX-real-secret");
    }

    #[test]
    fn test_apply_config_field_google_client_secret_empty_clears() {
        let state = create_test_state();
        state.config.write().unwrap().google_client_secret = "GOCSPX-real-secret".to_string();
        // Empty string is NOT a sentinel — it clears the secret
        let result = apply_config_field(&state, "google_client_secret", &json!(""));
        assert!(result);
        let config = state.config.read().unwrap();
        assert_eq!(config.google_client_secret, "");
    }

    #[test]
    fn test_apply_config_field_auth_google_enabled() {
        let state = create_test_state();
        assert!(!state.config.read().unwrap().auth_google_enabled); // default false
        let result = apply_config_field(&state, "auth_google_enabled", &json!("true"));
        assert!(result);
        assert!(state.config.read().unwrap().auth_google_enabled);
    }

    #[test]
    fn test_apply_config_field_auth_password_enabled() {
        let state = create_test_state();
        assert!(state.config.read().unwrap().auth_password_enabled); // default true
        let result = apply_config_field(&state, "auth_password_enabled", &json!("false"));
        assert!(result);
        assert!(!state.config.read().unwrap().auth_password_enabled);
    }

    #[test]
    fn test_apply_config_field_auth_toggle_invalid_value() {
        let state = create_test_state();
        let result = apply_config_field(&state, "auth_google_enabled", &json!("not_a_bool"));
        assert!(!result); // parsing fails
    }

    #[test]
    fn test_apply_config_field_unknown_key_returns_false() {
        let state = create_test_state();
        let result = apply_config_field(&state, "nonexistent_key", &json!("value"));
        assert!(!result);
    }

    #[tokio::test]
    async fn test_get_config_excludes_server_host_port() {
        let state = create_test_state();
        let result = get_config(State(state)).await;
        let value = result.0;
        let config = value["config"].as_object().unwrap();
        assert!(
            !config.contains_key("server_host"),
            "server_host must not appear in GET /config response"
        );
        assert!(
            !config.contains_key("server_port"),
            "server_port must not appear in GET /config response"
        );
    }
}
