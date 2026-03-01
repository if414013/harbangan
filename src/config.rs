use anyhow::Result;
use clap::Parser;
use std::io::IsTerminal;
use std::path::PathBuf;

/// Kiro Gateway - Rust Implementation
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct CliArgs {
    /// Server port
    #[arg(short, long, env = "SERVER_PORT", default_value = "8000")]
    pub port: u16,

    /// Path to config database for persistence
    #[arg(long, env = "CONFIG_DB")]
    pub config_db: Option<String>,

    /// Path to TLS certificate file (PEM format)
    #[arg(long, env = "TLS_CERT")]
    pub tls_cert: Option<String>,

    /// Path to TLS private key file (PEM format)
    #[arg(long, env = "TLS_KEY")]
    pub tls_key: Option<String>,

    /// Enable web UI dashboard (served at /_ui/)
    #[arg(long, env = "WEB_UI", default_value = "true")]
    pub web_ui: bool,

    /// Enable monitoring dashboard TUI
    #[arg(long, default_value = "false")]
    pub dashboard: bool,
}

#[derive(Clone, Debug)]
pub struct Config {
    // Server settings
    pub server_host: String,
    pub server_port: u16,

    // Authentication
    pub proxy_api_key: String,

    // Kiro credentials
    pub kiro_region: String,
    pub kiro_cli_db_file: PathBuf,

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

    // Dashboard
    pub dashboard: bool,

    // TLS
    pub tls_enabled: bool,
    pub tls_cert_path: Option<PathBuf>,
    pub tls_key_path: Option<PathBuf>,

    // Web UI
    pub web_ui_enabled: bool,
    pub config_db_path: Option<PathBuf>,
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
            server_host: "127.0.0.1".to_string(),
            server_port: 8000,
            proxy_api_key: String::new(),
            kiro_region: "us-east-1".to_string(),
            kiro_cli_db_file: PathBuf::new(),
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
            dashboard: false,
            tls_enabled: false,
            tls_cert_path: None,
            tls_key_path: None,
            web_ui_enabled: true,
            config_db_path: None,
        }
    }

    /// Load configuration from bootstrap CLI args only.
    ///
    /// Returns a Config with defaults for all DB-managed fields.
    /// Call `config_db.load_into_config()` afterwards to overlay persisted values.
    pub fn load() -> Result<Self> {
        // Load .env file if it exists
        dotenvy::dotenv().ok();

        // Parse CLI arguments (bootstrap-only subset)
        let args = CliArgs::parse();

        let mut config = Self::with_defaults();

        // Apply CLI overrides
        config.server_port = args.port;
        config.dashboard = args.dashboard;
        config.web_ui_enabled = args.web_ui;
        config.tls_cert_path = args.tls_cert.map(|s| expand_tilde(&s));
        config.tls_key_path = args.tls_key.map(|s| expand_tilde(&s));

        // Auto-enable TLS when cert+key are both provided
        if config.tls_cert_path.is_some() && config.tls_key_path.is_some() {
            config.tls_enabled = true;
        }

        config.config_db_path = args
            .config_db
            .map(|s| expand_tilde(&s))
            .or_else(|| dirs::home_dir().map(|h| h.join(".kiro-gateway").join("config.db")));

        Ok(config)
    }

    /// Validate configuration (bootstrap-level checks only).
    ///
    /// DB-managed fields (proxy_api_key, region, etc.) are NOT validated here
    /// because the gateway may be starting in setup mode with no config yet.
    pub fn validate(&self) -> Result<()> {
        if self.dashboard && !std::io::stdout().is_terminal() {
            anyhow::bail!(
                "--dashboard requires a terminal (TTY). Cannot run dashboard mode when stdout is not a terminal."
            );
        }

        // Validate TLS configuration
        if let Some(ref cert) = self.tls_cert_path {
            if self.tls_key_path.is_none() {
                anyhow::bail!(
                    "--tls-cert was provided without --tls-key. Both are required when using custom certificates."
                );
            }
            if !cert.exists() {
                anyhow::bail!("TLS certificate file not found: {}", cert.display());
            }
        }
        if let Some(ref key) = self.tls_key_path {
            if self.tls_cert_path.is_none() {
                anyhow::bail!(
                    "--tls-key was provided without --tls-cert. Both are required when using custom certificates."
                );
            }
            if !key.exists() {
                anyhow::bail!("TLS key file not found: {}", key.display());
            }
        }

        // Require TLS for non-loopback IP bindings
        let is_loopback = self.server_host == "127.0.0.1" || self.server_host == "::1";

        if !is_loopback && !self.is_tls_active() {
            if self.server_host == "localhost" {
                tracing::warn!(
                    "Host 'localhost' requires TLS because only IP literals (127.0.0.1, ::1) are recognized as loopback. \
                     Use --host 127.0.0.1 for local-only access without TLS, or enable TLS with --tls flag."
                );
            }

            anyhow::bail!(
                "TLS is required when binding to non-localhost addresses (current: {}). \
                 Either enable TLS with --tls flag, or bind to localhost with --host 127.0.0.1",
                self.server_host
            );
        }

        Ok(())
    }

    /// Returns true if TLS should be used (explicitly enabled or cert/key provided).
    pub fn is_tls_active(&self) -> bool {
        self.tls_enabled || (self.tls_cert_path.is_some() && self.tls_key_path.is_some())
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
        // Just "~" without slash should not expand
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

    fn create_test_config(server_host: &str, tls_enabled: bool) -> Config {
        Config {
            server_host: server_host.to_string(),
            tls_enabled,
            ..Config::with_defaults()
        }
    }

    #[test]
    fn test_with_defaults() {
        let config = Config::with_defaults();
        assert_eq!(config.server_host, "127.0.0.1");
        assert_eq!(config.server_port, 8000);
        assert_eq!(config.proxy_api_key, "");
        assert_eq!(config.kiro_region, "us-east-1");
        assert_eq!(config.debug_mode, DebugMode::Off);
        assert!(config.fake_reasoning_enabled);
        assert!(config.truncation_recovery);
    }

    #[test]
    fn test_validate_localhost_without_tls_passes() {
        let config = create_test_config("127.0.0.1", false);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_localhost_string_without_tls_fails() {
        let config = create_test_config("localhost", false);
        let result = config.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("TLS is required"));
    }

    #[test]
    fn test_validate_ipv6_localhost_without_tls_passes() {
        let config = create_test_config("::1", false);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_0_0_0_0_without_tls_fails() {
        let config = create_test_config("0.0.0.0", false);
        let result = config.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("TLS is required"));
    }

    #[test]
    fn test_validate_0_0_0_0_with_tls_passes() {
        let config = create_test_config("0.0.0.0", true);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_specific_ip_without_tls_fails() {
        let config = create_test_config("192.168.1.100", false);
        let result = config.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("TLS is required"));
    }

    #[test]
    fn test_validate_specific_ip_with_tls_passes() {
        let config = create_test_config("192.168.1.100", true);
        assert!(config.validate().is_ok());
    }
}
