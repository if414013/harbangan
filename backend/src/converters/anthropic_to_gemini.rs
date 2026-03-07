/// Convert Anthropic AnthropicMessagesRequest to a Gemini generateContent request body.
use crate::models::anthropic::AnthropicMessagesRequest;
use serde_json::{json, Value};

/// Convert an Anthropic-format request to a Gemini `generateContent` or
/// `streamGenerateContent` JSON body.
///
/// Mapping rules:
/// - `system` (string or block array) → `systemInstruction.parts`
/// - `assistant` role messages → `contents` with role `model`
/// - `user` role messages → `contents` with role `user`
/// - `max_tokens` → `generationConfig.maxOutputTokens` (if > 0)
/// - Content can be a string or an array of text blocks
pub fn anthropic_to_gemini(req: &AnthropicMessagesRequest) -> Value {
    let mut contents: Vec<Value> = Vec::new();

    for msg in &req.messages {
        let text = extract_text(&msg.content);
        let role = if msg.role == "assistant" {
            "model"
        } else {
            "user"
        };
        contents.push(json!({
            "role": role,
            "parts": [{ "text": text }]
        }));
    }

    let mut body = json!({ "contents": contents });

    if let Some(system) = &req.system {
        let sys_text = extract_text(system);
        if !sys_text.is_empty() {
            body["systemInstruction"] = json!({
                "parts": [{ "text": sys_text }]
            });
        }
    }

    if req.max_tokens > 0 {
        body["generationConfig"] = json!({ "maxOutputTokens": req.max_tokens });
    }

    body
}

fn extract_text(value: &Value) -> String {
    if let Some(s) = value.as_str() {
        s.to_string()
    } else if let Some(arr) = value.as_array() {
        arr.iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("")
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::anthropic::{AnthropicMessage, AnthropicMessagesRequest};
    use serde_json::json;

    fn make_req(messages: Vec<AnthropicMessage>) -> AnthropicMessagesRequest {
        AnthropicMessagesRequest {
            model: "claude-sonnet-4".to_string(),
            messages,
            max_tokens: 512,
            system: None,
            stream: false,
            tools: None,
            tool_choice: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            metadata: None,
        }
    }

    #[test]
    fn test_user_message_in_contents() {
        let req = make_req(vec![AnthropicMessage {
            role: "user".to_string(),
            content: json!("Hello"),
        }]);
        let body = anthropic_to_gemini(&req);
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "Hello");
    }

    #[test]
    fn test_assistant_becomes_model_role() {
        let req = make_req(vec![
            AnthropicMessage {
                role: "user".to_string(),
                content: json!("Hi"),
            },
            AnthropicMessage {
                role: "assistant".to_string(),
                content: json!("Hello!"),
            },
        ]);
        let body = anthropic_to_gemini(&req);
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents[1]["role"], "model");
    }

    #[test]
    fn test_system_string_to_system_instruction() {
        let mut req = make_req(vec![AnthropicMessage {
            role: "user".to_string(),
            content: json!("Hi"),
        }]);
        req.system = Some(json!("Be concise"));
        let body = anthropic_to_gemini(&req);
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "Be concise");
    }

    #[test]
    fn test_system_block_array_to_system_instruction() {
        let mut req = make_req(vec![AnthropicMessage {
            role: "user".to_string(),
            content: json!("Hi"),
        }]);
        req.system = Some(json!([
            {"type": "text", "text": "Part A"},
            {"type": "text", "text": "Part B"}
        ]));
        let body = anthropic_to_gemini(&req);
        assert_eq!(
            body["systemInstruction"]["parts"][0]["text"],
            "Part APart B"
        );
    }

    #[test]
    fn test_no_system_no_system_instruction() {
        let req = make_req(vec![AnthropicMessage {
            role: "user".to_string(),
            content: json!("Hi"),
        }]);
        let body = anthropic_to_gemini(&req);
        assert!(body.get("systemInstruction").is_none());
    }

    #[test]
    fn test_max_tokens_in_generation_config() {
        let req = make_req(vec![]);
        let body = anthropic_to_gemini(&req);
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 512);
    }

    #[test]
    fn test_zero_max_tokens_no_generation_config() {
        let mut req = make_req(vec![]);
        req.max_tokens = 0;
        let body = anthropic_to_gemini(&req);
        assert!(body.get("generationConfig").is_none());
    }

    #[test]
    fn test_content_as_block_array() {
        let req = make_req(vec![AnthropicMessage {
            role: "user".to_string(),
            content: json!([
                {"type": "text", "text": "Hello"},
                {"type": "text", "text": " world"}
            ]),
        }]);
        let body = anthropic_to_gemini(&req);
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents[0]["parts"][0]["text"], "Hello world");
    }
}
