//! Content conversion from Anthropic format to Google Generative AI parts.
//!
//! Converts Anthropic message content blocks (text, image, tool_use, tool_result,
//! thinking) into Google's `parts` array format used by the Cloud Code API.

use serde_json::{json, Value};
use tracing::debug;

use crate::antigravity::constants::GEMINI_SKIP_SIGNATURE;

/// Minimum length for a valid thinking signature.
const MIN_SIGNATURE_LENGTH: usize = 50;

/// Converts an Anthropic role to a Google role.
///
/// - `"assistant"` → `"model"`
/// - everything else → `"user"`
pub fn convert_role(role: &str) -> &'static str {
    match role {
        "assistant" => "model",
        _ => "user",
    }
}

/// Converts Anthropic message content to Google Generative AI parts.
///
/// Handles string content, arrays of content blocks, and falls back to
/// stringifying unknown shapes. Supports text, image, tool_use, tool_result,
/// and thinking block types.
pub fn convert_content_to_parts(content: &Value, is_claude: bool, is_gemini: bool) -> Vec<Value> {
    // String content → single text part
    if let Some(text) = content.as_str() {
        return vec![json!({ "text": text })];
    }

    // Non-array, non-string → stringify
    let blocks = match content.as_array() {
        Some(arr) => arr,
        None => return vec![json!({ "text": content.to_string() })],
    };

    let mut parts = Vec::new();
    let mut deferred_inline_data = Vec::new();

    for block in blocks {
        let block_type = match block.get("type").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => continue,
        };

        match block_type {
            "text" => {
                let text = block.get("text").and_then(|v| v.as_str()).unwrap_or("");
                if !text.trim().is_empty() {
                    parts.push(json!({ "text": text }));
                }
            }

            "image" => {
                if let Some(source) = block.get("source") {
                    let source_type = source.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    match source_type {
                        "base64" => {
                            let mime = source
                                .get("media_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("image/jpeg");
                            let data = source.get("data").and_then(|v| v.as_str()).unwrap_or("");
                            parts.push(json!({
                                "inlineData": {
                                    "mimeType": mime,
                                    "data": data
                                }
                            }));
                        }
                        "url" => {
                            let mime = source
                                .get("media_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("image/jpeg");
                            let url = source.get("url").and_then(|v| v.as_str()).unwrap_or("");
                            parts.push(json!({
                                "fileData": {
                                    "mimeType": mime,
                                    "fileUri": url
                                }
                            }));
                        }
                        _ => {}
                    }
                }
            }

            "tool_use" => {
                let name = block
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let args = block.get("input").cloned().unwrap_or(json!({}));

                let mut function_call = json!({
                    "name": name,
                    "args": args
                });

                // Claude models need the id field on functionCall
                if is_claude {
                    if let Some(id) = block.get("id").and_then(|v| v.as_str()) {
                        function_call
                            .as_object_mut()
                            .unwrap()
                            .insert("id".to_string(), json!(id));
                    }
                }

                let mut part = json!({ "functionCall": function_call });

                // Gemini models need thoughtSignature at the part level
                if is_gemini {
                    let signature = block
                        .get("thoughtSignature")
                        .and_then(|v| v.as_str())
                        .unwrap_or(GEMINI_SKIP_SIGNATURE);
                    part.as_object_mut()
                        .unwrap()
                        .insert("thoughtSignature".to_string(), json!(signature));
                }

                parts.push(part);
            }

            "tool_result" => {
                let tool_use_id = block
                    .get("tool_use_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let raw_content = block.get("content");

                let (response_content, image_parts) = convert_tool_result_content(raw_content);

                let mut function_response = json!({
                    "name": tool_use_id,
                    "response": response_content
                });

                // Claude models need the id field matching tool_use_id
                if is_claude {
                    function_response
                        .as_object_mut()
                        .unwrap()
                        .insert("id".to_string(), json!(tool_use_id));
                }

                parts.push(json!({ "functionResponse": function_response }));

                // Defer images to end of parts array so functionResponse parts stay consecutive
                deferred_inline_data.extend(image_parts);
            }

            "thinking" => {
                let signature = block
                    .get("signature")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if signature.len() >= MIN_SIGNATURE_LENGTH {
                    let thinking_text =
                        block.get("thinking").and_then(|v| v.as_str()).unwrap_or("");
                    parts.push(json!({
                        "text": thinking_text,
                        "thought": true,
                        "thoughtSignature": signature
                    }));
                }
                // Unsigned thinking blocks are dropped
            }

            _ => {
                debug!(block_type, "Skipping unknown content block type");
            }
        }
    }

    // Append deferred inline data at the end
    parts.extend(deferred_inline_data);

    parts
}

/// Converts tool_result content to a Google functionResponse `response` value,
/// plus any image parts that should be deferred.
fn convert_tool_result_content(content: Option<&Value>) -> (Value, Vec<Value>) {
    let Some(content) = content else {
        return (json!({ "result": "" }), vec![]);
    };

    // String content
    if let Some(text) = content.as_str() {
        return (json!({ "result": text }), vec![]);
    }

    // Array of content blocks
    if let Some(blocks) = content.as_array() {
        let mut image_parts = Vec::new();

        // Extract images
        for item in blocks {
            let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if item_type == "image" {
                if let Some(source) = item.get("source") {
                    if source.get("type").and_then(|v| v.as_str()) == Some("base64") {
                        let mime = source
                            .get("media_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("image/jpeg");
                        let data = source.get("data").and_then(|v| v.as_str()).unwrap_or("");
                        image_parts.push(json!({
                            "inlineData": {
                                "mimeType": mime,
                                "data": data
                            }
                        }));
                    }
                }
            }
        }

        // Extract text
        let texts: Vec<&str> = blocks
            .iter()
            .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("text"))
            .filter_map(|b| b.get("text").and_then(|v| v.as_str()))
            .collect();

        let result_text = if texts.is_empty() {
            if !image_parts.is_empty() {
                "Image attached"
            } else {
                ""
            }
        } else {
            // We need to own this, so we'll handle it below
            return (json!({ "result": texts.join("\n") }), image_parts);
        };

        return (json!({ "result": result_text }), image_parts);
    }

    // Fallback: pass through as-is
    (content.clone(), vec![])
}

/// Strips `cache_control` fields from all message content blocks.
///
/// Cloud Code API rejects `cache_control` with "Extra inputs are not permitted".
pub fn clean_cache_control(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .map(|msg| {
            let mut msg = msg.clone();
            if let Some(obj) = msg.as_object_mut() {
                // Clean cache_control from content blocks
                if let Some(content) = obj.get_mut("content") {
                    strip_cache_control(content);
                }
            }
            msg
        })
        .collect()
}

/// Recursively strips `cache_control` from a value.
fn strip_cache_control(value: &mut Value) {
    match value {
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                strip_cache_control(item);
            }
        }
        Value::Object(map) => {
            map.remove("cache_control");
            for (_, v) in map.iter_mut() {
                strip_cache_control(v);
            }
        }
        _ => {}
    }
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // === convert_role ===

    #[test]
    fn test_convert_role_assistant() {
        assert_eq!(convert_role("assistant"), "model");
    }

    #[test]
    fn test_convert_role_user() {
        assert_eq!(convert_role("user"), "user");
    }

    #[test]
    fn test_convert_role_unknown() {
        assert_eq!(convert_role("system"), "user");
    }

    // === convert_content_to_parts: text ===

    #[test]
    fn test_text_string() {
        let parts = convert_content_to_parts(&json!("Hello"), false, false);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], "Hello");
    }

    #[test]
    fn test_text_block() {
        let content = json!([{"type": "text", "text": "Hello world"}]);
        let parts = convert_content_to_parts(&content, false, false);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], "Hello world");
    }

    #[test]
    fn test_empty_text_block_skipped() {
        let content = json!([
            {"type": "text", "text": ""},
            {"type": "text", "text": "  "},
            {"type": "text", "text": "valid"}
        ]);
        let parts = convert_content_to_parts(&content, false, false);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], "valid");
    }

    // === convert_content_to_parts: image ===

    #[test]
    fn test_image_base64() {
        let content = json!([{
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": "image/png",
                "data": "iVBORw0KGgo="
            }
        }]);
        let parts = convert_content_to_parts(&content, false, false);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["inlineData"]["mimeType"], "image/png");
        assert_eq!(parts[0]["inlineData"]["data"], "iVBORw0KGgo=");
    }

    #[test]
    fn test_image_url() {
        let content = json!([{
            "type": "image",
            "source": {
                "type": "url",
                "media_type": "image/jpeg",
                "url": "https://example.com/img.jpg"
            }
        }]);
        let parts = convert_content_to_parts(&content, false, false);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["fileData"]["mimeType"], "image/jpeg");
        assert_eq!(
            parts[0]["fileData"]["fileUri"],
            "https://example.com/img.jpg"
        );
    }

    // === convert_content_to_parts: tool_use ===

    #[test]
    fn test_tool_use_basic() {
        let content = json!([{
            "type": "tool_use",
            "id": "call_1",
            "name": "get_weather",
            "input": {"city": "Seattle"}
        }]);
        let parts = convert_content_to_parts(&content, false, false);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["functionCall"]["name"], "get_weather");
        assert_eq!(parts[0]["functionCall"]["args"]["city"], "Seattle");
        // No id for non-Claude
        assert!(parts[0]["functionCall"].get("id").is_none());
    }

    #[test]
    fn test_tool_use_claude_includes_id() {
        let content = json!([{
            "type": "tool_use",
            "id": "call_1",
            "name": "get_weather",
            "input": {}
        }]);
        let parts = convert_content_to_parts(&content, true, false);
        assert_eq!(parts[0]["functionCall"]["id"], "call_1");
    }

    #[test]
    fn test_tool_use_gemini_includes_signature() {
        let content = json!([{
            "type": "tool_use",
            "id": "call_1",
            "name": "get_weather",
            "input": {}
        }]);
        let parts = convert_content_to_parts(&content, false, true);
        assert_eq!(parts[0]["thoughtSignature"], GEMINI_SKIP_SIGNATURE);
    }

    // === convert_content_to_parts: tool_result ===

    #[test]
    fn test_tool_result_string() {
        let content = json!([{
            "type": "tool_result",
            "tool_use_id": "call_1",
            "content": "72F and sunny"
        }]);
        let parts = convert_content_to_parts(&content, false, false);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["functionResponse"]["name"], "call_1");
        assert_eq!(
            parts[0]["functionResponse"]["response"]["result"],
            "72F and sunny"
        );
    }

    #[test]
    fn test_tool_result_blocks() {
        let content = json!([{
            "type": "tool_result",
            "tool_use_id": "call_1",
            "content": [{"type": "text", "text": "Result text"}]
        }]);
        let parts = convert_content_to_parts(&content, false, false);
        assert_eq!(
            parts[0]["functionResponse"]["response"]["result"],
            "Result text"
        );
    }

    #[test]
    fn test_tool_result_claude_includes_id() {
        let content = json!([{
            "type": "tool_result",
            "tool_use_id": "call_1",
            "content": "result"
        }]);
        let parts = convert_content_to_parts(&content, true, false);
        assert_eq!(parts[0]["functionResponse"]["id"], "call_1");
    }

    // === convert_content_to_parts: thinking ===

    #[test]
    fn test_thinking_with_valid_signature() {
        let sig = "a".repeat(MIN_SIGNATURE_LENGTH);
        let content = json!([{
            "type": "thinking",
            "thinking": "Let me think...",
            "signature": sig
        }]);
        let parts = convert_content_to_parts(&content, false, false);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], "Let me think...");
        assert_eq!(parts[0]["thought"], true);
        assert_eq!(parts[0]["thoughtSignature"], sig);
    }

    #[test]
    fn test_thinking_without_signature_dropped() {
        let content = json!([{
            "type": "thinking",
            "thinking": "Let me think...",
            "signature": ""
        }]);
        let parts = convert_content_to_parts(&content, false, false);
        assert!(parts.is_empty());
    }

    #[test]
    fn test_thinking_short_signature_dropped() {
        let content = json!([{
            "type": "thinking",
            "thinking": "Let me think...",
            "signature": "short"
        }]);
        let parts = convert_content_to_parts(&content, false, false);
        assert!(parts.is_empty());
    }

    // === convert_content_to_parts: mixed ===

    #[test]
    fn test_mixed_content() {
        let content = json!([
            {"type": "text", "text": "Hello"},
            {"type": "tool_use", "id": "c1", "name": "read", "input": {"path": "/tmp"}},
            {"type": "unknown_type", "data": "ignored"}
        ]);
        let parts = convert_content_to_parts(&content, false, false);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["text"], "Hello");
        assert!(parts[1].get("functionCall").is_some());
    }

    #[test]
    fn test_non_array_non_string_content() {
        let content = json!(42);
        let parts = convert_content_to_parts(&content, false, false);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], "42");
    }

    // === clean_cache_control ===

    #[test]
    fn test_clean_cache_control_strips_field() {
        let messages = vec![json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "Hello", "cache_control": {"type": "ephemeral"}},
                {"type": "text", "text": "World"}
            ]
        })];
        let cleaned = clean_cache_control(&messages);
        let blocks = cleaned[0]["content"].as_array().unwrap();
        assert!(blocks[0].get("cache_control").is_none());
        assert_eq!(blocks[0]["text"], "Hello");
        assert_eq!(blocks[1]["text"], "World");
    }

    #[test]
    fn test_clean_cache_control_preserves_other_fields() {
        let messages = vec![json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "Hi"}]
        })];
        let cleaned = clean_cache_control(&messages);
        assert_eq!(cleaned[0]["role"], "assistant");
        assert_eq!(cleaned[0]["content"][0]["text"], "Hi");
    }

    // === convert_tool_result_content ===

    #[test]
    fn test_tool_result_content_none() {
        let (response, images) = convert_tool_result_content(None);
        assert_eq!(response["result"], "");
        assert!(images.is_empty());
    }

    #[test]
    fn test_tool_result_content_with_images() {
        let content = json!([
            {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "abc"}},
            {"type": "text", "text": "Screenshot taken"}
        ]);
        let (response, images) = convert_tool_result_content(Some(&content));
        assert_eq!(response["result"], "Screenshot taken");
        assert_eq!(images.len(), 1);
        assert_eq!(images[0]["inlineData"]["mimeType"], "image/png");
    }

    #[test]
    fn test_tool_result_content_images_only() {
        let content = json!([
            {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "abc"}}
        ]);
        let (response, images) = convert_tool_result_content(Some(&content));
        assert_eq!(response["result"], "Image attached");
        assert_eq!(images.len(), 1);
    }
}
