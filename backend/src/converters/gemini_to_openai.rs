/// Convert a Gemini generateContent response body to OpenAI ChatCompletionResponse.
use crate::models::openai::{
    ChatCompletionChoice, ChatCompletionResponse, ChatCompletionUsage, ChatMessage,
};
use serde_json::Value;

/// Convert a Gemini `generateContent` response JSON body to an OpenAI-format
/// `ChatCompletionResponse`.
///
/// Gemini response shape:
/// ```json
/// {
///   "candidates": [{ "content": { "role": "model", "parts": [{ "text": "..." }] }, "finishReason": "STOP" }],
///   "usageMetadata": { "promptTokenCount": 5, "candidatesTokenCount": 10, "totalTokenCount": 15 }
/// }
/// ```
///
/// Mapping rules:
/// - `candidates[0].content.parts[0].text` → `choices[0].message.content`
/// - `candidates[0].finishReason` → `choices[0].finish_reason` (normalised to lowercase)
/// - `usageMetadata.promptTokenCount` → `usage.prompt_tokens`
/// - `usageMetadata.candidatesTokenCount` → `usage.completion_tokens`
pub fn gemini_to_openai(model: &str, body: &Value) -> ChatCompletionResponse {
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

    let finish_reason = body
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("finishReason"))
        .and_then(|r| r.as_str())
        .map(|r| r.to_lowercase())
        .map(|r| if r == "stop" { "stop".to_string() } else { r });

    let usage = body.get("usageMetadata").map(|u| {
        let prompt = u
            .get("promptTokenCount")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let completion = u
            .get("candidatesTokenCount")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        ChatCompletionUsage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
            credits_used: None,
        }
    });

    let message = ChatMessage {
        role: "assistant".to_string(),
        content: Some(serde_json::Value::String(text)),
        name: None,
        tool_calls: None,
        tool_call_id: None,
    };

    let choice = ChatCompletionChoice {
        index: 0,
        message,
        finish_reason,
        logprobs: None,
    };

    let mut resp = ChatCompletionResponse::new(
        "gemini-response".to_string(),
        model.to_string(),
        vec![choice],
    );
    resp.usage = usage;
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
                "promptTokenCount": 10,
                "candidatesTokenCount": 20,
                "totalTokenCount": 30
            }
        })
    }

    #[test]
    fn test_text_extracted_to_message_content() {
        let body = make_gemini_response("Hello world", "STOP");
        let resp = gemini_to_openai("gemini-2.5-pro", &body);
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.role, "assistant");
        assert_eq!(resp.choices[0].message.content, Some(json!("Hello world")));
    }

    #[test]
    fn test_finish_reason_stop_normalised_to_lowercase() {
        let body = make_gemini_response("Hi", "STOP");
        let resp = gemini_to_openai("gemini-2.5-pro", &body);
        assert_eq!(resp.choices[0].finish_reason, Some("stop".to_string()));
    }

    #[test]
    fn test_finish_reason_other_normalised_to_lowercase() {
        let body = make_gemini_response("Hi", "MAX_TOKENS");
        let resp = gemini_to_openai("gemini-2.5-pro", &body);
        assert_eq!(
            resp.choices[0].finish_reason,
            Some("max_tokens".to_string())
        );
    }

    #[test]
    fn test_usage_metadata_mapped() {
        let body = make_gemini_response("Hi", "STOP");
        let resp = gemini_to_openai("gemini-2.5-pro", &body);
        let usage = resp.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }

    #[test]
    fn test_model_forwarded() {
        let body = make_gemini_response("Hi", "STOP");
        let resp = gemini_to_openai("gemini-2.5-flash", &body);
        assert_eq!(resp.model, "gemini-2.5-flash");
    }

    #[test]
    fn test_empty_body_does_not_panic() {
        let body = json!({});
        let resp = gemini_to_openai("gemini-2.5-pro", &body);
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content, Some(json!("")));
        assert!(resp.choices[0].finish_reason.is_none());
        assert!(resp.usage.is_none());
    }

    #[test]
    fn test_no_usage_metadata_gives_none_usage() {
        let body = json!({
            "candidates": [{
                "content": { "role": "model", "parts": [{ "text": "Hi" }] },
                "finishReason": "STOP"
            }]
        });
        let resp = gemini_to_openai("gemini-2.5-pro", &body);
        assert!(resp.usage.is_none());
    }

    #[test]
    fn test_choice_index_is_zero() {
        let body = make_gemini_response("Hi", "STOP");
        let resp = gemini_to_openai("gemini-2.5-pro", &body);
        assert_eq!(resp.choices[0].index, 0);
    }
}
