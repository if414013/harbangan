pub mod anthropic_to_gemini;
pub mod anthropic_to_kiro;
pub mod anthropic_to_openai;
pub mod core;
pub mod gemini_to_anthropic;
pub mod gemini_to_openai;
pub mod kiro_to_anthropic;
pub mod kiro_to_openai;
pub mod openai_to_anthropic;
pub mod openai_to_gemini;
pub mod openai_to_kiro;

/// Integration tests: converter round-trips across format boundaries.
///
/// These tests verify that composing two converters produces semantically
/// equivalent output — i.e. message content, roles, system prompts, and
/// sampling parameters survive a full format round-trip.
#[cfg(test)]
mod tests {
    use crate::converters::anthropic_to_openai::anthropic_to_openai;
    use crate::converters::openai_to_anthropic::openai_to_anthropic;
    use crate::models::anthropic::{AnthropicMessage, AnthropicMessagesRequest};
    use crate::models::openai::{ChatCompletionRequest, ChatMessage};
    use serde_json::json;

    // ── helpers ────────────────────────────────────────────────────────────

    fn openai_req(messages: Vec<ChatMessage>) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "claude-sonnet-4".to_string(),
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

    fn anthropic_req(messages: Vec<AnthropicMessage>) -> AnthropicMessagesRequest {
        AnthropicMessagesRequest {
            model: "claude-sonnet-4".to_string(),
            messages,
            max_tokens: 1024,
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

    fn chat_msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_string(),
            content: Some(json!(content)),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn anth_msg(role: &str, content: &str) -> AnthropicMessage {
        AnthropicMessage {
            role: role.to_string(),
            content: json!(content),
        }
    }

    // ── OpenAI → Anthropic → OpenAI ─────────────────────────────────────

    #[test]
    fn test_openai_anthropic_openai_roundtrip_basic_message() {
        let mut req = openai_req(vec![chat_msg("user", "Hello")]);
        req.max_tokens = Some(512);

        let mid = openai_to_anthropic(&req);
        let back = anthropic_to_openai(&mid);

        // Message content survives
        let user_msgs: Vec<_> = back.messages.iter().filter(|m| m.role == "user").collect();
        assert_eq!(user_msgs.len(), 1);
        assert_eq!(user_msgs[0].content, Some(json!("Hello")));
        // max_tokens survives
        assert_eq!(back.max_tokens, Some(512));
    }

    #[test]
    fn test_openai_anthropic_openai_roundtrip_system_message() {
        let req = openai_req(vec![
            chat_msg("system", "Be concise"),
            chat_msg("user", "What is 2+2?"),
        ]);

        let mid = openai_to_anthropic(&req);
        // System was extracted from messages
        assert_eq!(mid.system, Some(json!("Be concise")));
        assert_eq!(mid.messages.len(), 1);

        let back = anthropic_to_openai(&mid);
        // System is re-injected as a system-role message
        let sys_msgs: Vec<_> = back
            .messages
            .iter()
            .filter(|m| m.role == "system")
            .collect();
        assert_eq!(sys_msgs.len(), 1);
        assert_eq!(sys_msgs[0].content, Some(json!("Be concise")));
        // User message also survives
        let user_msgs: Vec<_> = back.messages.iter().filter(|m| m.role == "user").collect();
        assert_eq!(user_msgs.len(), 1);
        assert_eq!(user_msgs[0].content, Some(json!("What is 2+2?")));
    }

    #[test]
    fn test_openai_anthropic_openai_roundtrip_multiturn() {
        let req = openai_req(vec![
            chat_msg("user", "Ping"),
            chat_msg("assistant", "Pong"),
            chat_msg("user", "Again"),
        ]);

        let mid = openai_to_anthropic(&req);
        assert_eq!(mid.messages.len(), 3);

        let back = anthropic_to_openai(&mid);
        assert_eq!(back.messages.len(), 3);
        assert_eq!(back.messages[0].role, "user");
        assert_eq!(back.messages[0].content, Some(json!("Ping")));
        assert_eq!(back.messages[1].role, "assistant");
        assert_eq!(back.messages[1].content, Some(json!("Pong")));
        assert_eq!(back.messages[2].role, "user");
        assert_eq!(back.messages[2].content, Some(json!("Again")));
    }

    #[test]
    fn test_openai_anthropic_openai_roundtrip_temperature_top_p() {
        let mut req = openai_req(vec![chat_msg("user", "Hi")]);
        req.temperature = Some(0.7);
        req.top_p = Some(0.9);

        let mid = openai_to_anthropic(&req);
        let back = anthropic_to_openai(&mid);

        assert_eq!(back.temperature, Some(0.7));
        assert_eq!(back.top_p, Some(0.9));
    }

    #[test]
    fn test_openai_anthropic_openai_roundtrip_stop_sequences() {
        let mut req = openai_req(vec![chat_msg("user", "Go")]);
        req.stop = Some(json!(["END", "STOP"]));

        let mid = openai_to_anthropic(&req);
        assert_eq!(
            mid.stop_sequences,
            Some(vec!["END".to_string(), "STOP".to_string()])
        );

        let back = anthropic_to_openai(&mid);
        assert_eq!(back.stop, Some(json!(["END", "STOP"])));
    }

    // ── Anthropic → OpenAI → Anthropic ──────────────────────────────────

    #[test]
    fn test_anthropic_openai_anthropic_roundtrip_basic_message() {
        let mut req = anthropic_req(vec![anth_msg("user", "Hello")]);
        req.max_tokens = 256;

        let mid = anthropic_to_openai(&req);
        let back = openai_to_anthropic(&mid);

        let user_msgs: Vec<_> = back.messages.iter().filter(|m| m.role == "user").collect();
        assert_eq!(user_msgs.len(), 1);
        assert_eq!(user_msgs[0].content, json!("Hello"));
        assert_eq!(back.max_tokens, 256);
    }

    #[test]
    fn test_anthropic_openai_anthropic_roundtrip_system_prompt() {
        let mut req = anthropic_req(vec![anth_msg("user", "Hi")]);
        req.system = Some(json!("You are helpful"));

        let mid = anthropic_to_openai(&req);
        // System becomes a system-role message in OpenAI format
        assert!(mid.messages.iter().any(|m| m.role == "system"));

        let back = openai_to_anthropic(&mid);
        // System is re-extracted
        assert_eq!(back.system, Some(json!("You are helpful")));
        assert_eq!(back.messages.len(), 1);
        assert_eq!(back.messages[0].role, "user");
    }

    #[test]
    fn test_anthropic_openai_anthropic_roundtrip_multiturn() {
        let req = anthropic_req(vec![
            anth_msg("user", "First"),
            anth_msg("assistant", "Second"),
            anth_msg("user", "Third"),
        ]);

        let mid = anthropic_to_openai(&req);
        assert_eq!(mid.messages.len(), 3);

        let back = openai_to_anthropic(&mid);
        assert_eq!(back.messages.len(), 3);
        assert_eq!(back.messages[0].content, json!("First"));
        assert_eq!(back.messages[1].content, json!("Second"));
        assert_eq!(back.messages[2].content, json!("Third"));
    }

    #[test]
    fn test_anthropic_openai_anthropic_roundtrip_temperature() {
        let mut req = anthropic_req(vec![anth_msg("user", "Hi")]);
        req.temperature = Some(0.4);
        req.top_p = Some(0.85);

        let mid = anthropic_to_openai(&req);
        let back = openai_to_anthropic(&mid);

        assert_eq!(back.temperature, Some(0.4));
        assert_eq!(back.top_p, Some(0.85));
    }

    #[test]
    fn test_anthropic_openai_anthropic_roundtrip_stop_sequences() {
        let mut req = anthropic_req(vec![anth_msg("user", "Go")]);
        req.stop_sequences = Some(vec!["HALT".to_string(), "DONE".to_string()]);

        let mid = anthropic_to_openai(&req);
        assert_eq!(mid.stop, Some(json!(["HALT", "DONE"])));

        let back = openai_to_anthropic(&mid);
        assert_eq!(
            back.stop_sequences,
            Some(vec!["HALT".to_string(), "DONE".to_string()])
        );
    }

    // ── model name survives both directions ─────────────────────────────

    #[test]
    fn test_model_name_preserved_openai_anthropic_openai() {
        let mut req = openai_req(vec![chat_msg("user", "Hi")]);
        req.model = "gpt-4o".to_string();

        let mid = openai_to_anthropic(&req);
        assert_eq!(mid.model, "gpt-4o");

        let back = anthropic_to_openai(&mid);
        assert_eq!(back.model, "gpt-4o");
    }

    #[test]
    fn test_model_name_preserved_anthropic_openai_anthropic() {
        let mut req = anthropic_req(vec![anth_msg("user", "Hi")]);
        req.model = "claude-3-5-sonnet-20241022".to_string();

        let mid = anthropic_to_openai(&req);
        assert_eq!(mid.model, "claude-3-5-sonnet-20241022");

        let back = openai_to_anthropic(&mid);
        assert_eq!(back.model, "claude-3-5-sonnet-20241022");
    }
}
