/// Convert a Gemini generateContent response body to Anthropic AnthropicMessagesResponse.
use crate::models::anthropic::{AnthropicMessagesResponse, AnthropicUsage, ContentBlock};
use serde_json::Value;

/// Convert a Gemini `generateContent` response JSON body to an Anthropic-format
/// `AnthropicMessagesResponse`.
///
/// Gemini response shape:
/// ```json
/// {
///   "candidates": [{ "content": { "role": "model", "parts": [{ "text": "..." }] }, "finishReason": "STOP" }],
///   "usageMetadata": { "promptTokenCount": 5, "candidatesTokenCount": 10 }
/// }
/// ```
///
/// Mapping rules:
/// - `candidates[0].content.parts[0].text` → `content[0]` as `ContentBlock::Text`
/// - `candidates[0].finishReason` → `stop_reason` (STOP → "end_turn", MAX_TOKENS → "max_tokens", others → lowercase)
/// - `usageMetadata.promptTokenCount` → `usage.input_tokens`
/// - `usageMetadata.candidatesTokenCount` → `usage.output_tokens`
pub fn gemini_to_anthropic(model: &str, body: &Value) -> AnthropicMessagesResponse {
    let text = body
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    let stop_reason = body
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("finishReason"))
        .and_then(|r| r.as_str())
        .map(|r| match r {
            "STOP" => "end_turn".to_string(),
            "MAX_TOKENS" => "max_tokens".to_string(),
            other => other.to_lowercase(),
        });

    let usage = body
        .get("usageMetadata")
        .map(|u| AnthropicUsage {
            input_tokens: u
                .get("promptTokenCount")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32,
            output_tokens: u
                .get("candidatesTokenCount")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32,
        })
        .unwrap_or(AnthropicUsage {
            input_tokens: 0,
            output_tokens: 0,
        });

    let content = vec![ContentBlock::Text { text }];

    let mut resp = AnthropicMessagesResponse::new(
        "gemini-response".to_string(),
        model.to_string(),
        content,
        usage,
    );
    resp.stop_reason = stop_reason;
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_gemini_response(text: &str, finish_reason: &str) -> Value {
        json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{ "text": text }]
                },
                "finishReason": finish_reason,
                "index": 0
            }],
            "usageMetadata": {
                "promptTokenCount": 8,
                "candidatesTokenCount": 15,
                "totalTokenCount": 23
            }
        })
    }

    #[test]
    fn test_text_extracted_to_content_block() {
        let body = make_gemini_response("Hello!", "STOP");
        let resp = gemini_to_anthropic("claude-sonnet-4", &body);
        assert_eq!(resp.content.len(), 1);
        if let ContentBlock::Text { text } = &resp.content[0] {
            assert_eq!(text, "Hello!");
        } else {
            panic!("Expected Text content block");
        }
    }

    #[test]
    fn test_stop_finish_reason_becomes_end_turn() {
        let body = make_gemini_response("Hi", "STOP");
        let resp = gemini_to_anthropic("gemini-2.5-pro", &body);
        assert_eq!(resp.stop_reason, Some("end_turn".to_string()));
    }

    #[test]
    fn test_max_tokens_finish_reason_mapped() {
        let body = make_gemini_response("Hi", "MAX_TOKENS");
        let resp = gemini_to_anthropic("gemini-2.5-pro", &body);
        assert_eq!(resp.stop_reason, Some("max_tokens".to_string()));
    }

    #[test]
    fn test_other_finish_reason_lowercased() {
        let body = make_gemini_response("Hi", "SAFETY");
        let resp = gemini_to_anthropic("gemini-2.5-pro", &body);
        assert_eq!(resp.stop_reason, Some("safety".to_string()));
    }

    #[test]
    fn test_usage_metadata_mapped() {
        let body = make_gemini_response("Hi", "STOP");
        let resp = gemini_to_anthropic("gemini-2.5-pro", &body);
        assert_eq!(resp.usage.input_tokens, 8);
        assert_eq!(resp.usage.output_tokens, 15);
    }

    #[test]
    fn test_model_forwarded() {
        let body = make_gemini_response("Hi", "STOP");
        let resp = gemini_to_anthropic("gemini-2.5-flash", &body);
        assert_eq!(resp.model, "gemini-2.5-flash");
    }

    #[test]
    fn test_role_is_assistant() {
        let body = make_gemini_response("Hi", "STOP");
        let resp = gemini_to_anthropic("gemini-2.5-pro", &body);
        assert_eq!(resp.role, "assistant");
    }

    #[test]
    fn test_response_type_is_message() {
        let body = make_gemini_response("Hi", "STOP");
        let resp = gemini_to_anthropic("gemini-2.5-pro", &body);
        assert_eq!(resp.response_type, "message");
    }

    #[test]
    fn test_empty_body_does_not_panic() {
        let body = json!({});
        let resp = gemini_to_anthropic("gemini-2.5-pro", &body);
        assert_eq!(resp.content.len(), 1);
        if let ContentBlock::Text { text } = &resp.content[0] {
            assert!(text.is_empty());
        } else {
            panic!("Expected Text content block");
        }
        assert!(resp.stop_reason.is_none());
        assert_eq!(resp.usage.input_tokens, 0);
        assert_eq!(resp.usage.output_tokens, 0);
    }

    #[test]
    fn test_no_usage_metadata_gives_zero_usage() {
        let body = json!({
            "candidates": [{
                "content": { "role": "model", "parts": [{ "text": "Hi" }] },
                "finishReason": "STOP"
            }]
        });
        let resp = gemini_to_anthropic("gemini-2.5-pro", &body);
        assert_eq!(resp.usage.input_tokens, 0);
        assert_eq!(resp.usage.output_tokens, 0);
    }
}
