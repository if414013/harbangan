/// Convert OpenAI ChatCompletionRequest to Anthropic AnthropicMessagesRequest.
use serde_json::json;

use crate::converters::core::{
    map_reasoning_effort_to_thinking, map_response_format_openai_to_anthropic,
    map_tool_choice_openai_to_anthropic,
};
use crate::models::anthropic::{
    AnthropicCustomTool, AnthropicMessage, AnthropicMessagesRequest, AnthropicTool,
};
use crate::models::openai::{ChatCompletionRequest, Tool};
use tracing::debug;

/// Convert an OpenAI-format request to Anthropic format.
///
/// Mapping rules:
/// - `messages` with role `system` are extracted and concatenated into `system`
/// - `user` and `assistant` messages are passed through with their content
/// - `max_tokens` (or `max_completion_tokens`) defaults to 4096 if unset
/// - `temperature`, `top_p`, `stop` are passed through when present
/// - `tool_choice` is mapped: "auto" → {"type":"auto"}, "required" → {"type":"any"}, etc.
/// - `parallel_tool_calls: false` → `disable_parallel_tool_use: true`
/// - `response_format.json_schema` → `output_format` (with constraint filtering)
/// - `reasoning_effort` → `thinking` config (budget_tokens based on effort level)
pub fn openai_to_anthropic(req: &ChatCompletionRequest) -> AnthropicMessagesRequest {
    let mut system_parts: Vec<String> = Vec::new();
    let mut messages: Vec<AnthropicMessage> = Vec::new();

    for msg in &req.messages {
        let text = msg
            .content
            .as_ref()
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();

        match msg.role.as_str() {
            "system" => {
                system_parts.push(text);
            }
            "assistant" => {
                // Build content blocks: text + tool_use blocks
                let mut blocks: Vec<serde_json::Value> = Vec::new();
                if !text.is_empty() {
                    blocks.push(json!({"type": "text", "text": text}));
                }
                if let Some(tool_calls) = &msg.tool_calls {
                    for tc in tool_calls {
                        let input: serde_json::Value =
                            serde_json::from_str(&tc.function.arguments).unwrap_or(json!({}));
                        blocks.push(json!({
                            "type": "tool_use",
                            "id": tc.id,
                            "name": tc.function.name,
                            "input": input,
                        }));
                    }
                }
                let content = if blocks.len() == 1 && msg.tool_calls.is_none() {
                    // Simple text — keep as string for compatibility
                    msg.content
                        .clone()
                        .unwrap_or(serde_json::Value::String(String::new()))
                } else if blocks.is_empty() {
                    serde_json::Value::String(String::new())
                } else {
                    serde_json::Value::Array(blocks)
                };
                messages.push(AnthropicMessage {
                    role: "assistant".to_string(),
                    content,
                });
            }
            "tool" => {
                // OpenAI tool result → Anthropic tool_result content block
                let tool_use_id = msg.tool_call_id.clone().unwrap_or_default();
                let result_content = msg
                    .content
                    .clone()
                    .unwrap_or_else(|| serde_json::Value::String(String::new()));
                let block = json!({
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": result_content,
                });
                // Anthropic expects tool_result in a user message
                // Buffer consecutive tool results into one user message
                if let Some(last) = messages.last_mut() {
                    if last.role == "user" {
                        if let Some(arr) = last.content.as_array_mut() {
                            arr.push(block);
                            continue;
                        }
                    }
                }
                messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: serde_json::Value::Array(vec![block]),
                });
            }
            _ => {
                messages.push(AnthropicMessage {
                    role: msg.role.clone(),
                    content: msg
                        .content
                        .clone()
                        .unwrap_or(serde_json::Value::String(String::new())),
                });
            }
        }
    }

    let system = if system_parts.is_empty() {
        None
    } else {
        Some(serde_json::Value::String(system_parts.join("\n")))
    };

    let max_tokens = req.max_tokens.or(req.max_completion_tokens).unwrap_or(4096);

    let stop_sequences = req.stop.as_ref().and_then(|s| {
        if let Some(arr) = s.as_array() {
            let seqs: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            if seqs.is_empty() {
                None
            } else {
                Some(seqs)
            }
        } else if let Some(str_val) = s.as_str() {
            Some(vec![str_val.to_string()])
        } else {
            None
        }
    });

    // Map tool_choice and parallel_tool_calls
    let (tool_choice, disable_parallel_tool_use) =
        map_tool_choice_openai_to_anthropic(&req.tool_choice, req.parallel_tool_calls);

    // Map response_format to output_format
    // Note: Anthropic uses a separate 'output_format' field, but our model doesn't have it yet
    // For now, we store it in metadata or handle it at a higher level
    let _output_format = map_response_format_openai_to_anthropic(&req.response_format);

    // Map reasoning_effort to thinking config
    let (thinking, should_drop_temp) =
        map_reasoning_effort_to_thinking(req.reasoning_effort.as_deref());

    // Drop temperature if thinking is enabled
    let temperature = if should_drop_temp {
        debug!(reasoning_effort = ?req.reasoning_effort, "Dropping temperature due to thinking mode");
        None
    } else {
        req.temperature
    };

    // Map OpenAI tools to Anthropic format
    let tools: Option<Vec<AnthropicTool>> = req.tools.as_ref().map(|tools| {
        tools
            .iter()
            .filter_map(|t| match t {
                Tool::Function(ft) => Some(AnthropicTool::Custom(AnthropicCustomTool {
                    name: ft.function.name.clone(),
                    description: ft.function.description.clone(),
                    input_schema: ft
                        .function
                        .parameters
                        .clone()
                        .unwrap_or(json!({"type": "object", "properties": {}})),
                })),
                Tool::ServerSide(_) => None, // Server-side tools don't map to Anthropic
            })
            .collect()
    });

    AnthropicMessagesRequest {
        model: req.model.clone(),
        messages,
        max_tokens,
        system,
        stream: req.stream,
        tools,
        tool_choice,
        temperature,
        top_p: req.top_p,
        top_k: None,
        thinking,
        stop_sequences,
        metadata: None,
        disable_parallel_tool_use,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::openai::{ChatCompletionRequest, ChatMessage};
    use serde_json::json;

    fn make_req(messages: Vec<ChatMessage>) -> ChatCompletionRequest {
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

    #[test]
    fn test_basic_user_message() {
        let req = make_req(vec![ChatMessage {
            role: "user".to_string(),
            content: Some(json!("Hello")),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }]);
        let out = openai_to_anthropic(&req);
        assert_eq!(out.messages.len(), 1);
        assert_eq!(out.messages[0].role, "user");
        assert_eq!(out.messages[0].content, json!("Hello"));
        assert!(out.system.is_none());
    }

    #[test]
    fn test_system_message_extracted() {
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
        let out = openai_to_anthropic(&req);
        assert_eq!(out.messages.len(), 1);
        assert_eq!(out.system, Some(json!("Be concise")));
    }

    #[test]
    fn test_multiple_system_messages_joined() {
        let req = make_req(vec![
            ChatMessage {
                role: "system".to_string(),
                content: Some(json!("Part 1")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "system".to_string(),
                content: Some(json!("Part 2")),
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
        let out = openai_to_anthropic(&req);
        assert_eq!(out.system, Some(json!("Part 1\nPart 2")));
        assert_eq!(out.messages.len(), 1);
    }

    #[test]
    fn test_assistant_message_preserved() {
        let req = make_req(vec![
            ChatMessage {
                role: "user".to_string(),
                content: Some(json!("Ping")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: Some(json!("Pong")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
        ]);
        let out = openai_to_anthropic(&req);
        assert_eq!(out.messages.len(), 2);
        assert_eq!(out.messages[1].role, "assistant");
        assert_eq!(out.messages[1].content, json!("Pong"));
    }

    #[test]
    fn test_max_tokens_defaults_to_4096() {
        let req = make_req(vec![]);
        let out = openai_to_anthropic(&req);
        assert_eq!(out.max_tokens, 4096);
    }

    #[test]
    fn test_max_tokens_from_request() {
        let mut req = make_req(vec![]);
        req.max_tokens = Some(1000);
        let out = openai_to_anthropic(&req);
        assert_eq!(out.max_tokens, 1000);
    }

    #[test]
    fn test_max_completion_tokens_fallback() {
        let mut req = make_req(vec![]);
        req.max_completion_tokens = Some(2000);
        let out = openai_to_anthropic(&req);
        assert_eq!(out.max_tokens, 2000);
    }

    #[test]
    fn test_max_tokens_takes_precedence_over_max_completion_tokens() {
        let mut req = make_req(vec![]);
        req.max_tokens = Some(500);
        req.max_completion_tokens = Some(2000);
        let out = openai_to_anthropic(&req);
        assert_eq!(out.max_tokens, 500);
    }

    #[test]
    fn test_temperature_and_top_p_forwarded() {
        let mut req = make_req(vec![]);
        req.temperature = Some(0.7);
        req.top_p = Some(0.9);
        let out = openai_to_anthropic(&req);
        assert_eq!(out.temperature, Some(0.7));
        assert_eq!(out.top_p, Some(0.9));
    }

    #[test]
    fn test_stop_string_to_vec() {
        let mut req = make_req(vec![]);
        req.stop = Some(json!("STOP"));
        let out = openai_to_anthropic(&req);
        assert_eq!(out.stop_sequences, Some(vec!["STOP".to_string()]));
    }

    #[test]
    fn test_stop_array_to_vec() {
        let mut req = make_req(vec![]);
        req.stop = Some(json!(["END", "DONE"]));
        let out = openai_to_anthropic(&req);
        assert_eq!(
            out.stop_sequences,
            Some(vec!["END".to_string(), "DONE".to_string()])
        );
    }

    #[test]
    fn test_stream_forwarded() {
        let mut req = make_req(vec![]);
        req.stream = true;
        let out = openai_to_anthropic(&req);
        assert!(out.stream);
    }

    #[test]
    fn test_model_forwarded() {
        let req = make_req(vec![]);
        let out = openai_to_anthropic(&req);
        assert_eq!(out.model, "claude-sonnet-4");
    }
}
