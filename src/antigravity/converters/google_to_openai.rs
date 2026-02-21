//! Google Generative AI → OpenAI Chat Completion response converter.
//!
//! Converts Google `generateContent` response candidates into OpenAI
//! `ChatCompletionChoice`, `ChatCompletionUsage`, and finish reasons.

use serde_json::Value;

use crate::models::openai::{
    ChatCompletionChoice, ChatCompletionUsage, ChatMessage, FunctionCall, ToolCall,
};

// === Content Conversion ===

/// Converts Google candidate parts into an OpenAI `ChatMessage`.
///
/// - Text parts → concatenated into `content`
/// - Thinking parts (`thought:true`) → skipped (no standard OpenAI field)
/// - `functionCall` parts → `tool_calls` array
/// - `inlineData` parts are skipped (OpenAI has no image content in responses)
pub fn convert_parts_to_chat_message(parts: &[Value]) -> ChatMessage {
    let mut text_parts: Vec<&str> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();

    for part in parts {
        // Thinking parts are skipped in OpenAI format (no standard field)
        if part.get("thought").and_then(|v| v.as_bool()) == Some(true) {
            continue;
        }

        if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
            text_parts.push(text);
            continue;
        }

        if let Some(fc) = part.get("functionCall") {
            let name = fc.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
            let id = fc
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or(name)
                .to_string();
            let args = fc
                .get("args")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "{}".to_string());

            tool_calls.push(ToolCall {
                id,
                tool_type: "function".to_string(),
                function: FunctionCall {
                    name: name.to_string(),
                    arguments: args,
                },
            });
        }
    }

    let content = if text_parts.is_empty() {
        None
    } else {
        Some(serde_json::Value::String(text_parts.join("")))
    };

    let tool_calls_opt = if tool_calls.is_empty() {
        None
    } else {
        Some(tool_calls)
    };

    ChatMessage {
        role: "assistant".to_string(),
        content,
        name: None,
        tool_calls: tool_calls_opt,
        tool_call_id: None,
    }
}

// === Finish Reason Mapping ===

/// Maps a Google `finishReason` to an OpenAI `finish_reason`.
///
/// - `STOP` → `stop`
/// - `MAX_TOKENS` → `length`
/// - `TOOL_USE` or `has_function_call` → `tool_calls`
/// - Everything else → `stop`
pub fn convert_finish_reason(reason: Option<&str>, has_function_call: bool) -> String {
    if has_function_call {
        return "tool_calls".to_string();
    }
    match reason {
        Some("STOP") => "stop".to_string(),
        Some("MAX_TOKENS") => "length".to_string(),
        Some("TOOL_USE") => "tool_calls".to_string(),
        _ => "stop".to_string(),
    }
}

// === Usage Conversion ===

/// Converts Google `usageMetadata` to OpenAI `ChatCompletionUsage`.
///
/// `prompt_tokens = promptTokenCount - cachedContentTokenCount`
pub fn convert_usage(usage_metadata: Option<&Value>) -> ChatCompletionUsage {
    let Some(meta) = usage_metadata else {
        return ChatCompletionUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            credits_used: None,
        };
    };

    let prompt_tokens = meta
        .get("promptTokenCount")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let cached_tokens = meta
        .get("cachedContentTokenCount")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let completion_tokens = meta
        .get("candidatesTokenCount")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;

    let input = prompt_tokens - cached_tokens;

    ChatCompletionUsage {
        prompt_tokens: input,
        completion_tokens,
        total_tokens: input + completion_tokens,
        credits_used: None,
    }
}

/// Builds a `ChatCompletionChoice` from Google candidate parts.
pub fn build_choice(parts: &[Value], finish_reason: Option<&str>) -> ChatCompletionChoice {
    let message = convert_parts_to_chat_message(parts);
    let has_tool_calls = message.tool_calls.is_some();
    let reason = convert_finish_reason(finish_reason, has_tool_calls);

    ChatCompletionChoice {
        index: 0,
        message,
        finish_reason: Some(reason),
        logprobs: None,
    }
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- convert_parts_to_chat_message ---

    #[test]
    fn test_text_parts_concatenated() {
        let parts = vec![json!({"text": "Hello "}), json!({"text": "world"})];
        let msg = convert_parts_to_chat_message(&parts);
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.content.unwrap().as_str().unwrap(), "Hello world");
        assert!(msg.tool_calls.is_none());
    }

    #[test]
    fn test_thinking_parts_skipped() {
        let parts = vec![
            json!({"text": "thinking...", "thought": true}),
            json!({"text": "visible"}),
        ];
        let msg = convert_parts_to_chat_message(&parts);
        assert_eq!(msg.content.unwrap().as_str().unwrap(), "visible");
    }

    #[test]
    fn test_function_call_parts() {
        let parts = vec![json!({
            "functionCall": {
                "name": "get_weather",
                "args": {"city": "NYC"},
                "id": "call_1"
            }
        })];
        let msg = convert_parts_to_chat_message(&parts);
        assert!(msg.content.is_none());
        let calls = msg.tool_calls.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].tool_type, "function");
        assert_eq!(calls[0].function.name, "get_weather");
        assert!(calls[0].function.arguments.contains("NYC"));
    }

    #[test]
    fn test_mixed_text_and_tool_calls() {
        let parts = vec![
            json!({"text": "Let me check"}),
            json!({"functionCall": {"name": "search", "args": {}, "id": "c1"}}),
        ];
        let msg = convert_parts_to_chat_message(&parts);
        assert_eq!(msg.content.unwrap().as_str().unwrap(), "Let me check");
        assert_eq!(msg.tool_calls.unwrap().len(), 1);
    }

    #[test]
    fn test_empty_parts() {
        let parts: Vec<Value> = vec![];
        let msg = convert_parts_to_chat_message(&parts);
        assert!(msg.content.is_none());
        assert!(msg.tool_calls.is_none());
    }

    #[test]
    fn test_inline_data_skipped() {
        let parts = vec![
            json!({"inlineData": {"mimeType": "image/png", "data": "abc"}}),
            json!({"text": "Here is the image"}),
        ];
        let msg = convert_parts_to_chat_message(&parts);
        assert_eq!(msg.content.unwrap().as_str().unwrap(), "Here is the image");
    }

    // --- convert_finish_reason ---

    #[test]
    fn test_finish_reason_stop() {
        assert_eq!(convert_finish_reason(Some("STOP"), false), "stop");
    }

    #[test]
    fn test_finish_reason_max_tokens() {
        assert_eq!(convert_finish_reason(Some("MAX_TOKENS"), false), "length");
    }

    #[test]
    fn test_finish_reason_tool_use() {
        assert_eq!(convert_finish_reason(Some("TOOL_USE"), false), "tool_calls");
    }

    #[test]
    fn test_finish_reason_has_function_call_overrides() {
        assert_eq!(convert_finish_reason(Some("STOP"), true), "tool_calls");
    }

    #[test]
    fn test_finish_reason_none_defaults_stop() {
        assert_eq!(convert_finish_reason(None, false), "stop");
    }

    // --- convert_usage ---

    #[test]
    fn test_usage_basic() {
        let meta = json!({
            "promptTokenCount": 100,
            "candidatesTokenCount": 50
        });
        let usage = convert_usage(Some(&meta));
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn test_usage_with_cached_tokens() {
        let meta = json!({
            "promptTokenCount": 100,
            "cachedContentTokenCount": 30,
            "candidatesTokenCount": 50
        });
        let usage = convert_usage(Some(&meta));
        assert_eq!(usage.prompt_tokens, 70);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 120);
    }

    #[test]
    fn test_usage_none() {
        let usage = convert_usage(None);
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
    }

    // --- build_choice ---

    #[test]
    fn test_build_choice_text() {
        let parts = vec![json!({"text": "Hello"})];
        let choice = build_choice(&parts, Some("STOP"));
        assert_eq!(choice.index, 0);
        assert_eq!(choice.finish_reason.unwrap(), "stop");
        assert_eq!(choice.message.content.unwrap().as_str().unwrap(), "Hello");
    }

    #[test]
    fn test_build_choice_tool_call_overrides_finish_reason() {
        let parts = vec![json!({
            "functionCall": {"name": "tool", "args": {}, "id": "c1"}
        })];
        let choice = build_choice(&parts, Some("STOP"));
        assert_eq!(choice.finish_reason.unwrap(), "tool_calls");
    }
}
