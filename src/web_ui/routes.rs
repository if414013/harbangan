use std::collections::HashMap;
use std::sync::atomic::Ordering;

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
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

/// GET /ui - Dashboard page
pub async fn dashboard_page() -> Html<&'static str> {
    Html(include_str!("templates/dashboard.html"))
}

/// GET /ui/config - Config page
pub async fn config_page() -> Html<&'static str> {
    Html(include_str!("templates/config.html"))
}

/// GET /ui/static/:filename - Serve static assets
pub async fn static_asset(Path(filename): Path<String>) -> Response {
    let (content, content_type) = match filename.as_str() {
        "style.css" => (include_str!("static/style.css"), "text/css; charset=utf-8"),
        "app.js" => (
            include_str!("static/app.js"),
            "application/javascript; charset=utf-8",
        ),
        _ => {
            return (StatusCode::NOT_FOUND, "Not found").into_response();
        }
    };

    ([(header::CONTENT_TYPE, content_type)], content).into_response()
}

/// GET /ui/api/metrics - Current metrics snapshot
pub async fn get_metrics(State(state): State<AppState>) -> Json<Value> {
    Json(state.metrics.to_json_snapshot())
}

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

#[derive(Deserialize)]
pub struct LogsQuery {
    pub search: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// GET /ui/api/logs - Paginated log entries with optional search
pub async fn get_logs(
    State(state): State<AppState>,
    Query(params): Query<LogsQuery>,
) -> Json<Value> {
    let limit = params.limit.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);

    let (entries, total): (Vec<Value>, usize) = if let Ok(buffer) = state.log_buffer.lock() {
        let filtered: Vec<Value> = buffer
            .iter()
            .filter(|entry| {
                params
                    .search
                    .as_ref()
                    .is_none_or(|s| entry.message.to_lowercase().contains(&s.to_lowercase()))
            })
            .skip(offset)
            .take(limit)
            .map(|entry| {
                json!({
                    "timestamp": entry.timestamp.to_rfc3339(),
                    "level": entry.level.to_string(),
                    "message": entry.message,
                })
            })
            .collect();
        let total = buffer.len();
        (filtered, total)
    } else {
        (Vec::new(), 0)
    };

    Json(json!({
        "logs": entries,
        "total": total,
        "limit": limit,
        "offset": offset,
    }))
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
    let config = state.config.read().unwrap();
    let setup_complete = state.setup_complete.load(Ordering::Relaxed);

    let masked_key = mask_sensitive(&config.proxy_api_key);

    let masked_refresh_token = state
        .config_db
        .as_ref()
        .and_then(|db| db.get_refresh_token().ok().flatten())
        .map(|t| mask_sensitive(&t))
        .unwrap_or_default();

    Json(json!({
        "setup_complete": setup_complete,
        "config": {
            "server_host": config.server_host,
            "server_port": config.server_port,
            "proxy_api_key": masked_key,
            "kiro_refresh_token": masked_refresh_token,
            "kiro_region": config.kiro_region,
            "streaming_timeout": config.streaming_timeout,
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
            "tool_description_max_length": config.tool_description_max_length,
            "tls_enabled": config.tls_enabled,
            "dashboard": config.dashboard,
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
        _ => false,
    }
}

/// Request body for initial setup.
#[derive(Deserialize)]
pub struct SetupRequest {
    pub proxy_api_key: String,
    pub kiro_refresh_token: String,
    #[serde(default = "default_region")]
    pub region: String,
}

fn default_region() -> String {
    "us-east-1".to_string()
}

/// POST /ui/api/setup - Initial setup (no auth required)
pub async fn setup(
    State(state): State<AppState>,
    Json(body): Json<SetupRequest>,
) -> Result<Json<Value>, ApiError> {
    if body.proxy_api_key.is_empty() {
        return Err(ApiError::ValidationError(
            "proxy_api_key cannot be empty".to_string(),
        ));
    }
    if body.kiro_refresh_token.is_empty() {
        return Err(ApiError::ValidationError(
            "kiro_refresh_token cannot be empty".to_string(),
        ));
    }
    if body.region.is_empty() {
        return Err(ApiError::ValidationError(
            "region cannot be empty".to_string(),
        ));
    }

    // Atomically set from false to true to prevent race conditions
    if state
        .setup_complete
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(ApiError::ValidationError(
            "Setup has already been completed".to_string(),
        ));
    }

    let config_db_ref = state
        .config_db
        .as_ref()
        .ok_or_else(|| {
            state.setup_complete.store(false, Ordering::SeqCst);
            ApiError::ConfigError("Config database not available".to_string())
        })?
        .clone();

    if let Err(e) = config_db_ref.save_initial_setup(
        &body.proxy_api_key,
        &body.kiro_refresh_token,
        &body.region,
    ) {
        state.setup_complete.store(false, Ordering::SeqCst);
        return Err(ApiError::Internal(e));
    }

    // Update runtime config
    {
        let mut config = state.config.write().unwrap_or_else(|p| p.into_inner());
        config.proxy_api_key = body.proxy_api_key;
        config.kiro_region = body.region;
    }

    // Reinitialize AuthManager with the newly saved credentials
    let threshold = {
        let cfg = state.config.read().unwrap_or_else(|p| p.into_inner());
        cfg.token_refresh_threshold
    };
    match crate::auth::AuthManager::new(config_db_ref, threshold) {
        Ok(new_auth) => {
            let mut auth_lock = state.auth_manager.write().await;
            *auth_lock = new_auth;
        }
        Err(e) => {
            tracing::warn!(error = ?e, "Failed to initialize auth after setup; proxy may not work until restart");
        }
    }

    Ok(Json(json!({ "success": true })))
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
            | "first_token_timeout" => {
                field.insert("type".to_string(), json!("number"));
            }
            "fake_reasoning_enabled" | "truncation_recovery" | "tls_enabled" => {
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
const SENSITIVE_CONFIG_KEYS: &[&str] = &["proxy_api_key", "kiro_refresh_token"];

/// GET /ui/api/config/history - Config change history
pub async fn get_config_history(
    State(state): State<AppState>,
    Query(params): Query<HistoryQuery>,
) -> Result<Json<Value>, ApiError> {
    let limit = params.limit.unwrap_or(50);

    if let Some(ref config_db) = state.config_db {
        let history = config_db.get_history(limit).map_err(ApiError::Internal)?;

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
    use crate::metrics::MetricsCollector;

    #[tokio::test]
    async fn test_get_system_info() {
        let result = get_system_info().await;
        let value = result.0;
        assert!(value["cpu_usage"].is_number());
        assert!(value["memory_bytes"].is_number());
        assert!(value["uptime_seconds"].is_number());
    }

    #[test]
    fn test_metrics_to_json_snapshot() {
        let collector = MetricsCollector::new();
        collector.record_request_end(100.0, "test-model", 50, 100);

        let snapshot = collector.to_json_snapshot();
        assert!(snapshot["total_requests"].is_number());
        assert!(snapshot["latency"]["p50"].is_number());
        assert!(snapshot["models"].is_array());

        let models = snapshot["models"].as_array().unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["name"], "test-model");
    }

    #[test]
    fn test_metrics_snapshot_empty() {
        let collector = MetricsCollector::new();
        let snapshot = collector.to_json_snapshot();
        assert_eq!(snapshot["total_requests"], 0);
        assert_eq!(snapshot["total_errors"], 0);
        assert_eq!(snapshot["active_connections"], 0);
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
        assert!(fields.contains_key("proxy_api_key"));

        let log_level = fields["log_level"].as_object().unwrap();
        assert!(log_level.contains_key("options"));
        assert_eq!(log_level["requires_restart"], false);

        let server_port = fields["server_port"].as_object().unwrap();
        assert_eq!(server_port["requires_restart"], true);
    }
}
