use anyhow::Result;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    // Server settings
    pub server_host: String,
    pub server_port: u16,

    // Kiro credentials
    pub kiro_region: String,

    // Timeouts
    #[allow(dead_code)]
    pub streaming_timeout: u64,
    pub token_refresh_threshold: u64,
    pub first_token_timeout: u64,

    // HTTP client
    pub http_max_connections: usize,
    pub http_connect_timeout: u64,
    pub http_request_timeout: u64,
    pub http_max_retries: u32,

    // Debug
    pub debug_mode: DebugMode,
    pub log_level: String,

    // Converter settings
    pub tool_description_max_length: usize,
    pub fake_reasoning_enabled: bool,
    pub fake_reasoning_max_tokens: u32,
    #[allow(dead_code)]
    pub fake_reasoning_handling: FakeReasoningHandling,

    // Truncation recovery
    pub truncation_recovery: bool,

    // TLS (always on — self-signed cert generated when no custom cert/key provided)
    pub tls_cert_path: Option<PathBuf>,
    pub tls_key_path: Option<PathBuf>,

    // Database
    pub database_url: Option<String>,

    // Google SSO (bootstrap from env vars)
    pub google_client_id: String,
    pub google_client_secret: String,
    pub google_callback_url: String,
}

#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum FakeReasoningHandling {
    AsReasoningContent, // Extract to reasoning_content field (OpenAI-compatible)
    Remove,             // Remove thinking block completely
    Pass,               // Pass through with original tags
    StripTags,          // Remove tags but keep content
}

#[derive(Clone, Debug, PartialEq)]
pub enum DebugMode {
    Off,
    Errors,
    All,
}

impl Config {
    /// Create a Config with sensible defaults for "setup mode".
    ///
    /// All fields have safe defaults so the gateway can start with no DB config.
    /// The DB overlay (`load_into_config`) fills in real values once setup is complete.
    pub fn with_defaults() -> Self {
        Config {
            server_host: "0.0.0.0".to_string(),
            server_port: 8000,
            kiro_region: "us-east-1".to_string(),
            streaming_timeout: 300,
            token_refresh_threshold: 300,
            first_token_timeout: 15,
            http_max_connections: 20,
            http_connect_timeout: 30,
            http_request_timeout: 300,
            http_max_retries: 3,
            debug_mode: DebugMode::Off,
            log_level: "info".to_string(),
            tool_description_max_length: 10000,
            fake_reasoning_enabled: true,
            fake_reasoning_max_tokens: 4000,
            fake_reasoning_handling: FakeReasoningHandling::AsReasoningContent,
            truncation_recovery: true,
            tls_cert_path: None,
            tls_key_path: None,
            database_url: None,
            google_client_id: String::new(),
            google_client_secret: String::new(),
            google_callback_url: String::new(),
        }
    }

    /// Load configuration from environment variables only (docker-compose deployment).
    pub fn load() -> Result<Self> {
        // Load .env file if it exists
        dotenvy::dotenv().ok();

        let mut config = Self::with_defaults();

        // Server
        if let Ok(v) = std::env::var("SERVER_HOST") {
            config.server_host = v;
        }
        if let Ok(v) = std::env::var("SERVER_PORT") {
            config.server_port = v
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid SERVER_PORT"))?;
        }

        // Database
        config.database_url = std::env::var("DATABASE_URL").ok();

        // TLS
        config.tls_cert_path = std::env::var("TLS_CERT").ok().map(|s| expand_tilde(&s));
        config.tls_key_path = std::env::var("TLS_KEY").ok().map(|s| expand_tilde(&s));

        // Google SSO
        config.google_client_id = std::env::var("GOOGLE_CLIENT_ID").unwrap_or_default();
        config.google_client_secret = std::env::var("GOOGLE_CLIENT_SECRET").unwrap_or_default();
        config.google_callback_url = std::env::var("GOOGLE_CALLBACK_URL").unwrap_or_default();

        Ok(config)
    }

    /// Validate configuration.
    pub fn validate(&self) -> Result<()> {
        // Google SSO is the only auth path — required for the web UI
        if self.google_client_id.is_empty() {
            anyhow::bail!(
                "GOOGLE_CLIENT_ID is required. \
                 Google SSO is the only auth path — the gateway is unusable without it."
            );
        }
        if self.google_callback_url.is_empty() {
            anyhow::bail!(
                "GOOGLE_CALLBACK_URL is required when GOOGLE_CLIENT_ID is set. \
                 No default is provided because SERVER_HOST=0.0.0.0 in Docker makes any auto-derived default broken."
            );
        }
        if self.google_client_secret.is_empty() {
            anyhow::bail!("GOOGLE_CLIENT_SECRET is required when GOOGLE_CLIENT_ID is set.");
        }

        // Validate TLS configuration
        if let Some(ref cert) = self.tls_cert_path {
            if self.tls_key_path.is_none() {
                anyhow::bail!(
                    "TLS_CERT was provided without TLS_KEY. Both are required when using custom certificates."
                );
            }
            if !cert.exists() {
                anyhow::bail!("TLS certificate file not found: {}", cert.display());
            }
        }
        if let Some(ref key) = self.tls_key_path {
            if self.tls_cert_path.is_none() {
                anyhow::bail!(
                    "TLS_KEY was provided without TLS_CERT. Both are required when using custom certificates."
                );
            }
            if !key.exists() {
                anyhow::bail!("TLS key file not found: {}", key.display());
            }
        }

        Ok(())
    }

    /// Whether custom TLS certificates were provided (vs auto-generated self-signed).
    pub fn has_custom_tls(&self) -> bool {
        self.tls_cert_path.is_some() && self.tls_key_path.is_some()
    }
}

/// Expand tilde (~) in file paths to user's home directory
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    PathBuf::from(path)
}

/// Parse debug mode from string
#[allow(dead_code)]
pub fn parse_debug_mode(s: &str) -> DebugMode {
    match s.to_lowercase().as_str() {
        "errors" => DebugMode::Errors,
        "all" => DebugMode::All,
        _ => DebugMode::Off,
    }
}

/// Parse fake reasoning handling mode from string
#[cfg(test)]
fn parse_fake_reasoning_handling(s: &str) -> FakeReasoningHandling {
    match s.to_lowercase().as_str() {
        "remove" => FakeReasoningHandling::Remove,
        "pass" => FakeReasoningHandling::Pass,
        "strip_tags" => FakeReasoningHandling::StripTags,
        _ => FakeReasoningHandling::AsReasoningContent, // default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde() {
        let path = expand_tilde("~/test/file.txt");
        assert!(path.to_string_lossy().contains("test/file.txt"));
        assert!(!path.to_string_lossy().starts_with("~"));

        let path = expand_tilde("/absolute/path");
        assert_eq!(path, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn test_expand_tilde_relative_path() {
        let path = expand_tilde("relative/path");
        assert_eq!(path, PathBuf::from("relative/path"));
    }

    #[test]
    fn test_expand_tilde_just_tilde() {
        let path = expand_tilde("~");
        assert_eq!(path, PathBuf::from("~"));
    }

    #[test]
    fn test_parse_debug_mode() {
        assert_eq!(parse_debug_mode("off"), DebugMode::Off);
        assert_eq!(parse_debug_mode("errors"), DebugMode::Errors);
        assert_eq!(parse_debug_mode("all"), DebugMode::All);
        assert_eq!(parse_debug_mode("invalid"), DebugMode::Off);
        assert_eq!(parse_debug_mode(""), DebugMode::Off);
    }

    #[test]
    fn test_parse_debug_mode_case_insensitive() {
        assert_eq!(parse_debug_mode("ERRORS"), DebugMode::Errors);
        assert_eq!(parse_debug_mode("Errors"), DebugMode::Errors);
        assert_eq!(parse_debug_mode("ALL"), DebugMode::All);
        assert_eq!(parse_debug_mode("All"), DebugMode::All);
        assert_eq!(parse_debug_mode("OFF"), DebugMode::Off);
    }

    #[test]
    fn test_parse_fake_reasoning_handling() {
        assert_eq!(
            parse_fake_reasoning_handling(""),
            FakeReasoningHandling::AsReasoningContent
        );
        assert_eq!(
            parse_fake_reasoning_handling("remove"),
            FakeReasoningHandling::Remove
        );
        assert_eq!(
            parse_fake_reasoning_handling("pass"),
            FakeReasoningHandling::Pass
        );
        assert_eq!(
            parse_fake_reasoning_handling("strip_tags"),
            FakeReasoningHandling::StripTags
        );
    }

    #[test]
    fn test_parse_fake_reasoning_handling_case_insensitive() {
        assert_eq!(
            parse_fake_reasoning_handling("REMOVE"),
            FakeReasoningHandling::Remove
        );
        assert_eq!(
            parse_fake_reasoning_handling("Remove"),
            FakeReasoningHandling::Remove
        );
        assert_eq!(
            parse_fake_reasoning_handling("PASS"),
            FakeReasoningHandling::Pass
        );
        assert_eq!(
            parse_fake_reasoning_handling("STRIP_TAGS"),
            FakeReasoningHandling::StripTags
        );
    }

    #[test]
    fn test_parse_fake_reasoning_handling_default() {
        assert_eq!(
            parse_fake_reasoning_handling("unknown"),
            FakeReasoningHandling::AsReasoningContent
        );
        assert_eq!(
            parse_fake_reasoning_handling("invalid"),
            FakeReasoningHandling::AsReasoningContent
        );
    }

    #[test]
    fn test_debug_mode_equality() {
        assert_eq!(DebugMode::Off, DebugMode::Off);
        assert_eq!(DebugMode::Errors, DebugMode::Errors);
        assert_eq!(DebugMode::All, DebugMode::All);
        assert_ne!(DebugMode::Off, DebugMode::Errors);
        assert_ne!(DebugMode::Errors, DebugMode::All);
    }

    #[test]
    fn test_fake_reasoning_handling_equality() {
        assert_eq!(
            FakeReasoningHandling::AsReasoningContent,
            FakeReasoningHandling::AsReasoningContent
        );
        assert_eq!(FakeReasoningHandling::Remove, FakeReasoningHandling::Remove);
        assert_eq!(FakeReasoningHandling::Pass, FakeReasoningHandling::Pass);
        assert_eq!(
            FakeReasoningHandling::StripTags,
            FakeReasoningHandling::StripTags
        );
        assert_ne!(FakeReasoningHandling::Remove, FakeReasoningHandling::Pass);
    }

    #[test]
    fn test_with_defaults() {
        let config = Config::with_defaults();
        assert_eq!(config.server_host, "0.0.0.0");
        assert_eq!(config.server_port, 8000);
        assert_eq!(config.kiro_region, "us-east-1");
        assert_eq!(config.debug_mode, DebugMode::Off);
        assert!(config.fake_reasoning_enabled);
        assert!(config.truncation_recovery);
        assert!(config.tls_cert_path.is_none());
        assert!(config.tls_key_path.is_none());
        assert_eq!(config.google_client_id, "");
        assert_eq!(config.google_client_secret, "");
        assert_eq!(config.google_callback_url, "");
    }

    #[test]
    fn test_validate_google_client_id_required() {
        let config = Config {
            google_client_id: String::new(),
            ..Config::with_defaults()
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("GOOGLE_CLIENT_ID"));
    }

    #[test]
    fn test_validate_google_callback_url_required() {
        let config = Config {
            google_client_id: "some-id".to_string(),
            google_client_secret: "some-secret".to_string(),
            google_callback_url: String::new(),
            ..Config::with_defaults()
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("GOOGLE_CALLBACK_URL"));
    }

    #[test]
    fn test_validate_google_secret_required() {
        let config = Config {
            google_client_id: "some-id".to_string(),
            google_client_secret: String::new(),
            google_callback_url: "http://localhost:8000/callback".to_string(),
            ..Config::with_defaults()
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("GOOGLE_CLIENT_SECRET"));
    }

    #[test]
    fn test_has_custom_tls() {
        let mut config = Config::with_defaults();
        assert!(!config.has_custom_tls());

        config.tls_cert_path = Some(PathBuf::from("/tmp/cert.pem"));
        assert!(!config.has_custom_tls()); // only cert, no key

        config.tls_key_path = Some(PathBuf::from("/tmp/key.pem"));
        assert!(config.has_custom_tls()); // both provided
    }
}
