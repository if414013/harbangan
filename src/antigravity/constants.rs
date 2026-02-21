//! Constants for Antigravity (Cloud Code) API integration.
//!
//! Based on the antigravity-claude-proxy reference implementation.

use std::env::consts::{ARCH, OS};

use regex::Regex;

// === Cloud Code API Endpoints ===

/// Daily (staging) Cloud Code endpoint
pub const ENDPOINT_DAILY: &str = "https://daily-cloudcode-pa.googleapis.com";

/// Production Cloud Code endpoint
pub const ENDPOINT_PROD: &str = "https://cloudcode-pa.googleapis.com";

/// Endpoint fallback order for generateContent (daily first, then prod)
pub const ENDPOINT_FALLBACKS: &[&str] = &[ENDPOINT_DAILY, ENDPOINT_PROD];

/// Endpoint order for loadCodeAssist (prod first - works better for fresh accounts)
pub const LOAD_CODE_ASSIST_ENDPOINTS: &[&str] = &[ENDPOINT_PROD, ENDPOINT_DAILY];

// === Client Identity Headers ===

pub const X_CLIENT_NAME: &str = "antigravity";
pub const X_CLIENT_VERSION: &str = "1.107.0";
pub const X_GOOG_API_CLIENT: &str = "gl-node/18.18.2 fire/0.8.6 grpc/1.10.x";

/// Default project ID if none can be discovered via loadCodeAssist
pub const DEFAULT_PROJECT_ID: &str = "rising-fact-p41fc";

// === OAuth Configuration ===

pub const OAUTH_CLIENT_ID: &str =
    "REDACTED_OAUTH_CLIENT_ID";
pub const OAUTH_CLIENT_SECRET: &str = "REDACTED_OAUTH_CLIENT_SECRET";
pub const OAUTH_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const OAUTH_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const OAUTH_USER_INFO_URL: &str = "https://www.googleapis.com/oauth2/v1/userinfo";
pub const OAUTH_CALLBACK_PORT: u16 = 51121;

pub const OAUTH_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
    "https://www.googleapis.com/auth/cclog",
    "https://www.googleapis.com/auth/experimentsandconfigs",
];

// === IDE / Platform / Plugin Enums ===

/// IDE type identifiers as expected by the Cloud Code API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IdeType {
    Unspecified = 0,
    Plugins = 7,
    Antigravity = 9,
    Jetski = 10,
}

/// Platform identifiers as specified in the Antigravity binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Platform {
    Unspecified = 0,
    DarwinAmd64 = 1,
    DarwinArm64 = 2,
    LinuxAmd64 = 3,
    LinuxArm64 = 4,
    WindowsAmd64 = 5,
}

/// Plugin type identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PluginType {
    Unspecified = 0,
    CloudCode = 1,
    Gemini = 2,
}

/// Client metadata sent in request bodies (loadCodeAssist, onboardUser, etc.)
#[derive(Debug, Clone)]
pub struct ClientMetadata {
    pub ide_type: u8,
    pub platform: u8,
    pub plugin_type: u8,
}

/// Returns the runtime platform enum value based on `std::env::consts`.
pub fn get_platform_enum() -> Platform {
    match (OS, ARCH) {
        ("macos", "aarch64") => Platform::DarwinArm64,
        ("macos", _) => Platform::DarwinAmd64,
        ("linux", "aarch64") => Platform::LinuxArm64,
        ("linux", _) => Platform::LinuxAmd64,
        ("windows", _) => Platform::WindowsAmd64,
        _ => Platform::Unspecified,
    }
}

/// Returns the default client metadata for API requests.
pub fn client_metadata() -> ClientMetadata {
    ClientMetadata {
        ide_type: IdeType::Antigravity as u8,
        platform: get_platform_enum() as u8,
        plugin_type: PluginType::Gemini as u8,
    }
}

// === User-Agent ===

/// Generates a platform-specific User-Agent string.
///
/// Format: `antigravity/<version> <os>/<arch>`
pub fn platform_user_agent() -> String {
    let os_name = match OS {
        "macos" => "darwin",
        other => other,
    };
    let arch_name = match ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        other => other,
    };
    format!("antigravity/{} {}/{}", X_CLIENT_VERSION, os_name, arch_name)
}

// === Model Family Detection ===

/// Model family classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFamily {
    Claude,
    Gemini,
    Unknown,
}

/// Determines the model family from a model name string.
///
/// Detection is dynamic (substring match), not a hardcoded list.
pub fn get_model_family(model_name: &str) -> ModelFamily {
    let lower = model_name.to_lowercase();
    if lower.contains("claude") {
        ModelFamily::Claude
    } else if lower.contains("gemini") {
        ModelFamily::Gemini
    } else {
        ModelFamily::Unknown
    }
}

/// Checks if a model supports thinking/reasoning output.
///
/// - Claude: model name contains "thinking"
/// - Gemini: model name contains "thinking", or version >= 3
pub fn is_thinking_model(model_name: &str) -> bool {
    let lower = model_name.to_lowercase();

    // Claude thinking models
    if lower.contains("claude") && lower.contains("thinking") {
        return true;
    }

    // Gemini thinking models
    if lower.contains("gemini") {
        if lower.contains("thinking") {
            return true;
        }
        // gemini-3 or higher (e.g., gemini-3, gemini-3.5, gemini-4)
        let re = Regex::new(r"gemini-(\d+)").expect("valid regex");
        if let Some(caps) = re.captures(&lower) {
            if let Ok(version) = caps[1].parse::<u32>() {
                if version >= 3 {
                    return true;
                }
            }
        }
    }

    false
}

// === System Instruction ===

/// Minimal system instruction injected into Cloud Code requests.
pub const SYSTEM_INSTRUCTION: &str = "You are Antigravity, a powerful agentic AI coding assistant \
designed by the Google Deepmind team working on Advanced Agentic Coding.\
You are pair programming with a USER to solve their coding task. \
The task may require creating a new codebase, modifying or debugging an existing codebase, \
or simply answering a question.\
**Absolute paths only**\
**Proactiveness**";

// === Token / Retry Constants ===

/// Token refresh interval (5 minutes)
pub const TOKEN_REFRESH_INTERVAL_MS: u64 = 5 * 60 * 1000;

/// Gemini maximum output tokens
pub const GEMINI_MAX_OUTPUT_TOKENS: u32 = 16384;

/// Sentinel value to skip thought signature validation
pub const GEMINI_SKIP_SIGNATURE: &str = "skip_thought_signature_validator";

// === Model Fallback Map ===

/// Returns the fallback model for a given primary model when quota is exhausted.
pub fn get_model_fallback(model: &str) -> Option<&'static str> {
    match model {
        "gemini-3-pro-high" => Some("claude-opus-4-6-thinking"),
        "gemini-3-pro-low" => Some("claude-sonnet-4-5"),
        "gemini-3-flash" => Some("claude-sonnet-4-5-thinking"),
        "claude-opus-4-6-thinking" => Some("gemini-3-pro-high"),
        "claude-sonnet-4-5-thinking" => Some("gemini-3-flash"),
        "claude-sonnet-4-5" => Some("gemini-3-flash"),
        _ => None,
    }
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_model_family_claude() {
        assert_eq!(get_model_family("claude-sonnet-4-5"), ModelFamily::Claude);
        assert_eq!(
            get_model_family("claude-opus-4-6-thinking"),
            ModelFamily::Claude
        );
        assert_eq!(
            get_model_family("Claude-Sonnet-4-5-Thinking"),
            ModelFamily::Claude
        );
    }

    #[test]
    fn test_get_model_family_gemini() {
        assert_eq!(get_model_family("gemini-3-flash"), ModelFamily::Gemini);
        assert_eq!(get_model_family("gemini-3-pro-high"), ModelFamily::Gemini);
        assert_eq!(get_model_family("Gemini-3-Pro-High"), ModelFamily::Gemini);
    }

    #[test]
    fn test_get_model_family_unknown() {
        assert_eq!(get_model_family("gpt-4o"), ModelFamily::Unknown);
        assert_eq!(get_model_family(""), ModelFamily::Unknown);
        assert_eq!(get_model_family("some-random-model"), ModelFamily::Unknown);
    }

    #[test]
    fn test_is_thinking_model_claude() {
        assert!(is_thinking_model("claude-opus-4-6-thinking"));
        assert!(is_thinking_model("claude-sonnet-4-5-thinking"));
        assert!(is_thinking_model("Claude-Opus-4-6-Thinking"));
        assert!(!is_thinking_model("claude-sonnet-4-5"));
        assert!(!is_thinking_model("claude-opus-4-6"));
    }

    #[test]
    fn test_is_thinking_model_gemini_explicit() {
        assert!(is_thinking_model("gemini-2-thinking"));
        assert!(is_thinking_model("Gemini-2-Thinking"));
    }

    #[test]
    fn test_is_thinking_model_gemini_version() {
        // gemini-3+ are implicitly thinking models
        assert!(is_thinking_model("gemini-3-flash"));
        assert!(is_thinking_model("gemini-3-pro-high"));
        assert!(is_thinking_model("gemini-4-ultra"));
        // gemini-2 without "thinking" is not
        assert!(!is_thinking_model("gemini-2-flash"));
        assert!(!is_thinking_model("gemini-1.5-pro"));
    }

    #[test]
    fn test_is_thinking_model_non_thinking() {
        assert!(!is_thinking_model("gpt-4o"));
        assert!(!is_thinking_model(""));
        assert!(!is_thinking_model("some-model"));
    }

    #[test]
    fn test_platform_user_agent_format() {
        let ua = platform_user_agent();
        assert!(ua.starts_with("antigravity/"));
        assert!(ua.contains('/'));
    }

    #[test]
    fn test_get_platform_enum_not_unspecified() {
        // On macOS/Linux CI this should resolve to a real platform
        let p = get_platform_enum();
        // We can't assert the exact value since it depends on the host,
        // but on common CI it should not be Unspecified
        assert!(p != Platform::Unspecified || (OS != "macos" && OS != "linux" && OS != "windows"));
    }

    #[test]
    fn test_client_metadata_values() {
        let meta = client_metadata();
        assert_eq!(meta.ide_type, IdeType::Antigravity as u8);
        assert_eq!(meta.plugin_type, PluginType::Gemini as u8);
    }

    #[test]
    fn test_get_model_fallback() {
        assert_eq!(
            get_model_fallback("gemini-3-flash"),
            Some("claude-sonnet-4-5-thinking")
        );
        assert_eq!(
            get_model_fallback("claude-sonnet-4-5"),
            Some("gemini-3-flash")
        );
        assert_eq!(get_model_fallback("unknown-model"), None);
    }

    #[test]
    fn test_endpoint_constants() {
        assert!(ENDPOINT_DAILY.starts_with("https://"));
        assert!(ENDPOINT_PROD.starts_with("https://"));
        assert_eq!(ENDPOINT_FALLBACKS.len(), 2);
        assert_eq!(ENDPOINT_FALLBACKS[0], ENDPOINT_DAILY);
        assert_eq!(ENDPOINT_FALLBACKS[1], ENDPOINT_PROD);
    }

    #[test]
    fn test_load_code_assist_endpoints_prod_first() {
        assert_eq!(LOAD_CODE_ASSIST_ENDPOINTS[0], ENDPOINT_PROD);
        assert_eq!(LOAD_CODE_ASSIST_ENDPOINTS[1], ENDPOINT_DAILY);
    }

    #[test]
    fn test_oauth_scopes_count() {
        assert_eq!(OAUTH_SCOPES.len(), 5);
        assert!(OAUTH_SCOPES
            .iter()
            .all(|s| s.starts_with("https://www.googleapis.com/auth/")));
    }
}
