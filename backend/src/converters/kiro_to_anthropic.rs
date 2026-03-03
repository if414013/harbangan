// Kiro to Anthropic converter
//
// This module converts Kiro API responses to Anthropic Messages API format.
// Handles both streaming and non-streaming responses.

#![allow(dead_code)]

use uuid::Uuid;

use crate::models::anthropic::{AnthropicMessagesResponse, AnthropicUsage, ContentBlock};
use crate::models::kiro::KiroResponse;

/// Generates a unique message ID in Anthropic format.
fn generate_message_id() -> String {
    format!("msg_{}", &Uuid::new_v4().simple().to_string()[..24])
}

/// Converts Kiro response to Anthropic MessagesResponse.
///
/// This handles the non-streaming case where we have a complete response.
pub fn convert_kiro_to_anthropic_response(
    kiro_response: &KiroResponse,
    model: &str,
) -> AnthropicMessagesResponse {
    let message_id = generate_message_id();

    // Build content blocks
    let mut content_blocks = Vec::new();

    // Add text content from assistant response message
    for block in &kiro_response.assistant_response_message.content {
        match block {
            crate::models::kiro::ContentBlock::Text { text } => {
                if !text.is_empty() {
                    content_blocks.push(ContentBlock::Text { text: text.clone() });
                }
            }
        }
    }

    // Add tool use blocks if present
    if let Some(tool_uses) = &kiro_response.assistant_response_message.tool_uses {
        for tool_use in tool_uses {
            content_blocks.push(ContentBlock::ToolUse {
                id: tool_use.tool_use_id.clone(),
                name: tool_use.name.clone(),
                input: tool_use.input.clone(),
            });
        }
    }

    // Determine stop_reason
    let stop_reason = if kiro_response.assistant_response_message.tool_uses.is_some() {
        Some("tool_use".to_string())
    } else {
        Some("end_turn".to_string())
    };

    // Calculate usage
    let usage = if let Some(kiro_usage) = &kiro_response.usage {
        AnthropicUsage {
            input_tokens: kiro_usage.input_tokens,
            output_tokens: kiro_usage.output_tokens,
        }
    } else {
        AnthropicUsage {
            input_tokens: 0,
            output_tokens: 0,
        }
    };

    // Create response
    AnthropicMessagesResponse {
        id: message_id,
        response_type: "message".to_string(),
        model: model.to_string(),
        role: "assistant".to_string(),
        content: content_blocks,
        stop_reason,
        stop_sequence: None,
        usage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_convert_kiro_to_anthropic_simple() {
        let kiro_response = KiroResponse {
            conversation_id: "test-conv".to_string(),
            assistant_response_message: crate::models::kiro::AssistantResponseMessage {
                content: vec![crate::models::kiro::ContentBlock::Text {
                    text: "Hello, world!".to_string(),
                }],
                tool_uses: None,
            },
            usage: None,
        };

        let response = convert_kiro_to_anthropic_response(&kiro_response, "claude-sonnet-4");

        assert_eq!(response.model, "claude-sonnet-4");
        assert_eq!(response.role, "assistant");
        assert_eq!(response.content.len(), 1);

        if let ContentBlock::Text { text } = &response.content[0] {
            assert_eq!(text, "Hello, world!");
        } else {
            panic!("Expected text content block");
        }

        assert_eq!(response.stop_reason, Some("end_turn".to_string()));
    }

    #[test]
    fn test_convert_kiro_to_anthropic_with_tools() {
        let kiro_response = KiroResponse {
            conversation_id: "test-conv".to_string(),
            assistant_response_message: crate::models::kiro::AssistantResponseMessage {
                content: vec![crate::models::kiro::ContentBlock::Text {
                    text: "Let me check that.".to_string(),
                }],
                tool_uses: Some(vec![crate::models::kiro::ToolUse {
                    tool_use_id: "toolu_abc123".to_string(),
                    name: "get_weather".to_string(),
                    input: json!({"location": "San Francisco"}),
                }]),
            },
            usage: None,
        };

        let response = convert_kiro_to_anthropic_response(&kiro_response, "claude-sonnet-4");

        assert_eq!(response.content.len(), 2);

        // First block should be text
        if let ContentBlock::Text { text } = &response.content[0] {
            assert_eq!(text, "Let me check that.");
        } else {
            panic!("Expected text content block");
        }

        // Second block should be tool_use
        if let ContentBlock::ToolUse { id, name, input } = &response.content[1] {
            assert_eq!(id, "toolu_abc123");
            assert_eq!(name, "get_weather");
            assert_eq!(
                input.get("location").and_then(|v| v.as_str()),
                Some("San Francisco")
            );
        } else {
            panic!("Expected tool_use content block");
        }

        assert_eq!(response.stop_reason, Some("tool_use".to_string()));
    }

    #[test]
    fn test_generate_message_id_format() {
        let id = generate_message_id();
        assert!(id.starts_with("msg_"));
        assert_eq!(id.len(), 28); // "msg_" + 24 chars
    }
}
