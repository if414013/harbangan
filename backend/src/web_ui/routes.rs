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

    // Clone the config snapshot and drop the read guard before any .await
    let config = state.config.read().unwrap().clone();

    Json(json!({
        "setup_complete": setup_complete,
        "config": {
            "server_host": config.server_host,
            "server_port": config.server_port,
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
            "qwen_oauth_client_id": config.qwen_oauth_client_id,
            "anthropic_oauth_client_id": config.anthropic_oauth_client_id,
            "openai_oauth_client_id": config.openai_oauth_client_id,
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
        "qwen_oauth_client_id" => {
            config.qwen_oauth_client_id = value_str;
            true
        }
        "anthropic_oauth_client_id" => {
            config.anthropic_oauth_client_id = value_str;
            true
        }
        "openai_oauth_client_id" => {
            config.openai_oauth_client_id = value_str;
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
            "server_port"
            | "fake_reasoning_max_tokens"
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
            "fake_reasoning_enabled" | "truncation_recovery" | "guardrails_enabled" => {
                field.insert("type".to_string(), json!("boolean"));
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
const SENSITIVE_CONFIG_KEYS: &[&str] = &["kiro_refresh_token", "oauth_client_secret"];

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
        assert!(fields.contains_key("server_port"));

        let log_level = fields["log_level"].as_object().unwrap();
        assert!(log_level.contains_key("options"));
        assert_eq!(log_level["requires_restart"], false);

        let server_port = fields["server_port"].as_object().unwrap();
        assert_eq!(server_port["requires_restart"], true);
    }
}
