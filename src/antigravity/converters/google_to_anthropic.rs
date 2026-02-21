//! Google Generative AI → Anthropic Messages API response converter.
//!
//! Converts Google `generateContent` response candidates into Anthropic
//! `ContentBlock` arrays, stop reasons, and usage.

use serde_json::Value;

use crate::models::anthropic::{AnthropicUsage, ContentBlock};

// === Content Part Conversion ===

/// Converts a single Google part to an Anthropic `ContentBlock`.
///
/// Mapping:
/// - `{text, thought:true, thoughtSignature}` → `Thinking { thinking, signature }`
/// - `{text}` → `Text { text }`
/// - `{functionCall:{name, args, id}}` → `ToolUse { id, name, input }`
/// - `{inlineData:{mimeType, data}}` → `Image { source: Base64 {..} }`
pub fn convert_part_to_content_block(part: &Value) -> Option<ContentBlock> {
    // Thinking part
    if part.get("thought").and_then(|v| v.as_bool()) == Some(true) {
        let thinking = part.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let signature = part
            .get("thoughtSignature")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Some(ContentBlock::Thinking {
            thinking: thinking.to_string(),
            signature,
        });
    }

    // Text part
    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
        return Some(ContentBlock::Text {
            text: text.to_string(),
        });
    }

    // Function call part
    if let Some(fc) = part.get("functionCall") {
        let name = fc.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
        let id = fc
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or(name)
            .to_string();
        let input = fc
            .get("args")
            .cloned()
            .unwrap_or(Value::Object(Default::default()));
        return Some(ContentBlock::ToolUse {
            id,
            name: name.to_string(),
            input,
        });
    }

    // Inline data (image) part
    if let Some(inline) = part.get("inlineData") {
        let mime = inline
            .get("mimeType")
            .and_then(|v| v.as_str())
            .unwrap_or("image/jpeg")
            .to_string();
        let data = inline
            .get("data")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Some(ContentBlock::Image {
            source: crate::models::anthropic::ImageSource::Base64 {
                media_type: mime,
                data,
            },
        });
    }

    None
}

/// Converts all parts from a Google candidate into Anthropic content blocks.
pub fn convert_parts_to_content_blocks(parts: &[Value]) -> Vec<ContentBlock> {
    parts
        .iter()
        .filter_map(convert_part_to_content_block)
        .collect()
}

// === Finish Reason Mapping ===

/// Maps a Google `finishReason` to an Anthropic `stop_reason`.
///
/// - `STOP` → `end_turn`
/// - `MAX_TOKENS` → `max_tokens`
/// - `TOOL_USE` → `tool_use`
/// - If `has_function_call` is true → `tool_use`
/// - Everything else → `end_turn`
pub fn convert_finish_reason(reason: Option<&str>, has_function_call: bool) -> String {
    if has_function_call {
        return "tool_use".to_string();
    }
    match reason {
        Some("STOP") => "end_turn".to_string(),
        Some("MAX_TOKENS") => "max_tokens".to_string(),
        Some("TOOL_USE") => "tool_use".to_string(),
        _ => "end_turn".to_string(),
    }
}

// === Usage Conversion ===

/// Converts Google `usageMetadata` to Anthropic `AnthropicUsage`.
///
/// `input_tokens = promptTokenCount - cachedContentTokenCount`
pub fn convert_usage(usage_metadata: Option<&Value>) -> AnthropicUsage {
    let Some(meta) = usage_metadata else {
        return AnthropicUsage {
            input_tokens: 0,
            output_tokens: 0,
        };
    };

    let prompt_tokens = meta
        .get("promptTokenCount")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let cached_tokens = meta
        .get("cachedContentTokenCount")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let output_tokens = meta
        .get("candidatesTokenCount")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    AnthropicUsage {
        input_tokens: (prompt_tokens - cached_tokens).max(0) as i32,
        output_tokens: output_tokens as i32,
    }
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- convert_part_to_content_block ---

    #[test]
    fn test_text_part() {
        let part = json!({"text": "Hello world"});
        let block = convert_part_to_content_block(&part).unwrap();
        match block {
            ContentBlock::Text { text } => assert_eq!(text, "Hello world"),
            _ => panic!("Expected Text block"),
        }
    }

    #[test]
    fn test_thinking_part() {
        let part = json!({
            "text": "Let me reason...",
            "thought": true,
            "thoughtSignature": "sig123abc"
        });
        let block = convert_part_to_content_block(&part).unwrap();
        match block {
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                assert_eq!(thinking, "Let me reason...");
                assert_eq!(signature, "sig123abc");
            }
            _ => panic!("Expected Thinking block"),
        }
    }

    #[test]
    fn test_thinking_part_without_signature() {
        let part = json!({"text": "thinking", "thought": true});
        let block = convert_part_to_content_block(&part).unwrap();
        match block {
            ContentBlock::Thinking { signature, .. } => assert_eq!(signature, ""),
            _ => panic!("Expected Thinking block"),
        }
    }

    #[test]
    fn test_function_call_part() {
        let part = json!({
            "functionCall": {
                "name": "get_weather",
                "args": {"city": "Seattle"},
                "id": "call_1"
            }
        });
        let block = convert_part_to_content_block(&part).unwrap();
        match block {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "get_weather");
                assert_eq!(input["city"], "Seattle");
            }
            _ => panic!("Expected ToolUse block"),
        }
    }

    #[test]
    fn test_function_call_id_falls_back_to_name() {
        let part = json!({"functionCall": {"name": "my_tool", "args": {}}});
        let block = convert_part_to_content_block(&part).unwrap();
        match block {
            ContentBlock::ToolUse { id, name, .. } => {
                assert_eq!(id, "my_tool");
                assert_eq!(name, "my_tool");
            }
            _ => panic!("Expected ToolUse block"),
        }
    }

    #[test]
    fn test_inline_data_part() {
        let part = json!({
            "inlineData": {"mimeType": "image/png", "data": "iVBORw0KGgo="}
        });
        let block = convert_part_to_content_block(&part).unwrap();
        match block {
            ContentBlock::Image { source } => match source {
                crate::models::anthropic::ImageSource::Base64 { media_type, data } => {
                    assert_eq!(media_type, "image/png");
                    assert_eq!(data, "iVBORw0KGgo=");
                }
                _ => panic!("Expected Base64 source"),
            },
            _ => panic!("Expected Image block"),
        }
    }

    #[test]
    fn test_unknown_part_returns_none() {
        let part = json!({"unknownField": "value"});
        assert!(convert_part_to_content_block(&part).is_none());
    }

    // --- convert_parts_to_content_blocks ---

    #[test]
    fn test_convert_multiple_parts() {
        let parts = vec![
            json!({"text": "Hello"}),
            json!({"unknownField": true}),
            json!({"text": "World"}),
        ];
        let blocks = convert_parts_to_content_blocks(&parts);
        assert_eq!(blocks.len(), 2);
    }

    // --- convert_finish_reason ---

    #[test]
    fn test_finish_reason_stop() {
        assert_eq!(convert_finish_reason(Some("STOP"), false), "end_turn");
    }

    #[test]
    fn test_finish_reason_max_tokens() {
        assert_eq!(
            convert_finish_reason(Some("MAX_TOKENS"), false),
            "max_tokens"
        );
    }

    #[test]
    fn test_finish_reason_tool_use() {
        assert_eq!(convert_finish_reason(Some("TOOL_USE"), false), "tool_use");
    }

    #[test]
    fn test_finish_reason_has_function_call_overrides() {
        assert_eq!(convert_finish_reason(Some("STOP"), true), "tool_use");
    }

    #[test]
    fn test_finish_reason_none_defaults_end_turn() {
        assert_eq!(convert_finish_reason(None, false), "end_turn");
    }

    // --- convert_usage ---

    #[test]
    fn test_usage_basic() {
        let meta = json!({
            "promptTokenCount": 100,
            "candidatesTokenCount": 50
        });
        let usage = convert_usage(Some(&meta));
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
    }

    #[test]
    fn test_usage_with_cached_tokens() {
        let meta = json!({
            "promptTokenCount": 100,
            "cachedContentTokenCount": 30,
            "candidatesTokenCount": 50
        });
        let usage = convert_usage(Some(&meta));
        assert_eq!(usage.input_tokens, 70);
        assert_eq!(usage.output_tokens, 50);
    }

    #[test]
    fn test_usage_none() {
        let usage = convert_usage(None);
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
    }

    #[test]
    fn test_usage_missing_fields() {
        let meta = json!({});
        let usage = convert_usage(Some(&meta));
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
    }
}
