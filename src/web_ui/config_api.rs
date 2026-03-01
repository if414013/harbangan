use std::collections::HashMap;

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
        | "first_token_timeout" => ChangeType::HotReload,
        "server_host" | "server_port" | "tls_enabled" | "tls_cert_path" | "tls_key_path"
        | "kiro_cli_db_file" | "proxy_api_key" => ChangeType::RequiresRestart,
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
        "proxy_api_key" => {
            let s = value
                .as_str()
                .ok_or_else(|| "proxy_api_key must be a string".to_string())?;
            if s.is_empty() {
                return Err("proxy_api_key cannot be empty".to_string());
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
        "fake_reasoning_enabled" | "truncation_recovery" | "tls_enabled" => {
            if value.is_boolean() || value.as_str().is_some_and(|s| s == "true" || s == "false") {
                Ok(())
            } else {
                Err(format!("{} must be a boolean", key))
            }
        }
        "fake_reasoning_max_tokens" => {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|s| s.parse::<u64>().ok()))
                .ok_or_else(|| {
                    "fake_reasoning_max_tokens must be a positive integer".to_string()
                })?;
            Ok(())
        }
        "tool_description_max_length" => {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|s| s.parse::<u64>().ok()))
                .ok_or_else(|| {
                    "tool_description_max_length must be a positive integer".to_string()
                })?;
            Ok(())
        }
        "first_token_timeout" => {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|s| s.parse::<u64>().ok()))
                .ok_or_else(|| "first_token_timeout must be a positive integer".to_string())?;
            Ok(())
        }
        "tls_cert_path" | "tls_key_path" | "kiro_cli_db_file" => {
            value
                .as_str()
                .ok_or_else(|| format!("{} must be a string", key))?;
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
    m.insert(
        "proxy_api_key",
        "API key required for client authentication",
    );
    m.insert("kiro_region", "AWS region for the Kiro API");
    m.insert("kiro_cli_db_file", "Path to the kiro-cli SQLite database");
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
    m.insert("tls_enabled", "Enable HTTPS/TLS for the server");
    m.insert("tls_cert_path", "Path to TLS certificate file (PEM)");
    m.insert("tls_key_path", "Path to TLS private key file (PEM)");
    m
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
            classify_config_change("tls_enabled"),
            ChangeType::RequiresRestart
        );
        assert_eq!(
            classify_config_change("tls_cert_path"),
            ChangeType::RequiresRestart
        );
        assert_eq!(
            classify_config_change("tls_key_path"),
            ChangeType::RequiresRestart
        );
        assert_eq!(
            classify_config_change("kiro_cli_db_file"),
            ChangeType::RequiresRestart
        );
        assert_eq!(
            classify_config_change("proxy_api_key"),
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
        for key in &[
            "fake_reasoning_enabled",
            "truncation_recovery",
            "tls_enabled",
        ] {
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
        assert!(validate_config_field("proxy_api_key", &json!("key")).is_ok());
        assert!(validate_config_field("proxy_api_key", &json!("")).is_err());
    }

    #[test]
    fn test_validate_unknown_field() {
        assert!(validate_config_field("nonexistent", &json!("val")).is_err());
    }

    #[test]
    fn test_field_descriptions_complete() {
        let descs = get_config_field_descriptions();
        let expected_keys = vec![
            "server_host",
            "server_port",
            "proxy_api_key",
            "kiro_region",
            "kiro_cli_db_file",
            "log_level",
            "debug_mode",
            "fake_reasoning_enabled",
            "fake_reasoning_max_tokens",
            "truncation_recovery",
            "tool_description_max_length",
            "first_token_timeout",
            "tls_enabled",
            "tls_cert_path",
            "tls_key_path",
        ];
        for key in expected_keys {
            assert!(descs.contains_key(key), "Missing description for '{}'", key);
        }
    }
}
