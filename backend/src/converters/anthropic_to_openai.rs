/// Convert Anthropic AnthropicMessagesRequest to OpenAI ChatCompletionRequest.
use serde_json::{json, Value};

use crate::converters::core::map_tool_choice_anthropic_to_openai;
use crate::models::anthropic::{AnthropicMessagesRequest, AnthropicTool};
use crate::models::openai::{
    ChatCompletionRequest, ChatMessage, FunctionCall, FunctionTool, Tool, ToolCall, ToolFunction,
};

fn tool_result_content_to_openai(content: &Value) -> Value {
    if content.is_null() {
        Value::String(String::new())
    } else {
        content.clone()
    }
}

/// Convert an Anthropic-format request to OpenAI format.
///
/// Mapping rules:
/// - `system` (string or block array) → prepended as a `system` role ChatMessage
/// - `user`/`assistant` messages are passed through with their content
/// - `max_tokens` → `max_tokens`
/// - `temperature`, `top_p` are passed through when present
/// - `stop_sequences` → `stop` (as a JSON array)
/// - `tool_choice` is mapped: {"type":"auto"} → "auto", {"type":"any"} → "required", etc.
/// - `disable_parallel_tool_use: true` → `parallel_tool_calls: false`
/// - `thinking` config → `reasoning_effort` (based on budget_tokens)
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
        if let Some(blocks) = msg.content.as_array() {
            // Content is an array of blocks — check for tool_use / tool_result
            let mut text_parts: Vec<String> = Vec::new();
            let mut tool_calls_out: Vec<ToolCall> = Vec::new();
            let mut tool_results: Vec<(String, Value)> = Vec::new(); // (tool_use_id, content)

            for block in blocks {
                let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match block_type {
                    "text" => {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            text_parts.push(text.to_string());
                        }
                    }
                    "tool_use" => {
                        let id = block
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = block
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let input = block.get("input").cloned().unwrap_or(json!({}));
                        let arguments = serde_json::to_string(&input).unwrap_or_default();
                        tool_calls_out.push(ToolCall {
                            id,
                            tool_type: "function".to_string(),
                            function: FunctionCall { name, arguments },
                        });
                    }
                    "tool_result" => {
                        let tool_use_id = block
                            .get("tool_use_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let content = block
                            .get("content")
                            .map(tool_result_content_to_openai)
                            .unwrap_or_else(|| Value::String(String::new()));
                        tool_results.push((tool_use_id, content));
                    }
                    _ => {
                        // Pass through other block types as text if they have text
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            text_parts.push(text.to_string());
                        }
                    }
                }
            }

            // Emit tool results as separate "tool" role messages
            for (tool_use_id, content) in tool_results {
                messages.push(ChatMessage {
                    role: "tool".to_string(),
                    content: Some(content),
                    name: None,
                    tool_calls: None,
                    tool_call_id: Some(tool_use_id),
                });
            }

            // Emit the assistant/user message with text + tool_calls
            if !text_parts.is_empty() || !tool_calls_out.is_empty() {
                let content_text = text_parts.join("\n");
                messages.push(ChatMessage {
                    role: msg.role.clone(),
                    content: if content_text.is_empty() {
                        None
                    } else {
                        Some(serde_json::Value::String(content_text))
                    },
                    name: None,
                    tool_calls: if tool_calls_out.is_empty() {
                        None
                    } else {
                        Some(tool_calls_out)
                    },
                    tool_call_id: None,
                });
            }
        } else {
            // Content is a plain string
            messages.push(ChatMessage {
                role: msg.role.clone(),
                content: Some(msg.content.clone()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            });
        }
    }

    let stop = req.stop_sequences.as_ref().map(|seqs| {
        serde_json::Value::Array(
            seqs.iter()
                .map(|s| serde_json::Value::String(s.clone()))
                .collect(),
        )
    });

    // Map tool_choice and disable_parallel_tool_use
    let (tool_choice, parallel_tool_calls) =
        map_tool_choice_anthropic_to_openai(&req.tool_choice, req.disable_parallel_tool_use);

    // Map thinking config to reasoning_effort
    let reasoning_effort = req.thinking.as_ref().and_then(|thinking| {
        let obj = thinking.as_object()?;
        obj.get("budget_tokens")
            .and_then(|v| v.as_i64())
            .map(|budget| {
                if budget <= 1000 {
                    "low".to_string()
                } else if budget <= 2000 {
                    "medium".to_string()
                } else {
                    "high".to_string()
                }
            })
    });

    // Map Anthropic tools to OpenAI format
    let tools: Option<Vec<Tool>> = req.tools.as_ref().map(|tools| {
        tools
            .iter()
            .filter_map(|t| match t {
                AnthropicTool::Custom(ct) => Some(Tool::Function(FunctionTool {
                    tool_type: "function".to_string(),
                    function: ToolFunction {
                        name: ct.name.clone(),
                        description: ct.description.clone(),
                        parameters: Some(ct.input_schema.clone()),
                    },
                })),
                AnthropicTool::ServerSide(_) => None,
            })
            .collect()
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
        tools,
        tool_choice,
        stream_options: None,
        logit_bias: None,
        logprobs: None,
        top_logprobs: None,
        user: None,
        seed: None,
        parallel_tool_calls,
        reasoning_effort,
        response_format: None,
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
            thinking: None,
            stop_sequences: None,
            metadata: None,
            disable_parallel_tool_use: None,
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
