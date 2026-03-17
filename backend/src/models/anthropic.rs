#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ==================================================================================================
// Content Block Models
// ==================================================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
        #[serde(default)]
        signature: String,
    },
    Image {
        source: ImageSource,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    RedactedThinking {
        data: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    Base64 { media_type: String, data: String },
    Url { url: String },
}

// ==================================================================================================
// Message Models
// ==================================================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: serde_json::Value,
}

// ==================================================================================================
// Tool Models
// ==================================================================================================

/// Anthropic tool definition — either a regular custom tool or a server-side tool.
///
/// Regular tools have `name`, `description`, `input_schema`.
/// Server-side tools (web_search, web_fetch, bash, text_editor, etc.) have a `type`
/// field like `web_search_20250305` and no `input_schema`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnthropicTool {
    /// Regular custom tool with input_schema
    Custom(AnthropicCustomTool),
    /// Server-side tool (web_search, web_fetch, bash, text_editor, etc.)
    ServerSide(AnthropicServerSideTool),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicCustomTool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicServerSideTool {
    /// Versioned type identifier (e.g., "web_search_20250305", "web_fetch_20250910")
    #[serde(rename = "type")]
    pub tool_type: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<i32>,
    /// All other fields (allowed_domains, blocked_domains, user_location, etc.)
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolChoice {
    Auto,
    Any,
    Tool { name: String },
}

// ==================================================================================================
// Request Models
// ==================================================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemContentBlock {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessagesRequest {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    pub max_tokens: i32,

    // Optional parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<serde_json::Value>,
    #[serde(default)]
    pub stream: bool,

    // Tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,

    // Sampling parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,

    // Thinking/reasoning config
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<serde_json::Value>,

    // Other parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_parallel_tool_use: Option<bool>,
}

// ==================================================================================================
// Response Models
// ==================================================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicUsage {
    pub input_tokens: i32,
    pub output_tokens: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessagesResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub response_type: String,
    pub role: String,
    pub content: Vec<ContentBlock>,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    pub usage: AnthropicUsage,
}

impl AnthropicMessagesResponse {
    pub fn new(
        id: String,
        model: String,
        content: Vec<ContentBlock>,
        usage: AnthropicUsage,
    ) -> Self {
        Self {
            id,
            response_type: "message".to_string(),
            role: "assistant".to_string(),
            content,
            model,
            stop_reason: None,
            stop_sequence: None,
            usage,
        }
    }
}

// ==================================================================================================
// Streaming Event Models
// ==================================================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    MessageStart {
        message: serde_json::Value,
    },
    ContentBlockStart {
        index: i32,
        content_block: serde_json::Value,
    },
    ContentBlockDelta {
        index: i32,
        delta: Delta,
    },
    ContentBlockStop {
        index: i32,
    },
    MessageDelta {
        delta: serde_json::Value,
        usage: MessageDeltaUsage,
    },
    MessageStop,
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub enum Delta {
    TextDelta { text: String },
    ThinkingDelta { thinking: String },
    SignatureDelta { signature: String },
    InputJsonDelta { partial_json: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDeltaUsage {
    pub output_tokens: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageStartData {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub role: String,
    pub content: Vec<serde_json::Value>,
    pub model: String,
    pub usage: AnthropicUsage,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_anthropic_usage_with_cache_fields() {
        let usage = AnthropicUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: Some(20),
            cache_read_input_tokens: Some(80),
        };
        let json = serde_json::to_value(&usage).unwrap();
        assert_eq!(json["cache_creation_input_tokens"], 20);
        assert_eq!(json["cache_read_input_tokens"], 80);
    }

    #[test]
    fn test_anthropic_usage_without_cache_fields() {
        let json = json!({
            "input_tokens": 100,
            "output_tokens": 50
        });
        let usage: AnthropicUsage = serde_json::from_value(json).unwrap();
        assert!(usage.cache_creation_input_tokens.is_none());
        assert!(usage.cache_read_input_tokens.is_none());
    }

    #[test]
    fn test_anthropic_usage_cache_fields_skip_serializing_none() {
        let usage = AnthropicUsage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        };
        let json = serde_json::to_value(&usage).unwrap();
        assert!(json.get("cache_creation_input_tokens").is_none());
        assert!(json.get("cache_read_input_tokens").is_none());
    }

    #[test]
    fn test_redacted_thinking_serde() {
        let block = ContentBlock::RedactedThinking {
            data: "abc123encrypted".to_string(),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "redacted_thinking");
        assert_eq!(json["data"], "abc123encrypted");

        // Round-trip
        let parsed: ContentBlock = serde_json::from_value(json).unwrap();
        if let ContentBlock::RedactedThinking { data } = parsed {
            assert_eq!(data, "abc123encrypted");
        } else {
            panic!("expected RedactedThinking variant");
        }
    }

    #[test]
    fn test_signature_delta_serde() {
        let delta = Delta::SignatureDelta {
            signature: "sig_abc123".to_string(),
        };
        let json = serde_json::to_value(&delta).unwrap();
        assert_eq!(json["type"], "signature_delta");
        assert_eq!(json["signature"], "sig_abc123");

        // Round-trip
        let parsed: Delta = serde_json::from_value(json).unwrap();
        if let Delta::SignatureDelta { signature } = parsed {
            assert_eq!(signature, "sig_abc123");
        } else {
            panic!("expected SignatureDelta variant");
        }
    }
}
