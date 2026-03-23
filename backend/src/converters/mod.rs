pub mod anthropic_to_kiro;
pub mod anthropic_to_openai;
pub mod core;
pub mod kiro_to_anthropic;
pub mod kiro_to_openai;
pub mod openai_to_anthropic;
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
    use crate::models::anthropic::{
        AnthropicCustomTool, AnthropicMessage, AnthropicMessagesRequest, AnthropicTool,
    };
    use crate::models::openai::{
        ChatCompletionRequest, ChatMessage, FunctionCall, FunctionTool, Tool, ToolCall,
        ToolFunction,
    };
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
            reasoning_effort: None,
            response_format: None,
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
            thinking: None,
            disable_parallel_tool_use: None,
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

    // ── Tool round-trip tests ────────────────────────────────────────────

    fn make_openai_tool(name: &str, desc: &str) -> Tool {
        Tool::Function(FunctionTool {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: name.to_string(),
                description: Some(desc.to_string()),
                parameters: Some(
                    json!({"type": "object", "properties": {"q": {"type": "string"}}}),
                ),
            },
        })
    }

    fn make_anthropic_tool(name: &str, desc: &str) -> AnthropicTool {
        AnthropicTool::Custom(AnthropicCustomTool {
            name: name.to_string(),
            description: Some(desc.to_string()),
            input_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        })
    }

    #[test]
    fn test_tool_roundtrip_openai_tools_survive_anthropic_and_back() {
        let mut req = openai_req(vec![chat_msg("user", "Search for cats")]);
        req.tools = Some(vec![make_openai_tool("search", "Search the web")]);

        let mid = openai_to_anthropic(&req);
        assert!(mid.tools.is_some());
        let tools = mid.tools.as_ref().unwrap();
        assert_eq!(tools.len(), 1);
        if let AnthropicTool::Custom(ct) = &tools[0] {
            assert_eq!(ct.name, "search");
            assert_eq!(ct.description, Some("Search the web".to_string()));
        } else {
            panic!("Expected Custom tool");
        }

        let back = anthropic_to_openai(&mid);
        assert!(back.tools.is_some());
        let tools = back.tools.as_ref().unwrap();
        assert_eq!(tools.len(), 1);
        if let Tool::Function(ft) = &tools[0] {
            assert_eq!(ft.function.name, "search");
            assert_eq!(ft.function.description, Some("Search the web".to_string()));
        } else {
            panic!("Expected Function tool");
        }
    }

    #[test]
    fn test_tool_roundtrip_anthropic_tools_survive_openai_and_back() {
        let mut req = anthropic_req(vec![anth_msg("user", "Get weather")]);
        req.tools = Some(vec![make_anthropic_tool(
            "get_weather",
            "Get current weather",
        )]);

        let mid = anthropic_to_openai(&req);
        assert!(mid.tools.is_some());
        let tools = mid.tools.as_ref().unwrap();
        assert_eq!(tools.len(), 1);
        if let Tool::Function(ft) = &tools[0] {
            assert_eq!(ft.function.name, "get_weather");
        } else {
            panic!("Expected Function tool");
        }

        let back = openai_to_anthropic(&mid);
        assert!(back.tools.is_some());
        let tools = back.tools.as_ref().unwrap();
        assert_eq!(tools.len(), 1);
        if let AnthropicTool::Custom(ct) = &tools[0] {
            assert_eq!(ct.name, "get_weather");
        } else {
            panic!("Expected Custom tool");
        }
    }

    #[test]
    fn test_tool_roundtrip_openai_tool_calls_to_anthropic_tool_use() {
        let mut req = openai_req(vec![
            chat_msg("user", "Search for cats"),
            ChatMessage {
                role: "assistant".to_string(),
                content: None,
                name: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_123".to_string(),
                    tool_type: "function".to_string(),
                    function: FunctionCall {
                        name: "search".to_string(),
                        arguments: r#"{"q":"cats"}"#.to_string(),
                    },
                }]),
                tool_call_id: None,
            },
        ]);
        req.tools = Some(vec![make_openai_tool("search", "Search")]);

        let mid = openai_to_anthropic(&req);
        // Assistant message should have tool_use content block
        let asst = &mid.messages[1];
        assert_eq!(asst.role, "assistant");
        let blocks = asst.content.as_array().expect("expected array content");
        let tool_use = blocks.iter().find(|b| b["type"] == "tool_use").unwrap();
        assert_eq!(tool_use["id"], "call_123");
        assert_eq!(tool_use["name"], "search");
        assert_eq!(tool_use["input"]["q"], "cats");

        let back = anthropic_to_openai(&mid);
        let asst_back = back
            .messages
            .iter()
            .find(|m| m.role == "assistant")
            .unwrap();
        let tc = asst_back.tool_calls.as_ref().expect("expected tool_calls");
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].id, "call_123");
        assert_eq!(tc[0].function.name, "search");
    }

    #[test]
    fn test_tool_roundtrip_anthropic_tool_use_to_openai_tool_calls() {
        let req = anthropic_req(vec![
            anth_msg("user", "Get weather"),
            AnthropicMessage {
                role: "assistant".to_string(),
                content: json!([
                    {"type": "tool_use", "id": "tu_456", "name": "get_weather", "input": {"city": "NYC"}}
                ]),
            },
        ]);

        let mid = anthropic_to_openai(&req);
        let asst = mid.messages.iter().find(|m| m.role == "assistant").unwrap();
        let tc = asst.tool_calls.as_ref().expect("expected tool_calls");
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].id, "tu_456");
        assert_eq!(tc[0].function.name, "get_weather");

        let back = openai_to_anthropic(&mid);
        let asst_back = &back
            .messages
            .iter()
            .find(|m| m.role == "assistant")
            .unwrap();
        let blocks = asst_back.content.as_array().expect("expected array");
        let tu = blocks.iter().find(|b| b["type"] == "tool_use").unwrap();
        assert_eq!(tu["id"], "tu_456");
        assert_eq!(tu["name"], "get_weather");
    }

    #[test]
    fn test_tool_roundtrip_openai_tool_result_to_anthropic_tool_result() {
        let req = openai_req(vec![
            chat_msg("user", "Search"),
            ChatMessage {
                role: "assistant".to_string(),
                content: None,
                name: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_789".to_string(),
                    tool_type: "function".to_string(),
                    function: FunctionCall {
                        name: "search".to_string(),
                        arguments: r#"{"q":"dogs"}"#.to_string(),
                    },
                }]),
                tool_call_id: None,
            },
            ChatMessage {
                role: "tool".to_string(),
                content: Some(json!("Found 10 results")),
                name: None,
                tool_calls: None,
                tool_call_id: Some("call_789".to_string()),
            },
        ]);

        let mid = openai_to_anthropic(&req);
        // Tool result should be in a user message as tool_result block
        let user_with_result = mid.messages.iter().find(|m| {
            m.role == "user"
                && m.content
                    .as_array()
                    .is_some_and(|a| a.iter().any(|b| b["type"] == "tool_result"))
        });
        assert!(user_with_result.is_some());
        let blocks = user_with_result.unwrap().content.as_array().unwrap();
        let tr = blocks.iter().find(|b| b["type"] == "tool_result").unwrap();
        assert_eq!(tr["tool_use_id"], "call_789");
        assert_eq!(tr["content"], "Found 10 results");

        let back = anthropic_to_openai(&mid);
        let tool_msg = back.messages.iter().find(|m| m.role == "tool").unwrap();
        assert_eq!(tool_msg.tool_call_id, Some("call_789".to_string()));
        assert_eq!(tool_msg.content, Some(json!("Found 10 results")));
    }

    #[test]
    fn test_tool_roundtrip_anthropic_tool_result_to_openai_tool_role() {
        let req = anthropic_req(vec![
            anth_msg("user", "Weather?"),
            AnthropicMessage {
                role: "assistant".to_string(),
                content: json!([
                    {"type": "tool_use", "id": "tu_abc", "name": "weather", "input": {}}
                ]),
            },
            AnthropicMessage {
                role: "user".to_string(),
                content: json!([
                    {"type": "tool_result", "tool_use_id": "tu_abc", "content": "Sunny 72F"}
                ]),
            },
        ]);

        let mid = anthropic_to_openai(&req);
        let tool_msg = mid.messages.iter().find(|m| m.role == "tool").unwrap();
        assert_eq!(tool_msg.tool_call_id, Some("tu_abc".to_string()));
        assert_eq!(tool_msg.content, Some(json!("Sunny 72F")));

        let back = openai_to_anthropic(&mid);
        // Should have tool_result block in a user message
        let user_tr = back.messages.iter().find(|m| {
            m.role == "user"
                && m.content
                    .as_array()
                    .is_some_and(|a| a.iter().any(|b| b["type"] == "tool_result"))
        });
        assert!(user_tr.is_some());
    }

    #[test]
    fn test_tool_roundtrip_preserves_structured_tool_result_content() {
        let req = anthropic_req(vec![
            anth_msg("user", "Weather?"),
            AnthropicMessage {
                role: "assistant".to_string(),
                content: json!([
                    {"type": "tool_use", "id": "tu_blocks", "name": "weather", "input": {}}
                ]),
            },
            AnthropicMessage {
                role: "user".to_string(),
                content: json!([
                    {
                        "type": "tool_result",
                        "tool_use_id": "tu_blocks",
                        "content": [{"type": "text", "text": "Sunny 72F"}]
                    }
                ]),
            },
        ]);

        let mid = anthropic_to_openai(&req);
        let tool_msg = mid.messages.iter().find(|m| m.role == "tool").unwrap();
        assert_eq!(tool_msg.tool_call_id, Some("tu_blocks".to_string()));
        assert_eq!(
            tool_msg.content,
            Some(json!([{"type": "text", "text": "Sunny 72F"}]))
        );

        let back = openai_to_anthropic(&mid);
        let user_tr = back.messages.iter().find(|m| {
            m.role == "user"
                && m.content
                    .as_array()
                    .is_some_and(|a| a.iter().any(|b| b["type"] == "tool_result"))
        });
        assert!(user_tr.is_some());
        let blocks = user_tr.unwrap().content.as_array().unwrap();
        let tr = blocks.iter().find(|b| b["type"] == "tool_result").unwrap();
        assert_eq!(tr["tool_use_id"], "tu_blocks");
        assert_eq!(
            tr["content"],
            json!([{"type": "text", "text": "Sunny 72F"}])
        );
    }

    #[test]
    fn test_tool_roundtrip_multiple_concurrent_tool_calls() {
        let req = openai_req(vec![
            chat_msg("user", "Search and translate"),
            ChatMessage {
                role: "assistant".to_string(),
                content: None,
                name: None,
                tool_calls: Some(vec![
                    ToolCall {
                        id: "call_a".to_string(),
                        tool_type: "function".to_string(),
                        function: FunctionCall {
                            name: "search".to_string(),
                            arguments: r#"{"q":"hello"}"#.to_string(),
                        },
                    },
                    ToolCall {
                        id: "call_b".to_string(),
                        tool_type: "function".to_string(),
                        function: FunctionCall {
                            name: "translate".to_string(),
                            arguments: r#"{"text":"hi","to":"es"}"#.to_string(),
                        },
                    },
                ]),
                tool_call_id: None,
            },
        ]);

        let mid = openai_to_anthropic(&req);
        let asst = mid.messages.iter().find(|m| m.role == "assistant").unwrap();
        let blocks = asst.content.as_array().expect("expected array");
        let tool_uses: Vec<_> = blocks.iter().filter(|b| b["type"] == "tool_use").collect();
        assert_eq!(tool_uses.len(), 2);
        assert_eq!(tool_uses[0]["name"], "search");
        assert_eq!(tool_uses[1]["name"], "translate");

        let back = anthropic_to_openai(&mid);
        let asst_back = back
            .messages
            .iter()
            .find(|m| m.role == "assistant")
            .unwrap();
        let tc = asst_back.tool_calls.as_ref().unwrap();
        assert_eq!(tc.len(), 2);
        assert_eq!(tc[0].function.name, "search");
        assert_eq!(tc[1].function.name, "translate");
    }

    #[test]
    fn test_tool_roundtrip_mixed_text_and_tool_calls() {
        let req = openai_req(vec![
            chat_msg("user", "Help me search"),
            ChatMessage {
                role: "assistant".to_string(),
                content: Some(json!("Let me search for that.")),
                name: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_mixed".to_string(),
                    tool_type: "function".to_string(),
                    function: FunctionCall {
                        name: "search".to_string(),
                        arguments: r#"{"q":"test"}"#.to_string(),
                    },
                }]),
                tool_call_id: None,
            },
        ]);

        let mid = openai_to_anthropic(&req);
        let asst = mid.messages.iter().find(|m| m.role == "assistant").unwrap();
        let blocks = asst.content.as_array().expect("expected array content");
        // Should have both text and tool_use blocks
        let has_text = blocks.iter().any(|b| b["type"] == "text");
        let has_tool_use = blocks.iter().any(|b| b["type"] == "tool_use");
        assert!(has_text, "expected text block");
        assert!(has_tool_use, "expected tool_use block");
        let text_block = blocks.iter().find(|b| b["type"] == "text").unwrap();
        assert_eq!(text_block["text"], "Let me search for that.");

        let back = anthropic_to_openai(&mid);
        let asst_back = back
            .messages
            .iter()
            .find(|m| m.role == "assistant")
            .unwrap();
        // Should have both content text and tool_calls
        assert!(asst_back.tool_calls.is_some());
        assert!(asst_back.content.is_some());
        let content = asst_back.content.as_ref().unwrap();
        assert_eq!(content.as_str().unwrap(), "Let me search for that.");
    }
}
