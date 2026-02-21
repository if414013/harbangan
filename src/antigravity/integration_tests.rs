//! Integration tests for the antigravity backend.
//!
//! Tests cross-cutting scenarios: model routing, full conversion pipeline,
//! SSE parsing, and disabled-mode behavior.

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::antigravity::converters::anthropic_to_google::convert_anthropic_to_google;
    use crate::antigravity::converters::google_to_anthropic::{
        convert_finish_reason, convert_usage,
    };
    use crate::antigravity::converters::google_to_openai::build_choice;
    use crate::antigravity::request_builder::{build_cloud_code_request, build_headers};
    use crate::antigravity::router::{Backend, BackendRouter};
    use crate::antigravity::session::SessionManager;
    use crate::antigravity::streaming::{parse_collected_response, CloudCodeSseParser};
    use crate::config::AntigravityConfig;
    use crate::models::anthropic::{AnthropicMessage, AnthropicMessagesRequest};

    fn make_anthropic_request(model: &str) -> AnthropicMessagesRequest {
        AnthropicMessagesRequest {
            model: model.to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: json!("Hello"),
            }],
            max_tokens: 1024,
            system: None,
            stream: true,
            tools: None,
            tool_choice: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            metadata: None,
        }
    }

    fn make_router(enabled: bool) -> BackendRouter {
        let config = AntigravityConfig {
            enabled,
            refresh_token: None,
            project_id: None,
            endpoint: None,
        };
        BackendRouter::new(&config)
    }

    // === Model Routing ===

    #[test]
    fn test_routing_antigravity_models_when_enabled() {
        let router = make_router(true);
        assert!(matches!(
            router.resolve("claude-sonnet-4-5"),
            Backend::Antigravity
        ));
        assert!(matches!(
            router.resolve("gemini-3-flash"),
            Backend::Antigravity
        ));
        assert!(matches!(
            router.resolve("gemini-3-pro-low"),
            Backend::Antigravity
        ));
        assert!(matches!(
            router.resolve("gemini-3-pro-high"),
            Backend::Antigravity
        ));
    }

    #[test]
    fn test_routing_kiro_models_always_go_to_kiro() {
        let router = make_router(true);
        assert!(matches!(router.resolve("claude-3-5-sonnet"), Backend::Kiro));
        assert!(matches!(router.resolve("CLAUDE_SONNET_V2"), Backend::Kiro));
        assert!(matches!(router.resolve("unknown-model"), Backend::Kiro));
    }

    // === SSE Parsing ===

    #[test]
    fn test_sse_parse_text_block() {
        let raw = "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hello world\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":10,\"candidatesTokenCount\":5}}}\n\n";
        let mut parser = CloudCodeSseParser::new("claude-sonnet-4-5");
        let events = parser.process_line(raw.trim());
        // Should produce SSE events
        let finish_events = parser.finish();
        assert!(events.len() + finish_events.len() > 0);
    }

    #[test]
    fn test_streaming_parser_emits_events() {
        let sse_body = "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hi there\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":10,\"candidatesTokenCount\":3}}}\n\n";
        let result = parse_collected_response(sse_body, "claude-sonnet-4-5");
        assert!(result.is_object());
    }

    // === Google→Anthropic Conversion ===

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
        assert_eq!(convert_finish_reason(Some("STOP"), true), "tool_use");
    }

    #[test]
    fn test_usage_cache_token_adjustment() {
        let metadata = json!({
            "promptTokenCount": 100,
            "candidatesTokenCount": 50,
            "cachedContentTokenCount": 30
        });
        let usage = convert_usage(Some(&metadata));
        // input_tokens = promptTokenCount - cachedContentTokenCount = 70
        assert_eq!(usage.input_tokens, 70);
        assert_eq!(usage.output_tokens, 50);
    }

    // === Google→OpenAI Conversion ===

    #[test]
    fn test_openai_choice_text() {
        let parts = vec![json!({"text": "Hello"})];
        let choice = build_choice(&parts, Some("STOP"));
        assert_eq!(choice.finish_reason, Some("stop".to_string()));
        assert!(choice.message.content.is_some());
    }

    #[test]
    fn test_openai_choice_tool_call() {
        let parts = vec![json!({"functionCall": {"name": "get_weather", "args": {"city": "NYC"}}})];
        let choice = build_choice(&parts, Some("STOP"));
        assert_eq!(choice.finish_reason, Some("tool_calls".to_string()));
        assert!(choice.message.tool_calls.is_some());
    }

    // === Request Builder ===

    #[test]
    fn test_cloud_code_envelope_format() {
        let google_req = json!({"contents": []});
        let envelope =
            build_cloud_code_request("my-project", "claude-sonnet-4-5", google_req.clone());
        assert_eq!(envelope["project"], "my-project");
        assert_eq!(envelope["model"], "claude-sonnet-4-5");
        assert_eq!(envelope["userAgent"], "antigravity");
        assert_eq!(envelope["requestType"], "agent");
        assert!(envelope["requestId"]
            .as_str()
            .unwrap()
            .starts_with("agent-"));
        assert_eq!(envelope["request"], google_req);
    }

    #[test]
    fn test_headers_include_required_fields() {
        let headers = build_headers("my-token", "session-123", "claude-sonnet-4-5").unwrap();
        assert!(headers.contains_key("authorization"));
        assert!(headers.contains_key("x-client-name"));
        assert!(headers.contains_key("x-machine-session-id"));
    }

    #[test]
    fn test_thinking_header_for_thinking_model() {
        let headers = build_headers("token", "sess", "claude-sonnet-4-5-thinking").unwrap();
        assert!(headers.contains_key("anthropic-beta"));
    }

    #[test]
    fn test_no_thinking_header_for_non_thinking_model() {
        let headers = build_headers("token", "sess", "claude-sonnet-4-5").unwrap();
        assert!(!headers.contains_key("anthropic-beta"));
    }

    // === Session Manager ===

    #[test]
    fn test_session_id_stable_per_email() {
        let mut mgr = SessionManager::new();
        let id1 = mgr.get_or_create("user@example.com").to_string();
        let id2 = mgr.get_or_create("user@example.com").to_string();
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_session_id_different_per_email() {
        let mut mgr = SessionManager::new();
        let id1 = mgr.get_or_create("alice@example.com").to_string();
        let id2 = mgr.get_or_create("bob@example.com").to_string();
        assert_ne!(id1, id2);
    }

    // === Anthropic→Google Conversion ===

    #[test]
    fn test_convert_anthropic_to_google_basic() {
        let req = make_anthropic_request("claude-sonnet-4-5");
        let google = convert_anthropic_to_google(&req);
        assert!(google["contents"].is_array());
        assert!(google["generationConfig"].is_object());
    }

    #[test]
    fn test_convert_anthropic_system_prompt() {
        let mut req = make_anthropic_request("claude-sonnet-4-5");
        req.system = Some(json!("You are helpful."));
        let google = convert_anthropic_to_google(&req);
        assert!(google["systemInstruction"].is_object());
    }
}
