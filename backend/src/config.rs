use anyhow::Result;

#[derive(Clone, Debug, PartialEq)]
pub enum GatewayMode {
    Full,
    Proxy,
}

/// Proxy-only mode configuration (all fields from env vars, no DB).
#[derive(Clone, Default)]
pub struct ProxyConfig {
    pub api_key: String,
    pub kiro_refresh_token: Option<String>,
    pub kiro_client_id: Option<String>,
    pub kiro_client_secret: Option<String>,
    pub kiro_sso_region: Option<String>,
    // Multi-provider credentials (env vars, no DB)
    pub anthropic_api_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub openai_base_url: Option<String>,
    pub copilot_token: Option<String>,
    pub copilot_base_url: Option<String>,
    pub qwen_token: Option<String>,
    pub qwen_base_url: Option<String>,
    pub custom_provider_url: Option<String>,
    pub custom_provider_key: Option<String>,
    pub custom_provider_models: Option<String>,
}

#[derive(Clone)]
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

    // Gateway mode
    pub gateway_mode: GatewayMode,

    // Proxy-only mode (grouped)
    pub proxy: Option<ProxyConfig>,

    // Database
    pub database_url: Option<String>,

    // Provider OAuth client IDs (defaults for public device-flow / PKCE clients)
    #[allow(dead_code)]
    pub qwen_oauth_client_id: String,
    #[allow(dead_code)]
    pub anthropic_oauth_client_id: String,
    #[allow(dead_code)]
    pub openai_oauth_client_id: String,

    // Google SSO (DB-backed, loaded via load_into_config)
    pub google_client_id: String,
    pub google_client_secret: String,
    pub google_callback_url: String,

    // Auth toggles (DB-backed, loaded via load_into_config)
    pub auth_google_enabled: bool,
    pub auth_password_enabled: bool,

    // Initial admin seeding (env vars, used by validate to allow password-only auth)
    pub initial_admin_email: Option<String>,
    pub initial_admin_password: Option<String>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("server_host", &self.server_host)
            .field("server_port", &self.server_port)
            .field("kiro_region", &self.kiro_region)
            .field("streaming_timeout", &self.streaming_timeout)
            .field("token_refresh_threshold", &self.token_refresh_threshold)
            .field("first_token_timeout", &self.first_token_timeout)
            .field("http_max_connections", &self.http_max_connections)
            .field("http_connect_timeout", &self.http_connect_timeout)
            .field("http_request_timeout", &self.http_request_timeout)
            .field("http_max_retries", &self.http_max_retries)
            .field("debug_mode", &self.debug_mode)
            .field("log_level", &self.log_level)
            .field(
                "tool_description_max_length",
                &self.tool_description_max_length,
            )
            .field("fake_reasoning_enabled", &self.fake_reasoning_enabled)
            .field("fake_reasoning_max_tokens", &self.fake_reasoning_max_tokens)
            .field("fake_reasoning_handling", &self.fake_reasoning_handling)
            .field("truncation_recovery", &self.truncation_recovery)
            .field("guardrails_enabled", &self.guardrails_enabled)
            .field("default_provider", &self.default_provider)
            .field("gateway_mode", &self.gateway_mode)
            .field("proxy", &self.proxy.as_ref().map(|_| "[REDACTED]"))
            .field("database_url", &self.database_url)
            .field("qwen_oauth_client_id", &self.qwen_oauth_client_id)
            .field("anthropic_oauth_client_id", &self.anthropic_oauth_client_id)
            .field("openai_oauth_client_id", &self.openai_oauth_client_id)
            .field("google_client_id", &self.google_client_id)
            .field("google_client_secret", &"[REDACTED]")
            .field("google_callback_url", &self.google_callback_url)
            .field("initial_admin_email", &self.initial_admin_email)
            .field(
                "initial_admin_password",
                &self.initial_admin_password.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
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
    /// Returns true when running in proxy-only mode (no DB, no Web UI).
    pub fn is_proxy_only(&self) -> bool {
        self.gateway_mode == GatewayMode::Proxy
    }

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
            gateway_mode: GatewayMode::Full,
            proxy: None,
            database_url: None,
            qwen_oauth_client_id: String::new(),
            anthropic_oauth_client_id: String::new(),
            openai_oauth_client_id: String::new(),
            google_client_id: String::new(),
            google_client_secret: String::new(),
            google_callback_url: String::new(),
            auth_google_enabled: false,
            auth_password_enabled: true,
            initial_admin_email: None,
            initial_admin_password: None,
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

        // Gateway mode (explicit env var, or inferred from PROXY_API_KEY)
        config.gateway_mode = match std::env::var("GATEWAY_MODE").as_deref() {
            Ok("proxy") => GatewayMode::Proxy,
            _ => GatewayMode::Full,
        };

        // Proxy-only mode: group all proxy fields into ProxyConfig
        if let Ok(api_key) = std::env::var("PROXY_API_KEY") {
            config.proxy = Some(ProxyConfig {
                api_key,
                kiro_refresh_token: std::env::var("KIRO_REFRESH_TOKEN").ok(),
                kiro_client_id: std::env::var("KIRO_CLIENT_ID").ok(),
                kiro_client_secret: std::env::var("KIRO_CLIENT_SECRET").ok(),
                kiro_sso_region: std::env::var("KIRO_SSO_REGION").ok(),
                anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
                openai_api_key: std::env::var("OPENAI_API_KEY").ok(),
                openai_base_url: std::env::var("OPENAI_BASE_URL").ok(),
                copilot_token: std::env::var("COPILOT_TOKEN").ok(),
                copilot_base_url: std::env::var("COPILOT_BASE_URL").ok(),
                qwen_token: std::env::var("QWEN_TOKEN").ok(),
                qwen_base_url: std::env::var("QWEN_BASE_URL").ok(),
                custom_provider_url: std::env::var("CUSTOM_PROVIDER_URL").ok(),
                custom_provider_key: std::env::var("CUSTOM_PROVIDER_KEY").ok(),
                custom_provider_models: std::env::var("CUSTOM_PROVIDER_MODELS").ok(),
            });
            // Infer proxy mode when PROXY_API_KEY is set (backward compat)
            config.gateway_mode = GatewayMode::Proxy;
        }

        if let Ok(v) = std::env::var("KIRO_REGION") {
            config.kiro_region = v;
        }
        if let Ok(v) = std::env::var("LOG_LEVEL") {
            config.log_level = v;
        }
        if let Ok(v) = std::env::var("DEBUG_MODE") {
            config.debug_mode = parse_debug_mode(&v);
        }

        // Provider OAuth client IDs (env var override for backward compat)
        if let Ok(v) = std::env::var("QWEN_OAUTH_CLIENT_ID") {
            config.qwen_oauth_client_id = v;
        }
        if let Ok(v) = std::env::var("ANTHROPIC_OAUTH_CLIENT_ID") {
            config.anthropic_oauth_client_id = v;
        }
        if let Ok(v) = std::env::var("OPENAI_OAUTH_CLIENT_ID") {
            config.openai_oauth_client_id = v;
        }

        // Initial admin seeding
        config.initial_admin_email = std::env::var("INITIAL_ADMIN_EMAIL")
            .ok()
            .filter(|s| !s.is_empty());
        config.initial_admin_password = std::env::var("INITIAL_ADMIN_PASSWORD")
            .ok()
            .filter(|s| !s.is_empty());

        Ok(config)
    }

    /// Validate configuration.
    pub fn validate(&self) -> Result<()> {
        // Proxy-only mode: require proxy API key with minimum length
        if self.is_proxy_only() {
            match &self.proxy {
                Some(p) if p.api_key.len() >= 16 => return Ok(()),
                Some(_) => {
                    anyhow::bail!("PROXY_API_KEY must be at least 16 characters for security")
                }
                None => {
                    anyhow::bail!("PROXY_API_KEY is required in proxy mode (GATEWAY_MODE=proxy)")
                }
            }
        }

        // Full mode: auth methods are configured at runtime via the admin UI
        // (stored in the config DB), not validated at startup.
        Ok(())
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
        assert_eq!(config.gateway_mode, GatewayMode::Full);
        assert!(config.proxy.is_none());
        assert!(config.initial_admin_email.is_none());
        assert!(config.initial_admin_password.is_none());
    }

    #[test]
    fn test_validate_full_mode_no_auth_at_startup() {
        // Full mode with no auth configured at startup is valid —
        // auth config is now in DB, not validated at startup.
        let config = Config::with_defaults();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_is_proxy_only_when_set() {
        let config = Config {
            gateway_mode: GatewayMode::Proxy,
            proxy: Some(ProxyConfig {
                api_key: "test-key-long-enough".to_string(),
                ..Default::default()
            }),
            ..Config::with_defaults()
        };
        assert!(config.is_proxy_only());
    }

    #[test]
    fn test_is_proxy_only_when_unset() {
        let config = Config::with_defaults();
        assert!(!config.is_proxy_only());
    }

    #[test]
    fn test_validate_proxy_mode_still_works() {
        let config = Config {
            gateway_mode: GatewayMode::Proxy,
            proxy: Some(ProxyConfig {
                api_key: "test-key-long-enough".to_string(),
                ..Default::default()
            }),
            ..Config::with_defaults()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_gateway_mode_proxy_from_env() {
        let config = Config {
            gateway_mode: GatewayMode::Proxy,
            proxy: Some(ProxyConfig {
                api_key: "a-secure-api-key-here".to_string(),
                kiro_refresh_token: Some("refresh-tok".to_string()),
                ..Default::default()
            }),
            ..Config::with_defaults()
        };
        assert!(config.is_proxy_only());
        assert_eq!(config.gateway_mode, GatewayMode::Proxy);
    }

    #[test]
    fn test_validate_proxy_mode_requires_min_key_length() {
        let config = Config {
            gateway_mode: GatewayMode::Proxy,
            proxy: Some(ProxyConfig {
                api_key: "short".to_string(),
                ..Default::default()
            }),
            ..Config::with_defaults()
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("at least 16 characters"));
    }

    #[test]
    fn test_debug_redacts_secrets() {
        let mut config = Config::with_defaults();
        config.google_client_secret = "super-secret".to_string();
        config.proxy = Some(ProxyConfig {
            api_key: "my-secret-api-key".to_string(),
            ..Default::default()
        });
        let debug_output = format!("{:?}", config);
        assert!(!debug_output.contains("super-secret"));
        assert!(!debug_output.contains("my-secret-api-key"));
        assert!(debug_output.contains("[REDACTED]"));
    }

    // ── Multi-provider ProxyConfig tests ──────────────────────────────

    #[test]
    fn test_proxy_config_all_provider_fields_none_by_default() {
        let proxy = ProxyConfig {
            api_key: "test-key-long-enough".to_string(),
            ..Default::default()
        };
        assert!(proxy.anthropic_api_key.is_none());
        assert!(proxy.openai_api_key.is_none());
        assert!(proxy.openai_base_url.is_none());
        assert!(proxy.copilot_token.is_none());
        assert!(proxy.copilot_base_url.is_none());
        assert!(proxy.qwen_token.is_none());
        assert!(proxy.qwen_base_url.is_none());
        assert!(proxy.custom_provider_url.is_none());
        assert!(proxy.custom_provider_key.is_none());
        assert!(proxy.custom_provider_models.is_none());
    }

    #[test]
    fn test_proxy_config_with_all_provider_fields() {
        let proxy = ProxyConfig {
            api_key: "test-key-long-enough".to_string(),
            kiro_refresh_token: None,
            kiro_client_id: None,
            kiro_client_secret: None,
            kiro_sso_region: None,
            anthropic_api_key: Some("sk-ant-test-key".to_string()),
            openai_api_key: Some("sk-proj-test-key".to_string()),
            openai_base_url: Some("https://api.openai.com/v1".to_string()),
            copilot_token: Some("cop-tok-test".to_string()),
            copilot_base_url: Some("https://api.githubcopilot.com".to_string()),
            qwen_token: Some("qwen-tok-test".to_string()),
            qwen_base_url: Some("https://qwen.example.com".to_string()),
            custom_provider_url: Some("http://localhost:11434/v1".to_string()),
            custom_provider_key: Some("custom-key-test".to_string()),
            custom_provider_models: Some("llama3,codellama,deepseek-r1".to_string()),
        };
        assert_eq!(proxy.anthropic_api_key.as_deref(), Some("sk-ant-test-key"));
        assert_eq!(proxy.openai_api_key.as_deref(), Some("sk-proj-test-key"));
        assert_eq!(
            proxy.openai_base_url.as_deref(),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(proxy.copilot_token.as_deref(), Some("cop-tok-test"));
        assert_eq!(
            proxy.copilot_base_url.as_deref(),
            Some("https://api.githubcopilot.com")
        );
        assert_eq!(proxy.qwen_token.as_deref(), Some("qwen-tok-test"));
        assert_eq!(
            proxy.qwen_base_url.as_deref(),
            Some("https://qwen.example.com")
        );
        assert_eq!(
            proxy.custom_provider_url.as_deref(),
            Some("http://localhost:11434/v1")
        );
        assert_eq!(
            proxy.custom_provider_key.as_deref(),
            Some("custom-key-test")
        );
        assert_eq!(
            proxy.custom_provider_models.as_deref(),
            Some("llama3,codellama,deepseek-r1")
        );
    }

    #[test]
    fn test_proxy_config_clone() {
        let proxy = ProxyConfig {
            api_key: "test-key-long-enough".to_string(),
            anthropic_api_key: Some("sk-ant-clone".to_string()),
            custom_provider_url: Some("http://localhost:11434/v1".to_string()),
            custom_provider_models: Some("llama3".to_string()),
            ..Default::default()
        };
        let cloned = proxy.clone();
        assert_eq!(cloned.api_key, "test-key-long-enough");
        assert_eq!(cloned.anthropic_api_key.as_deref(), Some("sk-ant-clone"));
        assert_eq!(
            cloned.custom_provider_url.as_deref(),
            Some("http://localhost:11434/v1")
        );
        assert_eq!(cloned.custom_provider_models.as_deref(), Some("llama3"));
    }

    #[test]
    fn test_proxy_config_partial_providers() {
        // Only Anthropic + Custom configured, rest None
        let proxy = ProxyConfig {
            api_key: "test-key-long-enough".to_string(),
            anthropic_api_key: Some("sk-ant-partial".to_string()),
            custom_provider_url: Some("http://localhost:11434/v1".to_string()),
            custom_provider_models: Some("llama3".to_string()),
            ..Default::default()
        };
        assert!(proxy.anthropic_api_key.is_some());
        assert!(proxy.openai_api_key.is_none());
        assert!(proxy.copilot_token.is_none());
        assert!(proxy.qwen_token.is_none());
        assert!(proxy.custom_provider_url.is_some());
        assert!(proxy.custom_provider_key.is_none());
    }

    // ── Google SSO config struct tests ───────────────────────────────

    #[test]
    fn test_google_sso_fields_default_empty() {
        let config = Config::with_defaults();
        assert!(config.google_client_id.is_empty());
        assert!(config.google_client_secret.is_empty());
        assert!(config.google_callback_url.is_empty());
    }

    #[test]
    fn test_google_sso_fields_settable() {
        let mut config = Config::with_defaults();
        config.google_client_id = "test-client-id.apps.googleusercontent.com".to_string();
        config.google_client_secret = "GOCSPX-test-secret".to_string();
        config.google_callback_url =
            "http://localhost:9999/_ui/api/auth/google/callback".to_string();

        assert_eq!(
            config.google_client_id,
            "test-client-id.apps.googleusercontent.com"
        );
        assert_eq!(config.google_client_secret, "GOCSPX-test-secret");
        assert_eq!(
            config.google_callback_url,
            "http://localhost:9999/_ui/api/auth/google/callback"
        );
    }

    #[test]
    fn test_debug_redacts_google_client_secret() {
        let mut config = Config::with_defaults();
        config.google_client_secret = "GOCSPX-super-secret-value".to_string();
        let debug_output = format!("{:?}", config);
        assert!(!debug_output.contains("GOCSPX-super-secret-value"));
        assert!(debug_output.contains("[REDACTED]"));
    }

    #[test]
    fn test_validate_full_mode_with_google_sso_configured() {
        let mut config = Config::with_defaults();
        config.google_client_id = "test-id".to_string();
        config.google_client_secret = "test-secret".to_string();
        config.google_callback_url = "http://localhost:9999/callback".to_string();
        // Full mode with SSO configured is valid
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_full_mode_with_partial_google_sso() {
        let mut config = Config::with_defaults();
        config.google_client_id = "test-id".to_string();
        // Missing secret and callback — still valid at startup since
        // auth config is now in DB, not validated at startup
        assert!(config.validate().is_ok());
    }
}
