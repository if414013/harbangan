use anyhow::Result;

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

    // Guardrails
    pub guardrails_enabled: bool,

    // Multi-provider routing
    /// Default provider when user has no matching key. Typically "kiro".
    #[allow(dead_code)]
    pub default_provider: String,

    // MCP Gateway
    pub mcp_enabled: bool,
    pub mcp_tool_execution_timeout: u64,
    pub mcp_health_check_interval: u64,
    pub mcp_tool_sync_interval: u64,
    pub mcp_max_consecutive_failures: u32,

    // Database
    pub database_url: Option<String>,

    // Proxy-only mode (no DB, no SSO, single API key)
    pub proxy_api_key: Option<String>,
    pub kiro_refresh_token: Option<String>,
    pub kiro_client_id: Option<String>,
    pub kiro_client_secret: Option<String>,
    pub kiro_sso_url: Option<String>,
    pub kiro_sso_region: Option<String>,

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
            guardrails_enabled: false,
            default_provider: "kiro".to_string(),
            mcp_enabled: false,
            mcp_tool_execution_timeout: 30,
            mcp_health_check_interval: 10,
            mcp_tool_sync_interval: 600,
            mcp_max_consecutive_failures: 5,
            database_url: None,
            proxy_api_key: None,
            kiro_refresh_token: None,
            kiro_client_id: None,
            kiro_client_secret: None,
            kiro_sso_url: None,
            kiro_sso_region: None,
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

        // Proxy-only mode
        config.proxy_api_key = std::env::var("PROXY_API_KEY").ok();
        config.kiro_refresh_token = std::env::var("KIRO_REFRESH_TOKEN").ok();
        config.kiro_client_id = std::env::var("KIRO_CLIENT_ID").ok();
        config.kiro_client_secret = std::env::var("KIRO_CLIENT_SECRET").ok();
        config.kiro_sso_url = std::env::var("KIRO_SSO_URL").ok();
        config.kiro_sso_region = std::env::var("KIRO_SSO_REGION").ok();
        if let Ok(v) = std::env::var("KIRO_REGION") {
            config.kiro_region = v;
        }
        if let Ok(v) = std::env::var("LOG_LEVEL") {
            config.log_level = v;
        }
        if let Ok(v) = std::env::var("DEBUG_MODE") {
            config.debug_mode = parse_debug_mode(&v);
        }

        // Google SSO
        config.google_client_id = std::env::var("GOOGLE_CLIENT_ID").unwrap_or_default();
        config.google_client_secret = std::env::var("GOOGLE_CLIENT_SECRET").unwrap_or_default();
        config.google_callback_url = std::env::var("GOOGLE_CALLBACK_URL").unwrap_or_default();

        Ok(config)
    }

    /// Validate configuration.
    pub fn validate(&self) -> Result<()> {
        // Proxy-only mode: skip Google SSO validation
        if self.proxy_api_key.is_some() {
            return Ok(());
        }

        // Google SSO is the only auth path — required for the web UI
        if self.google_client_id.is_empty() {
            anyhow::bail!(
                "GOOGLE_CLIENT_ID is required. \
                 Google SSO is the only auth path — the gateway is unusable without it. \
                 Set PROXY_API_KEY for proxy-only mode without SSO."
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

        Ok(())
    }

    /// Returns true if running in proxy-only mode (no DB, no SSO).
    pub fn is_proxy_only(&self) -> bool {
        self.proxy_api_key.is_some()
    }
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
}
