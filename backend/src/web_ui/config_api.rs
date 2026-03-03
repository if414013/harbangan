use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::routing::{delete, get};
use axum::{Extension, Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ApiError;
use crate::routes::{AppState, SessionInfo};

/// Whether a config change can be applied at runtime or requires a restart.
#[derive(Debug, Clone, PartialEq)]
pub enum ChangeType {
    HotReload,
    RequiresRestart,
}

/// Classify whether changing a given config key can be hot-reloaded.
pub fn classify_config_change(key: &str) -> ChangeType {
    match key {
        "log_level"
        | "debug_mode"
        | "fake_reasoning_enabled"
        | "fake_reasoning_max_tokens"
        | "truncation_recovery"
        | "tool_description_max_length"
        | "first_token_timeout"
        | "guardrails_enabled" => ChangeType::HotReload,
        "server_host"
        | "server_port"
        | "streaming_timeout"
        | "token_refresh_threshold"
        | "http_max_connections"
        | "http_connect_timeout"
        | "http_request_timeout"
        | "http_max_retries" => ChangeType::RequiresRestart,
        // Default unknown keys to restart for safety
        _ => ChangeType::RequiresRestart,
    }
}

/// Validate a config field name and value type.
///
/// Returns `Ok(())` if valid, or `Err(message)` describing the problem.
pub fn validate_config_field(key: &str, value: &serde_json::Value) -> Result<(), String> {
    match key {
        "server_host" => {
            value
                .as_str()
                .ok_or_else(|| "server_host must be a string".to_string())?;
            Ok(())
        }
        "server_port" => {
            let n = value
                .as_u64()
                .or_else(|| value.as_str().and_then(|s| s.parse::<u64>().ok()))
                .ok_or_else(|| "server_port must be a number".to_string())?;
            if n == 0 || n > 65535 {
                return Err("server_port must be between 1 and 65535".to_string());
            }
            Ok(())
        }
        "kiro_region" => {
            value
                .as_str()
                .ok_or_else(|| "kiro_region must be a string".to_string())?;
            Ok(())
        }
        "log_level" => {
            let s = value
                .as_str()
                .ok_or_else(|| "log_level must be a string".to_string())?;
            match s.to_lowercase().as_str() {
                "trace" | "debug" | "info" | "warn" | "error" => Ok(()),
                _ => Err(format!(
                    "log_level must be one of: trace, debug, info, warn, error (got '{}')",
                    s
                )),
            }
        }
        "debug_mode" => {
            let s = value
                .as_str()
                .ok_or_else(|| "debug_mode must be a string".to_string())?;
            match s.to_lowercase().as_str() {
                "off" | "errors" | "all" => Ok(()),
                _ => Err(format!(
                    "debug_mode must be one of: off, errors, all (got '{}')",
                    s
                )),
            }
        }
        "fake_reasoning_enabled" | "truncation_recovery" | "guardrails_enabled" => {
            if value.is_boolean() || value.as_str().is_some_and(|s| s == "true" || s == "false") {
                Ok(())
            } else {
                Err(format!("{} must be a boolean", key))
            }
        }
        "fake_reasoning_max_tokens" => {
            let n = value
                .as_u64()
                .or_else(|| value.as_str().and_then(|s| s.parse::<u64>().ok()))
                .ok_or_else(|| {
                    "fake_reasoning_max_tokens must be a positive integer".to_string()
                })?;
            if n == 0 || n > 1_000_000 {
                return Err("fake_reasoning_max_tokens must be between 1 and 1000000".to_string());
            }
            Ok(())
        }
        "tool_description_max_length" => {
            let n = value
                .as_u64()
                .or_else(|| value.as_str().and_then(|s| s.parse::<u64>().ok()))
                .ok_or_else(|| {
                    "tool_description_max_length must be a positive integer".to_string()
                })?;
            if n == 0 || n > 1_000_000 {
                return Err("tool_description_max_length must be between 1 and 1000000".to_string());
            }
            Ok(())
        }
        "first_token_timeout" => {
            let n = value
                .as_u64()
                .or_else(|| value.as_str().and_then(|s| s.parse::<u64>().ok()))
                .ok_or_else(|| "first_token_timeout must be a positive integer".to_string())?;
            if n == 0 || n > 86400 {
                return Err("first_token_timeout must be between 1 and 86400".to_string());
            }
            Ok(())
        }
        "streaming_timeout"
        | "token_refresh_threshold"
        | "http_connect_timeout"
        | "http_request_timeout" => {
            let n = value
                .as_u64()
                .or_else(|| value.as_str().and_then(|s| s.parse::<u64>().ok()))
                .ok_or_else(|| format!("{} must be a positive integer", key))?;
            if n == 0 || n > 86400 {
                return Err(format!("{} must be between 1 and 86400", key));
            }
            Ok(())
        }
        "http_max_connections" => {
            let n = value
                .as_u64()
                .or_else(|| value.as_str().and_then(|s| s.parse::<u64>().ok()))
                .ok_or_else(|| "http_max_connections must be a positive integer".to_string())?;
            if n == 0 || n > 1000 {
                return Err("http_max_connections must be between 1 and 1000".to_string());
            }
            Ok(())
        }
        "http_max_retries" => {
            let n = value
                .as_u64()
                .or_else(|| value.as_str().and_then(|s| s.parse::<u64>().ok()))
                .ok_or_else(|| "http_max_retries must be a non-negative integer".to_string())?;
            if n > 10 {
                return Err("http_max_retries must be between 0 and 10".to_string());
            }
            Ok(())
        }
        _ => Err(format!("Unknown config field: '{}'", key)),
    }
}

/// Human-readable descriptions for each known config field.
pub fn get_config_field_descriptions() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert(
        "server_host",
        "Server bind address (e.g. 127.0.0.1, 0.0.0.0)",
    );
    m.insert("server_port", "Server listen port (1-65535)");
    m.insert("kiro_region", "AWS region for the Kiro API");
    m.insert(
        "log_level",
        "Logging verbosity: trace, debug, info, warn, error",
    );
    m.insert("debug_mode", "Debug output mode: off, errors, all");
    m.insert(
        "fake_reasoning_enabled",
        "Enable fake reasoning / extended thinking",
    );
    m.insert(
        "fake_reasoning_max_tokens",
        "Maximum tokens for fake reasoning output",
    );
    m.insert(
        "truncation_recovery",
        "Detect and recover from truncated API responses",
    );
    m.insert(
        "tool_description_max_length",
        "Maximum character length for tool descriptions",
    );
    m.insert(
        "first_token_timeout",
        "Seconds to wait for the first token before timing out",
    );
    m.insert(
        "streaming_timeout",
        "Seconds before a streaming response times out",
    );
    m.insert(
        "token_refresh_threshold",
        "Seconds before token expiry to trigger a refresh",
    );
    m.insert(
        "http_max_connections",
        "Maximum concurrent outbound HTTP connections (1-1000)",
    );
    m.insert(
        "http_connect_timeout",
        "Seconds to wait for an outbound TCP connection",
    );
    m.insert(
        "http_request_timeout",
        "Seconds before an outbound HTTP request times out",
    );
    m.insert(
        "http_max_retries",
        "Retry attempts for failed upstream requests (0-10)",
    );
    m.insert(
        "oauth_client_id",
        "AWS SSO OIDC client ID for OAuth authentication",
    );
    m.insert(
        "oauth_client_secret",
        "AWS SSO OIDC client secret (JWT, ~3.5KB)",
    );
    m.insert(
        "oauth_client_secret_expires_at",
        "When the OAuth client secret expires (re-registration needed)",
    );
    m.insert("oauth_start_url", "IAM Identity Center start URL");
    m.insert("oauth_sso_region", "AWS region for SSO OIDC endpoints");
    m.insert(
        "guardrails_enabled",
        "Enable AWS Bedrock guardrails for input/output validation",
    );
    m
}

// ── Domain allowlist types ───────────────────────────────────────────

/// A domain in the allowlist.
#[derive(Serialize)]
struct DomainEntry {
    domain: String,
    added_by: Option<Uuid>,
    created_at: DateTime<Utc>,
}

/// Response for listing domains.
#[derive(Serialize)]
struct DomainListResponse {
    domains: Vec<DomainEntry>,
    count: usize,
}

/// Request to add a domain.
#[derive(Deserialize)]
struct AddDomainRequest {
    domain: String,
}

/// Response for add/delete domain operations.
#[derive(Serialize)]
struct DomainOpResponse {
    ok: bool,
}

// Session extraction is handled by the session_middleware + admin_middleware,
// which inject SessionInfo into request extensions. Handlers use Extension<SessionInfo>.

// ── Domain allowlist handlers ────────────────────────────────────────

/// GET /_ui/api/domains — list allowed domains (admin only)
async fn list_domains(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionInfo>,
) -> Result<Json<DomainListResponse>, ApiError> {
    let config_db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Database not configured")))?;

    let rows = config_db
        .list_allowed_domains()
        .await
        .map_err(ApiError::Internal)?;

    let domains: Vec<DomainEntry> = rows
        .into_iter()
        .map(|(domain, added_by, created_at)| DomainEntry {
            domain,
            added_by,
            created_at,
        })
        .collect();

    let count = domains.len();
    Ok(Json(DomainListResponse { domains, count }))
}

/// POST /_ui/api/domains — add domain (admin only, stored lowercase, exact match only)
async fn add_domain(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
    Json(body): Json<AddDomainRequest>,
) -> Result<Json<DomainOpResponse>, ApiError> {
    let admin_id = session.user_id;
    let config_db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Database not configured")))?;

    // Validate domain format
    let domain = body.domain.trim().to_lowercase();
    if domain.is_empty() {
        return Err(ApiError::ValidationError(
            "Domain cannot be empty".to_string(),
        ));
    }
    if domain.contains(' ') || domain.contains('@') {
        return Err(ApiError::ValidationError(
            "Invalid domain format. Provide just the domain (e.g. 'example.com'), not an email address.".to_string(),
        ));
    }
    if !domain.contains('.') {
        return Err(ApiError::ValidationError(
            "Invalid domain format. Domain must contain at least one dot (e.g. 'example.com')."
                .to_string(),
        ));
    }

    config_db
        .add_allowed_domain(&domain, admin_id)
        .await
        .map_err(ApiError::Internal)?;

    tracing::info!(
        admin_id = %admin_id,
        domain = %domain,
        "domain_added"
    );

    Ok(Json(DomainOpResponse { ok: true }))
}

/// DELETE /_ui/api/domains/:domain — remove domain (admin only)
async fn remove_domain(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
    Path(domain): Path<String>,
) -> Result<Json<DomainOpResponse>, ApiError> {
    let admin_id = session.user_id;
    let config_db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Database not configured")))?;

    let rows_affected = config_db
        .remove_allowed_domain(&domain)
        .await
        .map_err(ApiError::Internal)?;

    if rows_affected == 0 {
        return Err(ApiError::ValidationError(format!(
            "Domain '{}' not found in allowlist",
            domain
        )));
    }

    tracing::info!(
        admin_id = %admin_id,
        domain = %domain,
        "domain_removed"
    );

    Ok(Json(DomainOpResponse { ok: true }))
}

// ── Router ───────────────────────────────────────────────────────────

/// Build the domain allowlist management router (admin only).
pub fn domain_routes() -> Router<AppState> {
    Router::new()
        .route("/domains", get(list_domains).post(add_domain))
        .route("/domains/{domain}", delete(remove_domain))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_classify_hot_reload() {
        assert_eq!(classify_config_change("log_level"), ChangeType::HotReload);
        assert_eq!(classify_config_change("debug_mode"), ChangeType::HotReload);
        assert_eq!(
            classify_config_change("fake_reasoning_enabled"),
            ChangeType::HotReload
        );
        assert_eq!(
            classify_config_change("fake_reasoning_max_tokens"),
            ChangeType::HotReload
        );
        assert_eq!(
            classify_config_change("truncation_recovery"),
            ChangeType::HotReload
        );
        assert_eq!(
            classify_config_change("tool_description_max_length"),
            ChangeType::HotReload
        );
        assert_eq!(
            classify_config_change("first_token_timeout"),
            ChangeType::HotReload
        );
    }

    #[test]
    fn test_classify_requires_restart() {
        assert_eq!(
            classify_config_change("server_host"),
            ChangeType::RequiresRestart
        );
        assert_eq!(
            classify_config_change("server_port"),
            ChangeType::RequiresRestart
        );
        assert_eq!(
            classify_config_change("streaming_timeout"),
            ChangeType::RequiresRestart
        );
        assert_eq!(
            classify_config_change("token_refresh_threshold"),
            ChangeType::RequiresRestart
        );
        assert_eq!(
            classify_config_change("http_max_connections"),
            ChangeType::RequiresRestart
        );
        assert_eq!(
            classify_config_change("http_connect_timeout"),
            ChangeType::RequiresRestart
        );
        assert_eq!(
            classify_config_change("http_request_timeout"),
            ChangeType::RequiresRestart
        );
        assert_eq!(
            classify_config_change("http_max_retries"),
            ChangeType::RequiresRestart
        );
    }

    #[test]
    fn test_classify_unknown_defaults_to_restart() {
        assert_eq!(
            classify_config_change("something_unknown"),
            ChangeType::RequiresRestart
        );
    }

    #[test]
    fn test_validate_server_port_valid() {
        assert!(validate_config_field("server_port", &json!(8080)).is_ok());
        assert!(validate_config_field("server_port", &json!("443")).is_ok());
    }

    #[test]
    fn test_validate_server_port_invalid() {
        assert!(validate_config_field("server_port", &json!(0)).is_err());
        assert!(validate_config_field("server_port", &json!(70000)).is_err());
        assert!(validate_config_field("server_port", &json!("abc")).is_err());
    }

    #[test]
    fn test_validate_log_level() {
        assert!(validate_config_field("log_level", &json!("info")).is_ok());
        assert!(validate_config_field("log_level", &json!("DEBUG")).is_ok());
        assert!(validate_config_field("log_level", &json!("invalid")).is_err());
        assert!(validate_config_field("log_level", &json!(123)).is_err());
    }

    #[test]
    fn test_validate_debug_mode() {
        assert!(validate_config_field("debug_mode", &json!("off")).is_ok());
        assert!(validate_config_field("debug_mode", &json!("errors")).is_ok());
        assert!(validate_config_field("debug_mode", &json!("all")).is_ok());
        assert!(validate_config_field("debug_mode", &json!("verbose")).is_err());
    }

    #[test]
    fn test_validate_boolean_fields() {
        for key in &["fake_reasoning_enabled", "truncation_recovery"] {
            assert!(validate_config_field(key, &json!(true)).is_ok());
            assert!(validate_config_field(key, &json!(false)).is_ok());
            assert!(validate_config_field(key, &json!("true")).is_ok());
            assert!(validate_config_field(key, &json!("false")).is_ok());
            assert!(validate_config_field(key, &json!("yes")).is_err());
            assert!(validate_config_field(key, &json!(1)).is_err());
        }
    }

    #[test]
    fn test_validate_numeric_fields() {
        for key in &[
            "fake_reasoning_max_tokens",
            "tool_description_max_length",
            "first_token_timeout",
        ] {
            assert!(validate_config_field(key, &json!(100)).is_ok());
            assert!(validate_config_field(key, &json!("200")).is_ok());
            assert!(validate_config_field(key, &json!("abc")).is_err());
        }
    }

    #[test]
    fn test_validate_string_fields() {
        assert!(validate_config_field("server_host", &json!("0.0.0.0")).is_ok());
        assert!(validate_config_field("server_host", &json!(123)).is_err());
    }

    #[test]
    fn test_validate_unknown_field() {
        assert!(validate_config_field("nonexistent", &json!("val")).is_err());
    }

    #[test]
    fn test_proxy_api_key_removed_from_validation() {
        // proxy_api_key is no longer a valid config field
        assert!(validate_config_field("proxy_api_key", &json!("key")).is_err());
    }

    #[test]
    fn test_validate_http_max_connections() {
        assert!(validate_config_field("http_max_connections", &json!(20)).is_ok());
        assert!(validate_config_field("http_max_connections", &json!(1)).is_ok());
        assert!(validate_config_field("http_max_connections", &json!(1000)).is_ok());
        assert!(validate_config_field("http_max_connections", &json!(0)).is_err());
        assert!(validate_config_field("http_max_connections", &json!(1001)).is_err());
    }

    #[test]
    fn test_validate_http_max_retries() {
        assert!(validate_config_field("http_max_retries", &json!(0)).is_ok());
        assert!(validate_config_field("http_max_retries", &json!(10)).is_ok());
        assert!(validate_config_field("http_max_retries", &json!(11)).is_err());
    }

    #[test]
    fn test_validate_timeout_fields() {
        for key in &[
            "streaming_timeout",
            "token_refresh_threshold",
            "http_connect_timeout",
            "http_request_timeout",
        ] {
            assert!(validate_config_field(key, &json!(100)).is_ok());
            assert!(validate_config_field(key, &json!("200")).is_ok());
            assert!(validate_config_field(key, &json!("abc")).is_err());
            assert!(validate_config_field(key, &json!(0)).is_err());
            assert!(validate_config_field(key, &json!(86401)).is_err());
        }
        // first_token_timeout has the same bounds
        assert!(validate_config_field("first_token_timeout", &json!(0)).is_err());
        assert!(validate_config_field("first_token_timeout", &json!(86401)).is_err());
    }

    #[test]
    fn test_validate_http_max_connections_string() {
        assert!(validate_config_field("http_max_connections", &json!("20")).is_ok());
        assert!(validate_config_field("http_max_connections", &json!("0")).is_err());
        assert!(validate_config_field("http_max_connections", &json!("1001")).is_err());
        assert!(validate_config_field("http_max_connections", &json!(-1)).is_err());
    }

    #[test]
    fn test_validate_http_max_retries_string() {
        assert!(validate_config_field("http_max_retries", &json!("5")).is_ok());
        assert!(validate_config_field("http_max_retries", &json!("11")).is_err());
        assert!(validate_config_field("http_max_retries", &json!(-1)).is_err());
    }

    #[test]
    fn test_validate_fake_reasoning_max_tokens_bounds() {
        assert!(validate_config_field("fake_reasoning_max_tokens", &json!(1)).is_ok());
        assert!(validate_config_field("fake_reasoning_max_tokens", &json!(1000000)).is_ok());
        assert!(validate_config_field("fake_reasoning_max_tokens", &json!(0)).is_err());
        assert!(validate_config_field("fake_reasoning_max_tokens", &json!(1000001)).is_err());
    }

    #[test]
    fn test_validate_tool_description_max_length_bounds() {
        assert!(validate_config_field("tool_description_max_length", &json!(1)).is_ok());
        assert!(validate_config_field("tool_description_max_length", &json!(1000000)).is_ok());
        assert!(validate_config_field("tool_description_max_length", &json!(0)).is_err());
        assert!(validate_config_field("tool_description_max_length", &json!(1000001)).is_err());
    }

    #[test]
    fn test_field_descriptions_complete() {
        let descs = get_config_field_descriptions();
        let expected_keys = vec![
            "server_host",
            "server_port",
            "kiro_region",
            "log_level",
            "debug_mode",
            "fake_reasoning_enabled",
            "fake_reasoning_max_tokens",
            "truncation_recovery",
            "tool_description_max_length",
            "first_token_timeout",
            "streaming_timeout",
            "token_refresh_threshold",
            "http_max_connections",
            "http_connect_timeout",
            "http_request_timeout",
            "http_max_retries",
        ];
        for key in expected_keys {
            assert!(descs.contains_key(key), "Missing description for '{}'", key);
        }
        // proxy_api_key should NOT be in descriptions
        assert!(
            !descs.contains_key("proxy_api_key"),
            "proxy_api_key should be removed from descriptions"
        );
    }

    #[test]
    fn test_domain_validation_in_request() {
        // Test AddDomainRequest deserialization
        let json = serde_json::json!({ "domain": "example.com" });
        let req: AddDomainRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.domain, "example.com");
    }

    #[test]
    fn test_domain_entry_serialization() {
        let entry = DomainEntry {
            domain: "example.com".to_string(),
            added_by: Some(Uuid::new_v4()),
            created_at: Utc::now(),
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["domain"], "example.com");
        assert!(json["added_by"].is_string());
    }

    #[test]
    fn test_domain_list_response_serialization() {
        let resp = DomainListResponse {
            domains: vec![],
            count: 0,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["count"], 0);
        assert!(json["domains"].is_array());
    }
}
