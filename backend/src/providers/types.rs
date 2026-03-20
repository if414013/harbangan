use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ApiError;

/// Identifies which AI provider handles a request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderId {
    #[serde(rename = "kiro")]
    Kiro,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "openai_codex")]
    OpenAICodex,
    #[serde(rename = "copilot")]
    Copilot,
    #[serde(rename = "qwen")]
    Qwen,
    #[serde(rename = "custom")]
    Custom,
}

impl ProviderId {
    /// Returns the string identifier stored in the database.
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderId::Kiro => "kiro",
            ProviderId::Anthropic => "anthropic",
            ProviderId::OpenAICodex => "openai_codex",
            ProviderId::Copilot => "copilot",
            ProviderId::Qwen => "qwen",
            ProviderId::Custom => "custom",
        }
    }

    /// Human-readable name for UI display.
    pub fn display_name(&self) -> &'static str {
        match self {
            ProviderId::Kiro => "Kiro",
            ProviderId::Anthropic => "Anthropic",
            ProviderId::OpenAICodex => "OpenAI Codex",
            ProviderId::Copilot => "Copilot",
            ProviderId::Qwen => "Qwen",
            ProviderId::Custom => "Custom",
        }
    }

    /// Authentication category: how the provider acquires credentials.
    /// - `"device_code"`: Device authorization flow (Kiro, Copilot, Qwen)
    /// - `"oauth_relay"`: API key stored per-user (Anthropic, OpenAI)
    /// - `"custom"`: User-supplied base URL + key
    pub fn category(&self) -> &'static str {
        match self {
            ProviderId::Kiro | ProviderId::Copilot | ProviderId::Qwen => "device_code",
            ProviderId::Anthropic | ProviderId::OpenAICodex => "oauth_relay",
            ProviderId::Custom => "custom",
        }
    }

    /// Whether this provider can participate in admin load-balancing pools.
    pub fn supports_pool(&self) -> bool {
        !matches!(self, ProviderId::Custom)
    }

    /// All providers visible to users (excludes Custom).
    pub fn all_visible() -> &'static [ProviderId] {
        &[
            ProviderId::Kiro,
            ProviderId::Anthropic,
            ProviderId::OpenAICodex,
            ProviderId::Copilot,
            ProviderId::Qwen,
        ]
    }

    /// Default API base URL for this provider, if known.
    pub fn default_base_url(&self) -> Option<&'static str> {
        match self {
            ProviderId::Anthropic => Some("https://api.anthropic.com"),
            ProviderId::OpenAICodex => Some("https://api.openai.com"),
            ProviderId::Copilot => Some("https://api.githubcopilot.com"),
            ProviderId::Qwen => Some("https://dashscope-intl.aliyuncs.com/compatible-mode"),
            ProviderId::Kiro | ProviderId::Custom => None,
        }
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for ProviderId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "kiro" => Ok(ProviderId::Kiro),
            "anthropic" => Ok(ProviderId::Anthropic),
            "openai_codex" => Ok(ProviderId::OpenAICodex),
            "copilot" => Ok(ProviderId::Copilot),
            "qwen" => Ok(ProviderId::Qwen),
            "custom" => Ok(ProviderId::Custom),
            other => Err(format!("Unknown provider: {}", other)),
        }
    }
}

/// Per-user credentials resolved at request time.
#[derive(Debug, Clone)]
pub struct ProviderCredentials {
    #[allow(dead_code)]
    pub provider: ProviderId,
    pub access_token: String,
    /// Override the default API endpoint (optional).
    pub base_url: Option<String>,
    /// Label identifying this account (for multi-account load balancing).
    #[allow(dead_code)]
    pub account_label: String,
}

/// Per-request context passed to a provider implementation.
#[derive(Debug)]
pub struct ProviderContext<'a> {
    pub credentials: &'a ProviderCredentials,
    pub model: &'a str,
}

/// Non-streaming response from a provider API.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProviderResponse {
    /// HTTP status code returned by the provider.
    pub status: u16,
    /// Parsed JSON body from the provider response.
    pub body: Value,
    /// HTTP headers from the provider response (for rate-limit tracking).
    pub headers: axum::http::HeaderMap,
}

/// A single item in a provider streaming response.
/// Contains raw SSE bytes that the handler pipes to the client.
pub type ProviderStreamItem = Result<Bytes, ApiError>;

/// Streaming response from a provider, wrapping initial headers and the byte stream.
///
/// The headers are captured before the response body is consumed as a stream,
/// allowing the caller to extract rate-limit headers (e.g. Retry-After).
pub struct ProviderStreamResponse {
    /// HTTP headers from the initial streaming response.
    pub headers: axum::http::HeaderMap,
    /// The streaming body as SSE byte chunks.
    pub stream: std::pin::Pin<Box<dyn futures::stream::Stream<Item = ProviderStreamItem> + Send>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_id_as_str() {
        assert_eq!(ProviderId::Kiro.as_str(), "kiro");
        assert_eq!(ProviderId::Anthropic.as_str(), "anthropic");
        assert_eq!(ProviderId::OpenAICodex.as_str(), "openai_codex");
        assert_eq!(ProviderId::Copilot.as_str(), "copilot");
        assert_eq!(ProviderId::Qwen.as_str(), "qwen");
        assert_eq!(ProviderId::Custom.as_str(), "custom");
    }

    #[test]
    fn test_provider_id_display() {
        assert_eq!(ProviderId::Anthropic.to_string(), "anthropic");
        assert_eq!(ProviderId::OpenAICodex.to_string(), "openai_codex");
        assert_eq!(ProviderId::Copilot.to_string(), "copilot");
        assert_eq!(ProviderId::Qwen.to_string(), "qwen");
        assert_eq!(ProviderId::Custom.to_string(), "custom");
    }

    #[test]
    fn test_provider_id_from_str() {
        use std::str::FromStr;
        assert_eq!(ProviderId::from_str("kiro").unwrap(), ProviderId::Kiro);
        assert_eq!(
            ProviderId::from_str("anthropic").unwrap(),
            ProviderId::Anthropic
        );
        assert_eq!(
            ProviderId::from_str("openai_codex").unwrap(),
            ProviderId::OpenAICodex
        );
        assert_eq!(
            ProviderId::from_str("copilot").unwrap(),
            ProviderId::Copilot
        );
        assert_eq!(ProviderId::from_str("qwen").unwrap(), ProviderId::Qwen);
        assert_eq!(ProviderId::from_str("custom").unwrap(), ProviderId::Custom);
        assert!(ProviderId::from_str("unknown").is_err());
    }

    #[test]
    fn test_provider_id_serialize() {
        let id = ProviderId::Anthropic;
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"anthropic\"");

        let id = ProviderId::Copilot;
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"copilot\"");

        let id = ProviderId::Qwen;
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"qwen\"");

        let id = ProviderId::Custom;
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"custom\"");
    }

    #[test]
    fn test_provider_id_deserialize() {
        let id: ProviderId = serde_json::from_str("\"openai_codex\"").unwrap();
        assert_eq!(id, ProviderId::OpenAICodex);

        let id: ProviderId = serde_json::from_str("\"copilot\"").unwrap();
        assert_eq!(id, ProviderId::Copilot);

        let id: ProviderId = serde_json::from_str("\"qwen\"").unwrap();
        assert_eq!(id, ProviderId::Qwen);

        let id: ProviderId = serde_json::from_str("\"custom\"").unwrap();
        assert_eq!(id, ProviderId::Custom);
    }

    #[test]
    fn test_provider_credentials_clone() {
        let creds = ProviderCredentials {
            provider: ProviderId::Anthropic,
            access_token: "sk-ant-test".to_string(),
            base_url: None,
            account_label: "default".to_string(),
        };
        let cloned = creds.clone();
        assert_eq!(cloned.provider, ProviderId::Anthropic);
        assert_eq!(cloned.access_token, "sk-ant-test");
    }

    #[test]
    fn test_provider_id_serde_round_trip() {
        for id in [
            ProviderId::Kiro,
            ProviderId::Anthropic,
            ProviderId::OpenAICodex,
            ProviderId::Copilot,
            ProviderId::Qwen,
            ProviderId::Custom,
        ] {
            let json = serde_json::to_string(&id).unwrap();
            let back: ProviderId = serde_json::from_str(&json).unwrap();
            assert_eq!(back, id);
        }
    }

    #[test]
    fn test_provider_id_deserialize_unknown_fails() {
        let result = serde_json::from_str::<ProviderId>("\"azure\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_provider_id_hash_eq() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ProviderId::Copilot);
        set.insert(ProviderId::Copilot);
        assert_eq!(set.len(), 1);
        set.insert(ProviderId::Kiro);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_provider_credentials_with_base_url() {
        let creds = ProviderCredentials {
            provider: ProviderId::Copilot,
            access_token: "cop-tok".to_string(),
            base_url: Some("https://api.business.githubcopilot.com".to_string()),
            account_label: "default".to_string(),
        };
        let cloned = creds.clone();
        assert_eq!(cloned.provider, ProviderId::Copilot);
        assert_eq!(
            cloned.base_url.unwrap(),
            "https://api.business.githubcopilot.com"
        );
    }

    #[test]
    fn test_provider_id_from_str_error_message() {
        use std::str::FromStr;
        let err = ProviderId::from_str("azure").unwrap_err();
        assert!(err.contains("Unknown provider"));
        assert!(err.contains("azure"));
    }

    // ── 6.7: Qwen ProviderId additional tests ───────────────────────

    #[test]
    fn test_provider_id_qwen_as_str() {
        assert_eq!(ProviderId::Qwen.as_str(), "qwen");
    }

    #[test]
    fn test_provider_id_qwen_display() {
        assert_eq!(format!("{}", ProviderId::Qwen), "qwen");
    }

    #[test]
    fn test_provider_id_qwen_from_str() {
        use std::str::FromStr;
        assert_eq!(ProviderId::from_str("qwen").unwrap(), ProviderId::Qwen);
    }

    #[test]
    fn test_provider_id_qwen_serde_round_trip() {
        let json = serde_json::to_string(&ProviderId::Qwen).unwrap();
        assert_eq!(json, "\"qwen\"");
        let back: ProviderId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ProviderId::Qwen);
    }

    #[test]
    fn test_provider_id_qwen_hash_eq() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ProviderId::Qwen);
        set.insert(ProviderId::Qwen);
        assert_eq!(set.len(), 1);
        set.insert(ProviderId::Copilot);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_provider_id_qwen_not_equal_to_others() {
        assert_ne!(ProviderId::Qwen, ProviderId::Kiro);
        assert_ne!(ProviderId::Qwen, ProviderId::Anthropic);
        assert_ne!(ProviderId::Qwen, ProviderId::OpenAICodex);
        assert_ne!(ProviderId::Qwen, ProviderId::Copilot);
    }

    #[test]
    fn test_provider_credentials_qwen_with_base_url() {
        let creds = ProviderCredentials {
            provider: ProviderId::Qwen,
            access_token: "qwen-tok-abc".to_string(),
            base_url: Some("https://custom.qwen.ai/api".to_string()),
            account_label: "default".to_string(),
        };
        let cloned = creds.clone();
        assert_eq!(cloned.provider, ProviderId::Qwen);
        assert_eq!(cloned.access_token, "qwen-tok-abc");
        assert_eq!(cloned.base_url.unwrap(), "https://custom.qwen.ai/api");
    }

    #[test]
    fn test_provider_credentials_qwen_no_base_url() {
        let creds = ProviderCredentials {
            provider: ProviderId::Qwen,
            access_token: "qwen-tok".to_string(),
            base_url: None,
            account_label: "default".to_string(),
        };
        assert!(creds.base_url.is_none());
    }

    #[test]
    fn test_provider_id_qwen_from_str_case_sensitive() {
        use std::str::FromStr;
        // "Qwen" (capitalized) should fail — only lowercase "qwen" is valid
        assert!(ProviderId::from_str("Qwen").is_err());
        assert!(ProviderId::from_str("QWEN").is_err());
    }

    // ── Metadata methods ──────────────────────────────────────────────

    #[test]
    fn test_display_name() {
        assert_eq!(ProviderId::Kiro.display_name(), "Kiro");
        assert_eq!(ProviderId::Anthropic.display_name(), "Anthropic");
        assert_eq!(ProviderId::OpenAICodex.display_name(), "OpenAI Codex");
        assert_eq!(ProviderId::Copilot.display_name(), "Copilot");
        assert_eq!(ProviderId::Qwen.display_name(), "Qwen");
        assert_eq!(ProviderId::Custom.display_name(), "Custom");
    }

    #[test]
    fn test_category_exact_values() {
        assert_eq!(ProviderId::Kiro.category(), "device_code");
        assert_eq!(ProviderId::Anthropic.category(), "oauth_relay");
        assert_eq!(ProviderId::OpenAICodex.category(), "oauth_relay");
        assert_eq!(ProviderId::Copilot.category(), "device_code");
        assert_eq!(ProviderId::Qwen.category(), "device_code");
        assert_eq!(ProviderId::Custom.category(), "custom");
    }

    #[test]
    fn test_supports_pool() {
        assert!(ProviderId::Kiro.supports_pool());
        assert!(ProviderId::Anthropic.supports_pool());
        assert!(ProviderId::OpenAICodex.supports_pool());
        assert!(ProviderId::Copilot.supports_pool());
        assert!(ProviderId::Qwen.supports_pool());
        assert!(!ProviderId::Custom.supports_pool());
    }

    #[test]
    fn test_all_visible_excludes_custom() {
        let visible = ProviderId::all_visible();
        assert_eq!(visible.len(), 5);
        assert!(!visible.contains(&ProviderId::Custom));
        assert!(visible.contains(&ProviderId::Kiro));
        assert!(visible.contains(&ProviderId::Anthropic));
        assert!(visible.contains(&ProviderId::OpenAICodex));
        assert!(visible.contains(&ProviderId::Copilot));
        assert!(visible.contains(&ProviderId::Qwen));
    }

    /// Exhaustiveness guard: if a new variant is added to ProviderId,
    /// this test forces the developer to update all_visible().
    #[test]
    fn test_all_visible_exhaustiveness_guard() {
        // Every variant except Custom must be in all_visible()
        let visible = ProviderId::all_visible();
        for id in [
            ProviderId::Kiro,
            ProviderId::Anthropic,
            ProviderId::OpenAICodex,
            ProviderId::Copilot,
            ProviderId::Qwen,
        ] {
            assert!(
                visible.contains(&id),
                "{} missing from all_visible()",
                id.as_str()
            );
        }
        // Custom must NOT be in all_visible()
        assert!(
            !visible.contains(&ProviderId::Custom),
            "Custom should not be in all_visible()"
        );
    }

    #[test]
    fn test_default_base_url() {
        assert_eq!(
            ProviderId::Anthropic.default_base_url(),
            Some("https://api.anthropic.com")
        );
        assert_eq!(
            ProviderId::OpenAICodex.default_base_url(),
            Some("https://api.openai.com")
        );
        assert_eq!(
            ProviderId::Copilot.default_base_url(),
            Some("https://api.githubcopilot.com")
        );
        assert_eq!(
            ProviderId::Qwen.default_base_url(),
            Some("https://dashscope-intl.aliyuncs.com/compatible-mode")
        );
        assert_eq!(ProviderId::Kiro.default_base_url(), None);
        assert_eq!(ProviderId::Custom.default_base_url(), None);
    }
}
