/// Convert OpenAI ChatCompletionRequest to a Gemini generateContent request body.
use crate::models::openai::ChatCompletionRequest;
use serde_json::{json, Value};

/// Convert an OpenAI-format request to a Gemini `generateContent` or
/// `streamGenerateContent` JSON body.
///
/// Mapping rules:
/// - `system` role messages → `systemInstruction.parts`
/// - `assistant` role messages → `contents` with role `model`
/// - `user` role messages → `contents` with role `user`
/// - `max_tokens` → `generationConfig.maxOutputTokens`
/// - `temperature` → `generationConfig.temperature`
pub fn openai_to_gemini(req: &ChatCompletionRequest) -> Value {
    let mut system_instruction: Option<String> = None;
    let mut contents: Vec<Value> = Vec::new();

    for msg in &req.messages {
        let text = msg
            .content
            .as_ref()
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        match msg.role.as_str() {
            "system" => {
                system_instruction = Some(text);
            }
            "assistant" => {
                contents.push(json!({
                    "role": "model",
                    "parts": [{ "text": text }]
                }));
            }
            _ => {
                contents.push(json!({
                    "role": "user",
                    "parts": [{ "text": text }]
                }));
            }
        }
    }

    let mut body = json!({ "contents": contents });

    if let Some(sys) = system_instruction {
        body["systemInstruction"] = json!({
            "parts": [{ "text": sys }]
        });
    }

    let mut gen_config = json!({});
    if let Some(max_tokens) = req.max_tokens.or(req.max_completion_tokens) {
        gen_config["maxOutputTokens"] = json!(max_tokens);
    }
    if let Some(temp) = req.temperature {
        gen_config["temperature"] = json!(temp);
    }
    if gen_config
        .as_object()
        .map(|m| !m.is_empty())
        .unwrap_or(false)
    {
        body["generationConfig"] = gen_config;
    }

    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::openai::{ChatCompletionRequest, ChatMessage};
    use serde_json::json;

    fn make_req(messages: Vec<ChatMessage>) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "gemini-2.5-pro".to_string(),
            messages,
            stream: false,
            temperature: None,
            top_p: None,
            n: None,
            max_tokens: None,
            max_completion_tokens: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
            tools: None,
            tool_choice: None,
            stream_options: None,
            logit_bias: None,
            logprobs: None,
            top_logprobs: None,
            user: None,
            seed: None,
            parallel_tool_calls: None,
        }
    }

    #[test]
    fn test_user_message_in_contents() {
        let req = make_req(vec![ChatMessage {
            role: "user".to_string(),
            content: Some(json!("Hello")),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }]);
        let body = openai_to_gemini(&req);
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "Hello");
    }

    #[test]
    fn test_assistant_becomes_model_role() {
        let req = make_req(vec![
            ChatMessage {
                role: "user".to_string(),
                content: Some(json!("Hi")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: Some(json!("Hello!")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
        ]);
        let body = openai_to_gemini(&req);
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents[1]["role"], "model");
        assert_eq!(contents[1]["parts"][0]["text"], "Hello!");
    }

    #[test]
    fn test_system_extracted_to_system_instruction() {
        let req = make_req(vec![
            ChatMessage {
                role: "system".to_string(),
                content: Some(json!("Be concise")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: Some(json!("Hi")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
        ]);
        let body = openai_to_gemini(&req);
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "Be concise");
        // System should NOT be in contents
        assert_eq!(body["contents"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_no_system_no_system_instruction_field() {
        let req = make_req(vec![ChatMessage {
            role: "user".to_string(),
            content: Some(json!("Hi")),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }]);
        let body = openai_to_gemini(&req);
        assert!(body.get("systemInstruction").is_none());
    }

    #[test]
    fn test_max_tokens_in_generation_config() {
        let mut req = make_req(vec![]);
        req.max_tokens = Some(500);
        let body = openai_to_gemini(&req);
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 500);
    }

    #[test]
    fn test_max_completion_tokens_fallback() {
        let mut req = make_req(vec![]);
        req.max_completion_tokens = Some(300);
        let body = openai_to_gemini(&req);
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 300);
    }

    #[test]
    fn test_temperature_in_generation_config() {
        let mut req = make_req(vec![]);
        req.temperature = Some(0.7);
        let body = openai_to_gemini(&req);
        assert!((body["generationConfig"]["temperature"].as_f64().unwrap() - 0.7).abs() < 1e-6);
    }

    #[test]
    fn test_no_tokens_no_generation_config() {
        let req = make_req(vec![]);
        let body = openai_to_gemini(&req);
        assert!(body.get("generationConfig").is_none());
    }

    #[test]
    fn test_multiple_messages_order_preserved() {
        let req = make_req(vec![
            ChatMessage {
                role: "user".to_string(),
                content: Some(json!("First")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: Some(json!("Second")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: Some(json!("Third")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
        ]);
        let body = openai_to_gemini(&req);
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[0]["parts"][0]["text"], "First");
        assert_eq!(contents[1]["parts"][0]["text"], "Second");
        assert_eq!(contents[2]["parts"][0]["text"], "Third");
    }
}
