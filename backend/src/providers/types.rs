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
    #[serde(rename = "openai")]
    OpenAI,
    #[serde(rename = "gemini")]
    Gemini,
}

impl ProviderId {
    /// Returns the string identifier stored in the database.
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderId::Kiro => "kiro",
            ProviderId::Anthropic => "anthropic",
            ProviderId::OpenAI => "openai",
            ProviderId::Gemini => "gemini",
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
            "openai" => Ok(ProviderId::OpenAI),
            "gemini" => Ok(ProviderId::Gemini),
            other => Err(format!("Unknown provider: {}", other)),
        }
    }
}

/// Per-user credentials resolved at request time.
#[derive(Debug, Clone)]
pub struct ProviderCredentials {
    pub provider: ProviderId,
    pub access_token: String,
    /// Override the default API endpoint (optional).
    pub base_url: Option<String>,
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
}

/// A single item in a provider streaming response.
/// Contains raw SSE bytes that the handler pipes to the client.
pub type ProviderStreamItem = Result<Bytes, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_id_as_str() {
        assert_eq!(ProviderId::Kiro.as_str(), "kiro");
        assert_eq!(ProviderId::Anthropic.as_str(), "anthropic");
        assert_eq!(ProviderId::OpenAI.as_str(), "openai");
        assert_eq!(ProviderId::Gemini.as_str(), "gemini");
    }

    #[test]
    fn test_provider_id_display() {
        assert_eq!(ProviderId::Anthropic.to_string(), "anthropic");
        assert_eq!(ProviderId::OpenAI.to_string(), "openai");
    }

    #[test]
    fn test_provider_id_from_str() {
        use std::str::FromStr;
        assert_eq!(ProviderId::from_str("kiro").unwrap(), ProviderId::Kiro);
        assert_eq!(
            ProviderId::from_str("anthropic").unwrap(),
            ProviderId::Anthropic
        );
        assert_eq!(ProviderId::from_str("openai").unwrap(), ProviderId::OpenAI);
        assert_eq!(ProviderId::from_str("gemini").unwrap(), ProviderId::Gemini);
        assert!(ProviderId::from_str("unknown").is_err());
    }

    #[test]
    fn test_provider_id_serialize() {
        let id = ProviderId::Anthropic;
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"anthropic\"");
    }

    #[test]
    fn test_provider_id_deserialize() {
        let id: ProviderId = serde_json::from_str("\"openai\"").unwrap();
        assert_eq!(id, ProviderId::OpenAI);
    }

    #[test]
    fn test_provider_credentials_clone() {
        let creds = ProviderCredentials {
            provider: ProviderId::Anthropic,
            access_token: "sk-ant-test".to_string(),
            base_url: None,
        };
        let cloned = creds.clone();
        assert_eq!(cloned.provider, ProviderId::Anthropic);
        assert_eq!(cloned.access_token, "sk-ant-test");
    }
}
