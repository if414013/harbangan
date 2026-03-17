// Cross-format streaming translation
//
// Translates SSE events between OpenAI and Anthropic streaming formats.
// Used when a client hits one endpoint format but the provider returns another.

use std::pin::Pin;

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::ApiError;
use crate::providers::types::ProviderStreamItem;

use crate::models::anthropic::{Delta, MessageDeltaUsage, StreamEvent};
use crate::models::openai::{
    ChatCompletionChunk, ChatCompletionChunkChoice, ChatCompletionChunkDelta, ChatCompletionUsage,
    FunctionCallDelta, PromptTokensDetails, ToolCallDelta,
};

/// State tracker for OpenAI → Anthropic stream translation.
///
/// Anthropic streaming requires explicit content_block_start/stop events
/// and tracks block indices, while OpenAI just sends deltas.
pub struct OpenAIToAnthropicState {
    message_id: String,
    model: String,
    content_block_index: i32,
    has_started_message: bool,
    has_started_text_block: bool,
    tool_block_indices: std::collections::HashMap<i32, i32>, // openai tool index → anthropic block index
}

impl OpenAIToAnthropicState {
    pub fn new(model: &str) -> Self {
        Self {
            message_id: format!("msg_{}", &Uuid::new_v4().simple().to_string()[..24]),
            model: model.to_string(),
            content_block_index: 0,
            has_started_message: false,
            has_started_text_block: false,
            tool_block_indices: std::collections::HashMap::new(),
        }
    }

    /// Translate an OpenAI chunk into zero or more Anthropic stream events.
    pub fn translate_chunk(&mut self, chunk: &ChatCompletionChunk) -> Vec<StreamEvent> {
        let mut events = Vec::new();

        // Emit message_start on first chunk
        if !self.has_started_message {
            self.has_started_message = true;
            let mut usage_json = json!({"input_tokens": 0, "output_tokens": 0});
            // If the first chunk has usage with cache info, include it
            if let Some(u) = &chunk.usage {
                if let Some(details) = &u.prompt_tokens_details {
                    if let Some(cached) = details.cached_tokens {
                        usage_json["cache_read_input_tokens"] = json!(cached);
                    }
                }
            }
            events.push(StreamEvent::MessageStart {
                message: json!({
                    "id": self.message_id,
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": self.model,
                    "usage": usage_json
                }),
            });
        }

        if chunk.choices.is_empty() {
            // Usage-only chunk at end of stream
            if let Some(usage) = &chunk.usage {
                events.push(StreamEvent::MessageDelta {
                    delta: json!({"stop_reason": "end_turn"}),
                    usage: MessageDeltaUsage {
                        output_tokens: usage.completion_tokens,
                    },
                });
            }
            return events;
        }

        let choice = &chunk.choices[0];

        // Text content delta
        if let Some(content) = &choice.delta.content {
            if !content.is_empty() {
                // Start text block if needed
                if !self.has_started_text_block {
                    self.has_started_text_block = true;
                    events.push(StreamEvent::ContentBlockStart {
                        index: self.content_block_index,
                        content_block: json!({"type": "text", "text": ""}),
                    });
                }

                events.push(StreamEvent::ContentBlockDelta {
                    index: self.content_block_index,
                    delta: Delta::TextDelta {
                        text: content.clone(),
                    },
                });
            }
        }

        // Tool call deltas
        if let Some(tool_calls) = &choice.delta.tool_calls {
            for tc in tool_calls {
                let anthropic_index = if let Some(&idx) = self.tool_block_indices.get(&tc.index) {
                    idx
                } else {
                    // Close text block if open
                    if self.has_started_text_block {
                        events.push(StreamEvent::ContentBlockStop {
                            index: self.content_block_index,
                        });
                        self.content_block_index += 1;
                        self.has_started_text_block = false;
                    }

                    let idx = self.content_block_index;
                    self.tool_block_indices.insert(tc.index, idx);
                    self.content_block_index += 1;

                    // Emit content_block_start for tool_use
                    let tool_id = tc.id.clone().unwrap_or_else(|| {
                        format!("toolu_{}", &Uuid::new_v4().simple().to_string()[..24])
                    });
                    let tool_name = tc
                        .function
                        .as_ref()
                        .and_then(|f| f.name.clone())
                        .unwrap_or_default();

                    events.push(StreamEvent::ContentBlockStart {
                        index: idx,
                        content_block: json!({
                            "type": "tool_use",
                            "id": tool_id,
                            "name": tool_name,
                            "input": {}
                        }),
                    });

                    idx
                };

                // Emit input_json_delta if there are arguments
                if let Some(func) = &tc.function {
                    if let Some(args) = &func.arguments {
                        if !args.is_empty() {
                            events.push(StreamEvent::ContentBlockDelta {
                                index: anthropic_index,
                                delta: Delta::InputJsonDelta {
                                    partial_json: args.clone(),
                                },
                            });
                        }
                    }
                }
            }
        }

        // Finish reason
        if let Some(finish_reason) = &choice.finish_reason {
            // Close any open blocks
            if self.has_started_text_block {
                events.push(StreamEvent::ContentBlockStop {
                    index: self.content_block_index,
                });
            }
            // Close any open tool blocks
            for &idx in self.tool_block_indices.values() {
                events.push(StreamEvent::ContentBlockStop { index: idx });
            }

            let stop_reason = match finish_reason.as_str() {
                "tool_calls" => "tool_use",
                "length" => "max_tokens",
                _ => "end_turn",
            };

            let output_tokens = chunk
                .usage
                .as_ref()
                .map(|u| u.completion_tokens)
                .unwrap_or(0);

            events.push(StreamEvent::MessageDelta {
                delta: json!({"stop_reason": stop_reason}),
                usage: MessageDeltaUsage { output_tokens },
            });
            events.push(StreamEvent::MessageStop);
        }

        events
    }
}

/// State tracker for Anthropic → OpenAI stream translation.
pub struct AnthropicToOpenAIState {
    chunk_id: String,
    model: String,
    tool_indices: std::collections::HashMap<i32, ToolBlockInfo>, // anthropic block index → tool info
    /// Captured from MessageStart for cache token passthrough
    start_input_tokens: i32,
    start_cache_creation_input_tokens: Option<i32>,
    start_cache_read_input_tokens: Option<i32>,
}

struct ToolBlockInfo {
    openai_index: i32,
    id: String,
    name: String,
    started: bool,
}

impl AnthropicToOpenAIState {
    pub fn new(model: &str) -> Self {
        Self {
            chunk_id: format!("chatcmpl-{}", &Uuid::new_v4().simple().to_string()[..24]),
            model: model.to_string(),
            tool_indices: std::collections::HashMap::new(),
            start_input_tokens: 0,
            start_cache_creation_input_tokens: None,
            start_cache_read_input_tokens: None,
        }
    }

    /// Translate an Anthropic stream event into an optional OpenAI chunk.
    pub fn translate_event(&mut self, event: &StreamEvent) -> Option<ChatCompletionChunk> {
        match event {
            StreamEvent::MessageStart { message } => {
                // Extract model from message_start if available
                if let Some(m) = message.get("model").and_then(Value::as_str) {
                    self.model = m.to_string();
                }
                // Capture usage from message_start for cache token passthrough
                if let Some(usage) = message.get("usage") {
                    self.start_input_tokens = usage
                        .get("input_tokens")
                        .and_then(Value::as_i64)
                        .unwrap_or(0) as i32;
                    self.start_cache_creation_input_tokens = usage
                        .get("cache_creation_input_tokens")
                        .and_then(Value::as_i64)
                        .map(|v| v as i32);
                    self.start_cache_read_input_tokens = usage
                        .get("cache_read_input_tokens")
                        .and_then(Value::as_i64)
                        .map(|v| v as i32);
                }
                // Emit initial role chunk
                Some(self.make_chunk(
                    ChatCompletionChunkDelta {
                        role: Some("assistant".to_string()),
                        content: None,
                        tool_calls: None,
                        reasoning_content: None,
                    },
                    None,
                    None,
                ))
            }

            StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                // Track tool_use blocks
                if content_block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    let tool_id = content_block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let tool_name = content_block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let openai_index = self.tool_indices.len() as i32;
                    self.tool_indices.insert(
                        *index,
                        ToolBlockInfo {
                            openai_index,
                            id: tool_id,
                            name: tool_name,
                            started: false,
                        },
                    );
                }
                None // No OpenAI chunk for block start
            }

            StreamEvent::ContentBlockDelta { index, delta } => match delta {
                Delta::TextDelta { text } => Some(self.make_chunk(
                    ChatCompletionChunkDelta {
                        role: None,
                        content: Some(text.clone()),
                        tool_calls: None,
                        reasoning_content: None,
                    },
                    None,
                    None,
                )),

                Delta::ThinkingDelta { thinking } => Some(self.make_chunk(
                    ChatCompletionChunkDelta {
                        role: None,
                        content: None,
                        tool_calls: None,
                        reasoning_content: Some(thinking.clone()),
                    },
                    None,
                    None,
                )),

                Delta::InputJsonDelta { partial_json } => {
                    if let Some(tool_info) = self.tool_indices.get_mut(index) {
                        let tc_delta = if !tool_info.started {
                            tool_info.started = true;
                            ToolCallDelta {
                                index: tool_info.openai_index,
                                id: Some(tool_info.id.clone()),
                                tool_type: Some("function".to_string()),
                                function: Some(FunctionCallDelta {
                                    name: Some(tool_info.name.clone()),
                                    arguments: Some(partial_json.clone()),
                                }),
                            }
                        } else {
                            ToolCallDelta {
                                index: tool_info.openai_index,
                                id: None,
                                tool_type: None,
                                function: Some(FunctionCallDelta {
                                    name: None,
                                    arguments: Some(partial_json.clone()),
                                }),
                            }
                        };

                        Some(self.make_chunk(
                            ChatCompletionChunkDelta {
                                role: None,
                                content: None,
                                tool_calls: Some(vec![tc_delta]),
                                reasoning_content: None,
                            },
                            None,
                            None,
                        ))
                    } else {
                        None
                    }
                }

                // Signature deltas are Anthropic-specific (multi-turn thinking replay);
                // no OpenAI equivalent — skip silently
                Delta::SignatureDelta { .. } => None,
            },

            StreamEvent::ContentBlockStop { .. } => None,

            StreamEvent::MessageDelta { delta, usage } => {
                let stop_reason =
                    delta
                        .get("stop_reason")
                        .and_then(Value::as_str)
                        .map(|r| match r {
                            "tool_use" => "tool_calls".to_string(),
                            "max_tokens" => "length".to_string(),
                            _ => "stop".to_string(),
                        });

                let prompt_tokens = self.start_input_tokens;
                let total_tokens = prompt_tokens + usage.output_tokens;

                // Map Anthropic cache_read_input_tokens → OpenAI prompt_tokens_details.cached_tokens
                let prompt_tokens_details =
                    self.start_cache_read_input_tokens
                        .map(|cached| PromptTokensDetails {
                            cached_tokens: Some(cached),
                        });

                let chunk_usage = Some(ChatCompletionUsage {
                    prompt_tokens,
                    completion_tokens: usage.output_tokens,
                    total_tokens,
                    credits_used: None,
                    prompt_tokens_details,
                });

                Some(self.make_chunk(
                    ChatCompletionChunkDelta {
                        role: None,
                        content: None,
                        tool_calls: None,
                        reasoning_content: None,
                    },
                    stop_reason,
                    chunk_usage,
                ))
            }

            StreamEvent::MessageStop | StreamEvent::Ping => None,
        }
    }

    fn make_chunk(
        &self,
        delta: ChatCompletionChunkDelta,
        finish_reason: Option<String>,
        usage: Option<ChatCompletionUsage>,
    ) -> ChatCompletionChunk {
        ChatCompletionChunk {
            id: self.chunk_id.clone(),
            object: "chat.completion.chunk".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: self.model.clone(),
            choices: vec![ChatCompletionChunkChoice {
                index: 0,
                delta,
                finish_reason,
                logprobs: None,
            }],
            usage,
            system_fingerprint: None,
        }
    }
}

// ==================================================================================================
// Stream wrapping helpers
// ==================================================================================================

/// Wrap a parsed SSE stream of Anthropic JSON events, translating each into OpenAI SSE chunks.
///
/// Used when the client hits `/v1/chat/completions` (OpenAI endpoint) but the provider
/// returns Anthropic-format streaming events.
pub fn wrap_anthropic_stream_as_openai<S>(
    sse_values: S,
    model: &str,
) -> Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>>
where
    S: Stream<Item = Result<Value, ApiError>> + Send + 'static,
{
    let model = model.to_string();
    let stream = async_stream::stream! {
        let mut translator = AnthropicToOpenAIState::new(&model);
        futures::pin_mut!(sse_values);

        while let Some(item) = sse_values.next().await {
            match item {
                Ok(json) => {
                    // Parse the JSON value into a StreamEvent
                    if let Ok(event) = serde_json::from_value::<StreamEvent>(json) {
                        if let Some(chunk) = translator.translate_event(&event) {
                            match serde_json::to_string(&chunk) {
                                Ok(s) => yield Ok(Bytes::from(format!("data: {}\n\n", s))),
                                Err(e) => {
                                    tracing::warn!(error = %e, "Failed to serialize OpenAI chunk");
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    yield Err(e);
                    return;
                }
            }
        }

        // Emit [DONE] sentinel
        yield Ok(Bytes::from("data: [DONE]\n\n"));
    };
    Box::pin(stream)
}

/// Wrap a parsed SSE stream of OpenAI JSON chunks, translating each into Anthropic SSE events.
///
/// Used when the client hits `/v1/messages` (Anthropic endpoint) but the provider
/// returns OpenAI-format streaming chunks.
pub fn wrap_openai_stream_as_anthropic<S>(
    sse_values: S,
    model: &str,
) -> Pin<Box<dyn Stream<Item = ProviderStreamItem> + Send>>
where
    S: Stream<Item = Result<Value, ApiError>> + Send + 'static,
{
    let model = model.to_string();
    let stream = async_stream::stream! {
        let mut translator = OpenAIToAnthropicState::new(&model);
        futures::pin_mut!(sse_values);

        while let Some(item) = sse_values.next().await {
            match item {
                Ok(json) => {
                    if let Ok(chunk) = serde_json::from_value::<ChatCompletionChunk>(json) {
                        let events = translator.translate_chunk(&chunk);
                        for event in events {
                            match serde_json::to_value(&event) {
                                Ok(v) => {
                                    let event_type = v.get("type")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("unknown");
                                    yield Ok(Bytes::from(format!(
                                        "event: {}\ndata: {}\n\n",
                                        event_type, v
                                    )));
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "Failed to serialize Anthropic event");
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    yield Err(e);
                    return;
                }
            }
        }
    };
    Box::pin(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== OpenAI → Anthropic ====================

    #[test]
    fn test_openai_to_anthropic_text_content() {
        let mut state = OpenAIToAnthropicState::new("claude-sonnet-4");

        let chunk = ChatCompletionChunk {
            id: "chatcmpl-123".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 0,
            model: "claude-sonnet-4".to_string(),
            choices: vec![ChatCompletionChunkChoice {
                index: 0,
                delta: ChatCompletionChunkDelta {
                    role: Some("assistant".to_string()),
                    content: Some("Hello".to_string()),
                    tool_calls: None,
                    reasoning_content: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: None,
            system_fingerprint: None,
        };

        let events = state.translate_chunk(&chunk);

        // Should get: message_start, content_block_start, content_block_delta
        assert!(events.len() >= 3);
        assert!(matches!(events[0], StreamEvent::MessageStart { .. }));
        assert!(matches!(events[1], StreamEvent::ContentBlockStart { .. }));
        if let StreamEvent::ContentBlockDelta { delta, .. } = &events[2] {
            if let Delta::TextDelta { text } = delta {
                assert_eq!(text, "Hello");
            } else {
                panic!("Expected TextDelta");
            }
        } else {
            panic!("Expected ContentBlockDelta");
        }
    }

    #[test]
    fn test_openai_to_anthropic_finish_stop() {
        let mut state = OpenAIToAnthropicState::new("claude-sonnet-4");
        state.has_started_message = true;

        let chunk = ChatCompletionChunk {
            id: "chatcmpl-123".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 0,
            model: "claude-sonnet-4".to_string(),
            choices: vec![ChatCompletionChunkChoice {
                index: 0,
                delta: ChatCompletionChunkDelta {
                    role: None,
                    content: None,
                    tool_calls: None,
                    reasoning_content: None,
                },
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: None,
            system_fingerprint: None,
        };

        let events = state.translate_chunk(&chunk);

        // Should contain MessageDelta with stop_reason: end_turn and MessageStop
        let has_delta = events.iter().any(|e| {
            if let StreamEvent::MessageDelta { delta, .. } = e {
                delta.get("stop_reason").and_then(Value::as_str) == Some("end_turn")
            } else {
                false
            }
        });
        assert!(has_delta);
        assert!(events.iter().any(|e| matches!(e, StreamEvent::MessageStop)));
    }

    #[test]
    fn test_openai_to_anthropic_finish_tool_calls() {
        let mut state = OpenAIToAnthropicState::new("claude-sonnet-4");
        state.has_started_message = true;

        let chunk = ChatCompletionChunk {
            id: "chatcmpl-123".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 0,
            model: "claude-sonnet-4".to_string(),
            choices: vec![ChatCompletionChunkChoice {
                index: 0,
                delta: ChatCompletionChunkDelta {
                    role: None,
                    content: None,
                    tool_calls: None,
                    reasoning_content: None,
                },
                finish_reason: Some("tool_calls".to_string()),
                logprobs: None,
            }],
            usage: None,
            system_fingerprint: None,
        };

        let events = state.translate_chunk(&chunk);

        let has_tool_use_stop = events.iter().any(|e| {
            if let StreamEvent::MessageDelta { delta, .. } = e {
                delta.get("stop_reason").and_then(Value::as_str) == Some("tool_use")
            } else {
                false
            }
        });
        assert!(has_tool_use_stop);
    }

    #[test]
    fn test_openai_to_anthropic_tool_call_delta() {
        let mut state = OpenAIToAnthropicState::new("claude-sonnet-4");
        state.has_started_message = true;

        let chunk = ChatCompletionChunk {
            id: "chatcmpl-123".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 0,
            model: "claude-sonnet-4".to_string(),
            choices: vec![ChatCompletionChunkChoice {
                index: 0,
                delta: ChatCompletionChunkDelta {
                    role: None,
                    content: None,
                    tool_calls: Some(vec![ToolCallDelta {
                        index: 0,
                        id: Some("call_abc".to_string()),
                        tool_type: Some("function".to_string()),
                        function: Some(FunctionCallDelta {
                            name: Some("get_weather".to_string()),
                            arguments: Some("{\"loc".to_string()),
                        }),
                    }]),
                    reasoning_content: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: None,
            system_fingerprint: None,
        };

        let events = state.translate_chunk(&chunk);

        // Should have content_block_start (tool_use) and content_block_delta (input_json)
        let has_tool_start = events.iter().any(|e| {
            if let StreamEvent::ContentBlockStart { content_block, .. } = e {
                content_block.get("type").and_then(Value::as_str) == Some("tool_use")
            } else {
                false
            }
        });
        assert!(has_tool_start);

        let has_json_delta = events.iter().any(|e| {
            matches!(
                e,
                StreamEvent::ContentBlockDelta {
                    delta: Delta::InputJsonDelta { .. },
                    ..
                }
            )
        });
        assert!(has_json_delta);
    }

    // ==================== Anthropic → OpenAI ====================

    #[test]
    fn test_anthropic_to_openai_message_start() {
        let mut state = AnthropicToOpenAIState::new("claude-sonnet-4");

        let event = StreamEvent::MessageStart {
            message: json!({
                "id": "msg_123",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "claude-sonnet-4",
                "usage": {"input_tokens": 10, "output_tokens": 0}
            }),
        };

        let chunk = state.translate_event(&event);
        assert!(chunk.is_some());
        let chunk = chunk.unwrap();
        assert_eq!(chunk.choices[0].delta.role, Some("assistant".to_string()));
    }

    #[test]
    fn test_anthropic_to_openai_text_delta() {
        let mut state = AnthropicToOpenAIState::new("claude-sonnet-4");

        let event = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::TextDelta {
                text: "Hello".to_string(),
            },
        };

        let chunk = state.translate_event(&event);
        assert!(chunk.is_some());
        let chunk = chunk.unwrap();
        assert_eq!(chunk.choices[0].delta.content, Some("Hello".to_string()));
    }

    #[test]
    fn test_anthropic_to_openai_thinking_delta() {
        let mut state = AnthropicToOpenAIState::new("claude-sonnet-4");

        let event = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::ThinkingDelta {
                thinking: "Let me think...".to_string(),
            },
        };

        let chunk = state.translate_event(&event);
        assert!(chunk.is_some());
        let chunk = chunk.unwrap();
        assert_eq!(
            chunk.choices[0].delta.reasoning_content,
            Some("Let me think...".to_string())
        );
    }

    #[test]
    fn test_anthropic_to_openai_tool_use() {
        let mut state = AnthropicToOpenAIState::new("claude-sonnet-4");

        // First: content_block_start to register the tool
        let start_event = StreamEvent::ContentBlockStart {
            index: 1,
            content_block: json!({
                "type": "tool_use",
                "id": "toolu_abc",
                "name": "get_weather",
                "input": {}
            }),
        };
        assert!(state.translate_event(&start_event).is_none());

        // Then: input_json_delta
        let delta_event = StreamEvent::ContentBlockDelta {
            index: 1,
            delta: Delta::InputJsonDelta {
                partial_json: "{\"location\":".to_string(),
            },
        };

        let chunk = state.translate_event(&delta_event);
        assert!(chunk.is_some());
        let chunk = chunk.unwrap();
        let tool_calls = chunk.choices[0].delta.tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls[0].id, Some("toolu_abc".to_string()));
        assert_eq!(
            tool_calls[0].function.as_ref().unwrap().name,
            Some("get_weather".to_string())
        );
    }

    #[test]
    fn test_anthropic_to_openai_message_delta_stop() {
        let mut state = AnthropicToOpenAIState::new("claude-sonnet-4");

        let event = StreamEvent::MessageDelta {
            delta: json!({"stop_reason": "end_turn"}),
            usage: MessageDeltaUsage { output_tokens: 42 },
        };

        let chunk = state.translate_event(&event);
        assert!(chunk.is_some());
        let chunk = chunk.unwrap();
        assert_eq!(chunk.choices[0].finish_reason, Some("stop".to_string()));
    }

    #[test]
    fn test_anthropic_to_openai_message_delta_tool_use() {
        let mut state = AnthropicToOpenAIState::new("claude-sonnet-4");

        let event = StreamEvent::MessageDelta {
            delta: json!({"stop_reason": "tool_use"}),
            usage: MessageDeltaUsage { output_tokens: 100 },
        };

        let chunk = state.translate_event(&event);
        assert!(chunk.is_some());
        let chunk = chunk.unwrap();
        assert_eq!(
            chunk.choices[0].finish_reason,
            Some("tool_calls".to_string())
        );
    }

    #[test]
    fn test_anthropic_to_openai_ping_ignored() {
        let mut state = AnthropicToOpenAIState::new("claude-sonnet-4");
        assert!(state.translate_event(&StreamEvent::Ping).is_none());
    }

    #[test]
    fn test_anthropic_to_openai_message_stop_ignored() {
        let mut state = AnthropicToOpenAIState::new("claude-sonnet-4");
        assert!(state.translate_event(&StreamEvent::MessageStop).is_none());
    }

    #[test]
    fn test_anthropic_to_openai_content_block_stop_ignored() {
        let mut state = AnthropicToOpenAIState::new("claude-sonnet-4");
        let event = StreamEvent::ContentBlockStop { index: 0 };
        assert!(state.translate_event(&event).is_none());
    }

    #[test]
    fn test_anthropic_to_openai_signature_delta_skipped() {
        let mut state = AnthropicToOpenAIState::new("claude-sonnet-4");
        let event = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::SignatureDelta {
                signature: "sig_abc123".to_string(),
            },
        };
        assert!(state.translate_event(&event).is_none());
    }

    #[test]
    fn test_anthropic_to_openai_cache_tokens_passthrough() {
        let mut state = AnthropicToOpenAIState::new("claude-sonnet-4");

        // MessageStart with cache tokens in usage
        let msg_start = StreamEvent::MessageStart {
            message: json!({
                "id": "msg_test",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "claude-sonnet-4",
                "usage": {
                    "input_tokens": 100,
                    "output_tokens": 0,
                    "cache_creation_input_tokens": 50,
                    "cache_read_input_tokens": 30
                }
            }),
        };
        state.translate_event(&msg_start);

        // Verify state captured the cache tokens
        assert_eq!(state.start_input_tokens, 100);
        assert_eq!(state.start_cache_creation_input_tokens, Some(50));
        assert_eq!(state.start_cache_read_input_tokens, Some(30));

        // MessageDelta should include cache tokens in final usage
        let msg_delta = StreamEvent::MessageDelta {
            delta: json!({"stop_reason": "end_turn"}),
            usage: MessageDeltaUsage { output_tokens: 42 },
        };
        let chunk = state.translate_event(&msg_delta).unwrap();
        let usage = chunk.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 42);
        assert_eq!(usage.total_tokens, 142);
        let details = usage.prompt_tokens_details.unwrap();
        assert_eq!(details.cached_tokens, Some(30));
    }

    #[test]
    fn test_anthropic_to_openai_no_cache_tokens() {
        let mut state = AnthropicToOpenAIState::new("claude-sonnet-4");

        // MessageStart without cache tokens
        let msg_start = StreamEvent::MessageStart {
            message: json!({
                "id": "msg_test",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "claude-sonnet-4",
                "usage": {"input_tokens": 50, "output_tokens": 0}
            }),
        };
        state.translate_event(&msg_start);

        let msg_delta = StreamEvent::MessageDelta {
            delta: json!({"stop_reason": "end_turn"}),
            usage: MessageDeltaUsage { output_tokens: 10 },
        };
        let chunk = state.translate_event(&msg_delta).unwrap();
        let usage = chunk.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 50);
        assert!(usage.prompt_tokens_details.is_none());
    }

    #[test]
    fn test_openai_to_anthropic_cache_tokens_in_message_start() {
        let mut state = OpenAIToAnthropicState::new("gpt-4o");

        // First chunk with usage containing cache info
        let chunk = ChatCompletionChunk {
            id: "chatcmpl-test".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 0,
            model: "gpt-4o".to_string(),
            choices: vec![ChatCompletionChunkChoice {
                index: 0,
                delta: ChatCompletionChunkDelta {
                    role: Some("assistant".to_string()),
                    content: None,
                    tool_calls: None,
                    reasoning_content: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: Some(ChatCompletionUsage {
                prompt_tokens: 100,
                completion_tokens: 0,
                total_tokens: 100,
                credits_used: None,
                prompt_tokens_details: Some(PromptTokensDetails {
                    cached_tokens: Some(80),
                }),
            }),
            system_fingerprint: None,
        };

        let events = state.translate_chunk(&chunk);
        // First event should be MessageStart
        let msg_start = &events[0];
        if let StreamEvent::MessageStart { message } = msg_start {
            let usage = message.get("usage").unwrap();
            assert_eq!(usage["cache_read_input_tokens"], 80);
        } else {
            panic!("expected MessageStart");
        }
    }
}
