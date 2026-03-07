/// Convert Anthropic AnthropicMessagesRequest to OpenAI ChatCompletionRequest.
use crate::models::anthropic::AnthropicMessagesRequest;
use crate::models::openai::{ChatCompletionRequest, ChatMessage};

/// Convert an Anthropic-format request to OpenAI format.
///
/// Mapping rules:
/// - `system` (string or block array) → prepended as a `system` role ChatMessage
/// - `user`/`assistant` messages are passed through with their content
/// - `max_tokens` → `max_tokens`
/// - `temperature`, `top_p` are passed through when present
/// - `stop_sequences` → `stop` (as a JSON array)
pub fn anthropic_to_openai(req: &AnthropicMessagesRequest) -> ChatCompletionRequest {
    let mut messages: Vec<ChatMessage> = Vec::new();

    // Prepend system message if present
    if let Some(system) = &req.system {
        let text = system
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| {
                system.as_array().map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
            })
            .unwrap_or_default();

        if !text.is_empty() {
            messages.push(ChatMessage {
                role: "system".to_string(),
                content: Some(serde_json::Value::String(text)),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            });
        }
    }

    for msg in &req.messages {
        messages.push(ChatMessage {
            role: msg.role.clone(),
            content: Some(msg.content.clone()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    let stop = req.stop_sequences.as_ref().map(|seqs| {
        serde_json::Value::Array(
            seqs.iter()
                .map(|s| serde_json::Value::String(s.clone()))
                .collect(),
        )
    });

    ChatCompletionRequest {
        model: req.model.clone(),
        messages,
        stream: req.stream,
        temperature: req.temperature,
        top_p: req.top_p,
        n: None,
        max_tokens: Some(req.max_tokens),
        max_completion_tokens: None,
        stop,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::anthropic::{AnthropicMessage, AnthropicMessagesRequest};
    use serde_json::json;

    fn make_req(messages: Vec<AnthropicMessage>) -> AnthropicMessagesRequest {
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

    #[test]
    fn test_basic_user_message() {
        let req = make_req(vec![AnthropicMessage {
            role: "user".to_string(),
            content: json!("Hello"),
        }]);
        let out = anthropic_to_openai(&req);
        assert_eq!(out.messages.len(), 1);
        assert_eq!(out.messages[0].role, "user");
        assert_eq!(out.messages[0].content, Some(json!("Hello")));
    }

    #[test]
    fn test_system_prepended_as_message() {
        let mut req = make_req(vec![AnthropicMessage {
            role: "user".to_string(),
            content: json!("Hi"),
        }]);
        req.system = Some(json!("Be concise"));
        let out = anthropic_to_openai(&req);
        assert_eq!(out.messages.len(), 2);
        assert_eq!(out.messages[0].role, "system");
        assert_eq!(out.messages[0].content, Some(json!("Be concise")));
        assert_eq!(out.messages[1].role, "user");
    }

    #[test]
    fn test_system_block_array_joined() {
        let mut req = make_req(vec![AnthropicMessage {
            role: "user".to_string(),
            content: json!("Hi"),
        }]);
        req.system = Some(json!([
            {"type": "text", "text": "Part 1"},
            {"type": "text", "text": "Part 2"}
        ]));
        let out = anthropic_to_openai(&req);
        assert_eq!(out.messages[0].role, "system");
        assert_eq!(out.messages[0].content, Some(json!("Part 1\nPart 2")));
    }

    #[test]
    fn test_no_system_no_prepend() {
        let req = make_req(vec![AnthropicMessage {
            role: "user".to_string(),
            content: json!("Hi"),
        }]);
        let out = anthropic_to_openai(&req);
        assert_eq!(out.messages.len(), 1);
        assert_eq!(out.messages[0].role, "user");
    }

    #[test]
    fn test_assistant_message_preserved() {
        let req = make_req(vec![
            AnthropicMessage {
                role: "user".to_string(),
                content: json!("Ping"),
            },
            AnthropicMessage {
                role: "assistant".to_string(),
                content: json!("Pong"),
            },
        ]);
        let out = anthropic_to_openai(&req);
        assert_eq!(out.messages.len(), 2);
        assert_eq!(out.messages[1].role, "assistant");
    }

    #[test]
    fn test_max_tokens_forwarded() {
        let req = make_req(vec![]);
        let out = anthropic_to_openai(&req);
        assert_eq!(out.max_tokens, Some(1024));
    }

    #[test]
    fn test_temperature_and_top_p_forwarded() {
        let mut req = make_req(vec![]);
        req.temperature = Some(0.5);
        req.top_p = Some(0.8);
        let out = anthropic_to_openai(&req);
        assert_eq!(out.temperature, Some(0.5));
        assert_eq!(out.top_p, Some(0.8));
    }

    #[test]
    fn test_stop_sequences_become_stop_array() {
        let mut req = make_req(vec![]);
        req.stop_sequences = Some(vec!["END".to_string(), "DONE".to_string()]);
        let out = anthropic_to_openai(&req);
        assert_eq!(out.stop, Some(json!(["END", "DONE"])));
    }

    #[test]
    fn test_no_stop_sequences_is_none() {
        let req = make_req(vec![]);
        let out = anthropic_to_openai(&req);
        assert!(out.stop.is_none());
    }

    #[test]
    fn test_stream_forwarded() {
        let mut req = make_req(vec![]);
        req.stream = true;
        let out = anthropic_to_openai(&req);
        assert!(out.stream);
    }

    #[test]
    fn test_model_forwarded() {
        let req = make_req(vec![]);
        let out = anthropic_to_openai(&req);
        assert_eq!(out.model, "claude-sonnet-4");
    }
}
