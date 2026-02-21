use anyhow::{Context, Result};
use clap::Parser;
use dialoguer::{Confirm, Input, Password, Select};
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

/// Kiro Gateway - Rust Implementation
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct CliArgs {
    /// Server host address (use 127.0.0.1 or ::1 for localhost; the string "localhost" requires TLS)
    #[arg(short = 'H', long, env = "SERVER_HOST", default_value = "127.0.0.1")]
    pub host: String,

    /// Server port
    #[arg(short, long, env = "SERVER_PORT", default_value = "8000")]
    pub port: u16,

    /// Proxy API key for client authentication
    #[arg(short = 'k', long, env = "PROXY_API_KEY")]
    pub api_key: Option<String>,

    /// Path to kiro-cli SQLite database
    #[arg(short = 'd', long, env = "KIRO_CLI_DB_FILE")]
    pub db_file: Option<String>,

    /// AWS region for Kiro API
    #[arg(short = 'r', long, env = "KIRO_REGION", default_value = "us-east-1")]
    pub region: String,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, env = "LOG_LEVEL", default_value = "info")]
    pub log_level: String,

    /// Debug mode (off, errors, all)
    #[arg(long, env = "DEBUG_MODE", default_value = "off")]
    pub debug_mode: String,

    /// Enable fake reasoning/extended thinking
    #[arg(long, env = "FAKE_REASONING", default_value = "true")]
    pub fake_reasoning: bool,

    /// Max tokens for fake reasoning
    #[arg(long, env = "FAKE_REASONING_MAX_TOKENS", default_value = "4000")]
    pub fake_reasoning_max_tokens: u32,

    /// First token timeout in seconds
    #[arg(long, env = "FIRST_TOKEN_TIMEOUT", default_value = "15")]
    pub first_token_timeout: u64,

    /// HTTP request timeout in seconds
    #[arg(long, env = "HTTP_REQUEST_TIMEOUT", default_value = "300")]
    pub http_timeout: u64,

    /// HTTP max retries
    #[arg(long, env = "HTTP_MAX_RETRIES", default_value = "3")]
    pub http_retries: u32,

    /// Enable monitoring dashboard TUI
    #[arg(long, default_value = "false")]
    pub dashboard: bool,

    /// Enable HTTPS/TLS (auto-generates a self-signed certificate if --tls-cert/--tls-key are not provided)
    #[arg(long, env = "TLS_ENABLED", default_value = "false")]
    pub tls: bool,

    /// Path to TLS certificate file (PEM format)
    #[arg(long, env = "TLS_CERT")]
    pub tls_cert: Option<String>,

    /// Path to TLS private key file (PEM format)
    #[arg(long, env = "TLS_KEY")]
    pub tls_key: Option<String>,

    /// Enable Antigravity (Cloud Code) backend
    #[arg(long, env = "ANTIGRAVITY_ENABLED", default_value = "false")]
    pub antigravity_enabled: bool,

    /// Antigravity Google OAuth refresh token (composite: refreshToken|projectId|managedProjectId)
    #[arg(long, env = "ANTIGRAVITY_REFRESH_TOKEN")]
    pub antigravity_refresh_token: Option<String>,

    /// Antigravity Cloud Code project ID (overrides value from refresh token)
    #[arg(long, env = "ANTIGRAVITY_PROJECT_ID")]
    pub antigravity_project_id: Option<String>,

    /// Antigravity Cloud Code API endpoint (overrides default endpoint fallback)
    #[arg(long, env = "ANTIGRAVITY_ENDPOINT")]
    pub antigravity_endpoint: Option<String>,
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

    // Dashboard
    pub dashboard: bool,

    // TLS
    pub tls_enabled: bool,
    pub tls_cert_path: Option<PathBuf>,
    pub tls_key_path: Option<PathBuf>,

    // Antigravity (Cloud Code)
    #[allow(dead_code)]
    pub antigravity: AntigravityConfig,
}

/// Configuration for the Antigravity (Cloud Code) backend.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct AntigravityConfig {
    /// Whether the antigravity backend is enabled (opt-in).
    pub enabled: bool,
    /// Google OAuth refresh token (composite format: refreshToken|projectId|managedProjectId).
    pub refresh_token: Option<String>,
    /// Cloud Code project ID (overrides value embedded in refresh token).
    pub project_id: Option<String>,
    /// Custom Cloud Code API endpoint (overrides default fallback chain).
    pub endpoint: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
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
    /// Load configuration from all sources with priority: CLI > ENV > defaults
    pub fn load() -> Result<Self> {
        // Load .env file if it exists
        dotenvy::dotenv().ok();

        // Parse CLI arguments
        let args = CliArgs::parse();

        // Build config with priority handling
        let config = Config {
            // Server settings (from CLI with defaults)
            server_host: args.host,
            server_port: args.port,

            // Authentication (CLI > ENV, required)
            proxy_api_key: args
                .api_key
                .or_else(|| std::env::var("PROXY_API_KEY").ok())
                .context("PROXY_API_KEY is required (use -k or set PROXY_API_KEY env var)")?,

            // Kiro credentials
            kiro_region: args.region,

            kiro_cli_db_file: args
                .db_file
                .map(|s| expand_tilde(&s))
                .or_else(|| {
                    std::env::var("KIRO_CLI_DB_FILE")
                        .ok()
                        .map(|s| expand_tilde(&s))
                })
                .context("KIRO_CLI_DB_FILE is required (use -d or set KIRO_CLI_DB_FILE env var)")?,

            // Timeouts
            streaming_timeout: std::env::var("STREAMING_READ_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(300),

            token_refresh_threshold: std::env::var("TOKEN_REFRESH_THRESHOLD")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(300),

            first_token_timeout: args.first_token_timeout,

            // HTTP client
            http_max_connections: std::env::var("HTTP_MAX_CONNECTIONS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(20),

            http_connect_timeout: std::env::var("HTTP_CONNECT_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30),

            http_request_timeout: args.http_timeout,

            http_max_retries: args.http_retries,

            // Debug
            debug_mode: parse_debug_mode(&args.debug_mode),

            log_level: args.log_level,

            // Converter settings
            tool_description_max_length: std::env::var("TOOL_DESCRIPTION_MAX_LENGTH")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10000),

            fake_reasoning_enabled: args.fake_reasoning,

            fake_reasoning_max_tokens: args.fake_reasoning_max_tokens,

            fake_reasoning_handling: parse_fake_reasoning_handling(
                &std::env::var("FAKE_REASONING_HANDLING").unwrap_or_default(),
            ),

            dashboard: args.dashboard,

            // TLS
            tls_enabled: args.tls,
            tls_cert_path: args.tls_cert.map(|s| expand_tilde(&s)),
            tls_key_path: args.tls_key.map(|s| expand_tilde(&s)),

            // Antigravity
            antigravity: AntigravityConfig {
                enabled: args.antigravity_enabled,
                refresh_token: args.antigravity_refresh_token,
                project_id: args.antigravity_project_id,
                endpoint: args.antigravity_endpoint,
            },
        };

        Ok(config)
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        // Validate that the SQLite database file exists
        if !self.kiro_cli_db_file.exists() {
            anyhow::bail!(
                "KIRO_CLI_DB_FILE does not exist: {}",
                self.kiro_cli_db_file.display()
            );
        }

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
            // Provide helpful hint if user specified "localhost" string
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
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    PathBuf::from(path)
}

/// Parse debug mode from string
fn parse_debug_mode(s: &str) -> DebugMode {
    match s.to_lowercase().as_str() {
        "errors" => DebugMode::Errors,
        "all" => DebugMode::All,
        _ => DebugMode::Off,
    }
}

/// Parse fake reasoning handling mode from string
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
        // Unknown values should default to AsReasoningContent
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

    // Helper function to create a test config with a temporary database file.
    // Uses a unique filename per call to avoid cross-test interference.
    fn create_test_config(server_host: &str, tls_enabled: bool) -> (Config, PathBuf) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!(
            "test_kiro_{}_{}.db",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::File::create(&temp_file).unwrap();

        let config = Config {
            server_host: server_host.to_string(),
            server_port: 8000,
            proxy_api_key: "test-key".to_string(),
            kiro_region: "us-east-1".to_string(),
            kiro_cli_db_file: temp_file.clone(),
            streaming_timeout: 300,
            token_refresh_threshold: 300,
            first_token_timeout: 15,
            http_max_connections: 20,
            http_connect_timeout: 30,
            http_request_timeout: 300,
            http_max_retries: 3,
            debug_mode: DebugMode::Off,
            log_level: "info".to_string(),
            tool_description_max_length: 1000,
            fake_reasoning_enabled: false,
            fake_reasoning_max_tokens: 0,
            fake_reasoning_handling: FakeReasoningHandling::AsReasoningContent,
            dashboard: false,
            tls_enabled,
            tls_cert_path: None,
            tls_key_path: None,
            antigravity: AntigravityConfig {
                enabled: false,
                refresh_token: None,
                project_id: None,
                endpoint: None,
            },
        };

        (config, temp_file)
    }

    #[test]
    fn test_validate_localhost_without_tls_passes() {
        let (config, _tmp) = create_test_config("127.0.0.1", false);
        assert!(config.validate().is_ok());
        let _ = std::fs::remove_file(_tmp);
    }

    #[test]
    fn test_validate_localhost_string_without_tls_fails() {
        // "localhost" is not treated as a loopback IP literal, so TLS is required
        let (config, _tmp) = create_test_config("localhost", false);
        let result = config.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("TLS is required"));
        let _ = std::fs::remove_file(_tmp);
    }

    #[test]
    fn test_validate_ipv6_localhost_without_tls_passes() {
        let (config, _tmp) = create_test_config("::1", false);
        assert!(config.validate().is_ok());
        let _ = std::fs::remove_file(_tmp);
    }

    #[test]
    fn test_validate_0_0_0_0_without_tls_fails() {
        let (config, _tmp) = create_test_config("0.0.0.0", false);
        let result = config.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("TLS is required"));
        let _ = std::fs::remove_file(_tmp);
    }

    #[test]
    fn test_validate_0_0_0_0_with_tls_passes() {
        let (config, _tmp) = create_test_config("0.0.0.0", true);
        assert!(config.validate().is_ok());
        let _ = std::fs::remove_file(_tmp);
    }

    #[test]
    fn test_validate_specific_ip_without_tls_fails() {
        let (config, _tmp) = create_test_config("192.168.1.100", false);
        let result = config.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("TLS is required"));
        let _ = std::fs::remove_file(_tmp);
    }

    #[test]
    fn test_validate_specific_ip_with_tls_passes() {
        let (config, _tmp) = create_test_config("192.168.1.100", true);
        assert!(config.validate().is_ok());
        let _ = std::fs::remove_file(_tmp);
    }
}

// === Interactive Setup ===

/// Check if interactive setup is needed (no .env file and missing required values)
pub fn needs_interactive_setup() -> bool {
    // Check if .env file exists
    let env_file_exists = std::path::Path::new(".env").exists();

    // Check if required env vars are set
    let has_proxy_key = std::env::var("PROXY_API_KEY").is_ok();
    let has_db_file = std::env::var("KIRO_CLI_DB_FILE").is_ok();

    // Need setup if no .env and missing required values
    !env_file_exists && (!has_proxy_key || !has_db_file)
}

/// Run interactive setup to collect required configuration
pub fn run_interactive_setup() -> Result<InteractiveConfig> {
    println!();
    println!("╔═══════════════════════════════════════════════════════════╗");
    println!("║           🔧 Kiro Gateway - First Time Setup              ║");
    println!("╚═══════════════════════════════════════════════════════════╝");
    println!();
    println!("No configuration found. Let's set up your gateway.");
    println!();

    // Prompt for PROXY_API_KEY
    let proxy_api_key: String = Password::new()
        .with_prompt("Enter a password to protect your gateway (PROXY_API_KEY)")
        .interact()
        .context("Failed to read PROXY_API_KEY")?;

    if proxy_api_key.is_empty() {
        anyhow::bail!("PROXY_API_KEY cannot be empty");
    }

    // Try to auto-detect kiro-cli database path
    let default_db_path = detect_kiro_cli_db_path();

    let kiro_cli_db_file: String = if let Some(ref default_path) = default_db_path {
        println!();
        println!("Found kiro-cli database at: {}", default_path);

        let use_default = Confirm::new()
            .with_prompt("Use this path?")
            .default(true)
            .interact()
            .context("Failed to read confirmation")?;

        if use_default {
            default_path.clone()
        } else {
            Input::new()
                .with_prompt("Enter path to kiro-cli SQLite database (KIRO_CLI_DB_FILE)")
                .interact_text()
                .context("Failed to read KIRO_CLI_DB_FILE")?
        }
    } else {
        println!();
        println!("Could not auto-detect kiro-cli database location.");
        println!("Common locations:");
        println!("  - macOS: ~/Library/Application Support/kiro-cli/data.sqlite3");
        println!("  - Linux: ~/.local/share/kiro-cli/data.sqlite3");
        println!();

        Input::new()
            .with_prompt("Enter path to kiro-cli SQLite database (KIRO_CLI_DB_FILE)")
            .interact_text()
            .context("Failed to read KIRO_CLI_DB_FILE")?
    };

    // Validate the database file exists
    let expanded_path = expand_tilde(&kiro_cli_db_file);
    if !expanded_path.exists() {
        anyhow::bail!(
            "Database file does not exist: {}\n\nMake sure you have logged in with kiro-cli:\n  kiro-cli login",
            expanded_path.display()
        );
    }

    // Prompt for region with default
    println!();
    let regions = vec!["us-east-1", "us-west-2", "eu-west-1", "ap-northeast-1"];
    let region_idx = Select::new()
        .with_prompt("Select AWS region for Kiro API")
        .items(&regions)
        .default(0)
        .interact()
        .context("Failed to read region selection")?;
    let kiro_region = regions[region_idx].to_string();

    // Prompt for server port with default
    println!();
    let server_port: String = Input::new()
        .with_prompt("Server port")
        .default("8000".to_string())
        .interact_text()
        .context("Failed to read server port")?;

    let config = InteractiveConfig {
        proxy_api_key,
        kiro_cli_db_file,
        kiro_region,
        server_port,
    };

    // Ask if user wants to save to .env file
    println!();
    let save_to_env = Confirm::new()
        .with_prompt("Save configuration to .env file?")
        .default(true)
        .interact()
        .context("Failed to read save confirmation")?;

    if save_to_env {
        save_env_file(&config)?;
        println!();
        println!("✅ Configuration saved to .env file");
    }

    println!();
    println!("✅ Setup complete! Starting gateway...");
    println!();

    Ok(config)
}

/// Configuration collected from interactive setup
#[derive(Debug, Clone)]
pub struct InteractiveConfig {
    pub proxy_api_key: String,
    pub kiro_cli_db_file: String,
    pub kiro_region: String,
    pub server_port: String,
}

/// Try to detect the kiro-cli database path
fn detect_kiro_cli_db_path() -> Option<String> {
    // Try macOS path first
    if let Some(home) = dirs::home_dir() {
        let macos_path = home.join("Library/Application Support/kiro-cli/data.sqlite3");
        if macos_path.exists() {
            return Some(macos_path.to_string_lossy().to_string());
        }

        // Try Linux path
        let linux_path = home.join(".local/share/kiro-cli/data.sqlite3");
        if linux_path.exists() {
            return Some(linux_path.to_string_lossy().to_string());
        }

        // Try old kiro path (data.db)
        let old_macos_path = home.join("Library/Application Support/kiro-cli/data.db");
        if old_macos_path.exists() {
            return Some(old_macos_path.to_string_lossy().to_string());
        }

        // Try ~/.kiro/data.db (legacy)
        let legacy_path = home.join(".kiro/data.db");
        if legacy_path.exists() {
            return Some(legacy_path.to_string_lossy().to_string());
        }
    }

    None
}

/// Save configuration to .env file
fn save_env_file(config: &InteractiveConfig) -> Result<()> {
    let env_content = format!(
        r#"# Kiro Gateway Configuration
# Generated by interactive setup

# Password to protect the proxy server (required)
PROXY_API_KEY={}

# Path to kiro-cli SQLite database (required)
KIRO_CLI_DB_FILE={}

# AWS region for Kiro API
KIRO_REGION={}

# Server settings
SERVER_HOST=127.0.0.1
SERVER_PORT={}

# Logging (trace, debug, info, warn, error)
LOG_LEVEL=info

# Debug mode (off, errors, all)
DEBUG_MODE=off
"#,
        config.proxy_api_key, config.kiro_cli_db_file, config.kiro_region, config.server_port,
    );

    let mut file = std::fs::File::create(".env").context("Failed to create .env file")?;
    file.write_all(env_content.as_bytes())
        .context("Failed to write .env file")?;

    Ok(())
}
