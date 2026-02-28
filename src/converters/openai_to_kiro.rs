// OpenAI to Kiro converter
//
// This module converts OpenAI API format to Kiro API format.
// It acts as an adapter layer that converts OpenAI-specific formats
// to the unified format used by the core converter.

use serde_json::{json, Value};
use tracing::debug;

use crate::config::Config;
use crate::models::openai::{ChatCompletionRequest, ChatMessage, Tool};
use crate::resolver::normalize_model_name;

use super::core::{
    build_kiro_history, convert_images_to_kiro_format, convert_tool_results_to_kiro_format,
    convert_tools_to_kiro_format, ensure_assistant_before_tool_results,
    extract_images_from_content, extract_text_content, extract_tool_results_from_content,
    get_thinking_system_prompt_addition, inject_thinking_tags, merge_adjacent_messages,
    process_tools_with_long_descriptions, strip_all_tool_content, ContentBlock, KiroPayloadResult,
    MessageContent, ToolCall, ToolFunction, ToolResult, UnifiedMessage, UnifiedTool,
};

// ==================================================================================================
// OpenAI-specific Message Processing
// ==================================================================================================

/// Converts OpenAI content to MessageContent.
///
/// OpenAI content can be:
/// - String: "Hello, world!"
/// - List of content blocks: [{"type": "text", "text": "Hello"}, {"type": "image_url", "image_url": {"url": "..."}}]
fn convert_openai_content(content: &Value) -> MessageContent {
    if let Some(text) = content.as_str() {
        return MessageContent::Text(text.to_string());
    }

    if let Some(blocks) = content.as_array() {
        let content_blocks: Vec<ContentBlock> = blocks
            .iter()
            .filter_map(|block| {
                let block_type = block.get("type")?.as_str()?;
                match block_type {
                    "text" => {
                        let text = block.get("text")?.as_str()?.to_string();
                        Some(ContentBlock::Text { text })
                    }
                    "image_url" => {
                        let image_url_obj = block.get("image_url")?;
                        let url = image_url_obj.get("url")?.as_str()?.to_string();
                        Some(ContentBlock::ImageUrl {
                            image_url: super::core::ImageUrl { url },
                        })
                    }
                    _ => None,
                }
            })
            .collect();

        return MessageContent::Blocks(content_blocks);
    }

    MessageContent::Text(content.to_string())
}

/// Extracts tool results from OpenAI message content.
fn extract_tool_results_from_openai(content: &MessageContent) -> Vec<ToolResult> {
    let mut tool_results = Vec::new();

    if let MessageContent::Blocks(blocks) = content {
        for block in blocks {
            if let ContentBlock::ToolResult {
                tool_use_id,
                content,
            } = block
            {
                tool_results.push(ToolResult {
                    result_type: "tool_result".to_string(),
                    tool_use_id: tool_use_id.clone(),
                    content: if content.is_empty() {
                        "(empty result)".to_string()
                    } else {
                        content.clone()
                    },
                });
            }
        }
    }

    tool_results
}

/// Extracts tool calls from OpenAI assistant message.
fn extract_tool_calls_from_openai(msg: &ChatMessage) -> Option<Vec<ToolCall>> {
    msg.tool_calls.as_ref().map(|calls| {
        calls
            .iter()
            .map(|tc| ToolCall {
                id: tc.id.clone(),
                call_type: "function".to_string(),
                function: ToolFunction {
                    name: tc.function.name.clone(),
                    arguments: tc.function.arguments.clone(),
                },
            })
            .collect()
    })
}

/// Converts OpenAI messages to unified format.
///
/// Handles:
/// - System messages (extracted as system prompt)
/// - Tool messages (converted to user messages with tool_results)
/// - Tool calls in assistant messages
pub fn convert_openai_messages_to_unified(
    messages: &[ChatMessage],
) -> (String, Vec<UnifiedMessage>) {
    let mut system_prompt = String::new();
    let mut non_system_messages = Vec::new();

    // Extract system prompt
    for msg in messages {
        if msg.role == "system" {
            let content = match &msg.content {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => String::new(),
            };
            system_prompt.push_str(&content);
            system_prompt.push('\n');
        } else {
            non_system_messages.push(msg);
        }
    }

    system_prompt = system_prompt.trim().to_string();

    // Process tool messages - convert to user messages with tool_results
    let mut processed = Vec::new();
    let mut pending_tool_results = Vec::new();
    let mut total_tool_calls = 0;
    let mut total_tool_results = 0;
    let mut total_images = 0;

    for msg in non_system_messages {
        if msg.role == "tool" {
            // Collect tool results
            let content = match &msg.content {
                Some(serde_json::Value::String(s)) => s.as_str(),
                Some(v) => {
                    // Try to extract text from JSON value
                    v.as_str().unwrap_or("(empty result)")
                }
                None => "(empty result)",
            };
            let tool_result = ToolResult {
                result_type: "tool_result".to_string(),
                tool_use_id: msg.tool_call_id.clone().unwrap_or_default(),
                content: content.to_string(),
            };
            pending_tool_results.push(tool_result);
            total_tool_results += 1;
        } else {
            // If there are accumulated tool results, create user message with them
            if !pending_tool_results.is_empty() {
                let unified_msg = UnifiedMessage {
                    role: "user".to_string(),
                    content: MessageContent::Text(String::new()),
                    tool_calls: None,
                    tool_results: Some(pending_tool_results.clone()),
                    images: None,
                };
                processed.push(unified_msg);
                pending_tool_results.clear();
            }

            // Convert regular message
            let content = match &msg.content {
                Some(v) => convert_openai_content(v),
                None => MessageContent::Text(String::new()),
            };

            let tool_calls = if msg.role == "assistant" {
                let calls = extract_tool_calls_from_openai(msg);
                if let Some(ref c) = calls {
                    total_tool_calls += c.len();
                }
                calls
            } else {
                None
            };

            let tool_results = if msg.role == "user" {
                let results = extract_tool_results_from_openai(&content);
                if !results.is_empty() {
                    total_tool_results += results.len();
                    Some(results)
                } else {
                    None
                }
            } else {
                None
            };

            let images = if msg.role == "user" {
                let imgs = extract_images_from_content(&content);
                if !imgs.is_empty() {
                    total_images += imgs.len();
                    Some(imgs)
                } else {
                    None
                }
            } else {
                None
            };

            let unified_msg = UnifiedMessage {
                role: msg.role.clone(),
                content,
                tool_calls,
                tool_results,
                images,
            };
            processed.push(unified_msg);
        }
    }

    // If tool results remain at the end
    if !pending_tool_results.is_empty() {
        let unified_msg = UnifiedMessage {
            role: "user".to_string(),
            content: MessageContent::Text(String::new()),
            tool_calls: None,
            tool_results: Some(pending_tool_results),
            images: None,
        };
        processed.push(unified_msg);
    }

    // Log summary if any tool content or images were found
    if total_tool_calls > 0 || total_tool_results > 0 || total_images > 0 {
        debug!(
            "Converted {} OpenAI messages: {} tool_calls, {} tool_results, {} images",
            messages.len(),
            total_tool_calls,
            total_tool_results,
            total_images
        );
    }

    (system_prompt, processed)
}

/// Converts OpenAI tools to unified format.
pub fn convert_openai_tools_to_unified(tools: &Option<Vec<Tool>>) -> Option<Vec<UnifiedTool>> {
    tools.as_ref().map(|tools| {
        tools
            .iter()
            .filter(|tool| tool.tool_type == "function")
            .map(|tool| UnifiedTool {
                name: tool.function.name.clone(),
                description: tool.function.description.clone(),
                input_schema: tool.function.parameters.clone(),
            })
            .collect()
    })
}

// ==================================================================================================
// Main Entry Point
// ==================================================================================================

/// Builds complete payload for Kiro API from OpenAI request.
///
/// This is the main entry point for OpenAI → Kiro conversion.
pub fn build_kiro_payload(
    request: &ChatCompletionRequest,
    conversation_id: &str,
    profile_arn: &str,
    config: &Config,
) -> Result<KiroPayloadResult, String> {
    // Convert messages to unified format
    let (system_prompt, unified_messages) = convert_openai_messages_to_unified(&request.messages);

    // Convert tools to unified format
    let unified_tools = convert_openai_tools_to_unified(&request.tools);

    // Normalize model name
    let model_id = normalize_model_name(&request.model);

    debug!(
        "Converting OpenAI request: model={} -> {}, messages={}, tools={}, system_prompt_length={}",
        request.model,
        model_id,
        unified_messages.len(),
        unified_tools.as_ref().map_or(0, |t| t.len()),
        system_prompt.len()
    );

    // Build Kiro payload using core function
    build_kiro_payload_core(
        unified_messages,
        system_prompt,
        &model_id,
        unified_tools,
        conversation_id,
        profile_arn,
        true, // inject_thinking
        config,
    )
}

/// Core function to build Kiro payload from unified data.
///
/// This is shared logic that can be used by all converters.
#[allow(clippy::too_many_arguments)]
pub fn build_kiro_payload_core(
    messages: Vec<UnifiedMessage>,
    system_prompt: String,
    model_id: &str,
    tools: Option<Vec<UnifiedTool>>,
    conversation_id: &str,
    profile_arn: &str,
    inject_thinking: bool,
    config: &Config,
) -> Result<KiroPayloadResult, String> {
    // Process tools with long descriptions
    let (processed_tools, tool_documentation) =
        process_tools_with_long_descriptions(tools.clone(), config);

    // Add tool documentation to system prompt if present
    let mut full_system_prompt = system_prompt;
    debug!("Initial system_prompt length: {}", full_system_prompt.len());
    debug!("Tool documentation length: {}", tool_documentation.len());

    if !tool_documentation.is_empty() {
        if !full_system_prompt.is_empty() {
            full_system_prompt.push_str(&tool_documentation);
        } else {
            full_system_prompt = tool_documentation.trim().to_string();
        }
    }
    debug!(
        "After tool documentation, full_system_prompt length: {}",
        full_system_prompt.len()
    );

    // Add thinking mode legitimization to system prompt if enabled
    let thinking_system_addition = get_thinking_system_prompt_addition(config);
    if !thinking_system_addition.is_empty() {
        if !full_system_prompt.is_empty() {
            full_system_prompt.push_str(&thinking_system_addition);
        } else {
            full_system_prompt = thinking_system_addition.trim().to_string();
        }
    }
    debug!(
        "After thinking addition, full_system_prompt length: {}",
        full_system_prompt.len()
    );

    // Add truncation recovery legitimization to system prompt if enabled
    let truncation_system_addition =
        crate::truncation::get_truncation_recovery_system_addition(config.truncation_recovery);
    if !truncation_system_addition.is_empty() {
        if !full_system_prompt.is_empty() {
            full_system_prompt.push_str(&truncation_system_addition);
        } else {
            full_system_prompt = truncation_system_addition.trim().to_string();
        }
    }
    debug!(
        "After truncation recovery addition, full_system_prompt length: {}",
        full_system_prompt.len()
    );

    // If no tools are defined, strip ALL tool-related content from messages
    // Kiro API rejects requests with toolResults but no tools
    let (messages_with_tools_handled, _stripped_tool_results) = if tools.is_none() {
        strip_all_tool_content(messages)
    } else {
        // Ensure assistant messages exist before tool_results (Kiro API requirement)
        ensure_assistant_before_tool_results(messages)
    };

    // Merge adjacent messages with the same role
    let merged_messages = merge_adjacent_messages(messages_with_tools_handled);

    if merged_messages.is_empty() {
        return Err("No messages to send".to_string());
    }

    // Build history (all messages except the last one)
    let history_messages = if merged_messages.len() > 1 {
        &merged_messages[..merged_messages.len() - 1]
    } else {
        &[]
    };

    // If there's a system prompt, add it to the first user message in history
    let mut history_messages_vec = history_messages.to_vec();
    debug!(
        "History messages count: {}, first role: {:?}",
        history_messages_vec.len(),
        history_messages_vec.first().map(|m| &m.role)
    );
    if !full_system_prompt.is_empty() && !history_messages_vec.is_empty() {
        if history_messages_vec[0].role == "user" {
            let original_content = extract_text_content(&history_messages_vec[0].content);
            debug!(
                "Adding system prompt to first history message (original content: {} chars)",
                original_content.len()
            );
            history_messages_vec[0].content =
                MessageContent::Text(format!("{}\n\n{}", full_system_prompt, original_content));
            debug!(
                "First history message now has {} chars",
                extract_text_content(&history_messages_vec[0].content).len()
            );
        } else {
            debug!("First history message is not user role, skipping system prompt injection to history");
        }
    }

    let history = build_kiro_history(&history_messages_vec, model_id);

    // Current message (the last one)
    let current_message = &merged_messages[merged_messages.len() - 1];
    let mut current_content = extract_text_content(&current_message.content);
    debug!(
        "Current content length before system prompt: {}",
        current_content.len()
    );

    // If system prompt exists but history is empty - add to current message
    debug!(
        "Checking system prompt injection: full_system_prompt.is_empty()={}, history.is_empty()={}",
        full_system_prompt.is_empty(),
        history.is_empty()
    );
    if !full_system_prompt.is_empty() && history.is_empty() {
        debug!(
            "Adding system prompt ({} chars) to current message",
            full_system_prompt.len()
        );
        current_content = format!("{}\n\n{}", full_system_prompt, current_content);
        debug!(
            "Current content length after system prompt: {}",
            current_content.len()
        );
    }

    // If current message is assistant, add it to history and create "Continue" message
    let mut final_history = history;
    if current_message.role == "assistant" {
        final_history.push(json!({
            "assistantResponseMessage": {
                "content": current_content
            }
        }));
        current_content = "Continue".to_string();
    }

    // If content is empty - use "Continue"
    if current_content.is_empty() {
        current_content = "Continue".to_string();
    }

    // Process images in current message
    let images_to_convert = if let Some(imgs) = &current_message.images {
        Some(imgs.clone())
    } else {
        let extracted = extract_images_from_content(&current_message.content);
        if extracted.is_empty() {
            None
        } else {
            Some(extracted)
        }
    };

    let kiro_images = images_to_convert.map(|imgs| convert_images_to_kiro_format(&Some(imgs)));

    // Build user_input_context for tools and toolResults only
    let mut user_input_context = json!({});

    // Add tools if present
    let kiro_tools = convert_tools_to_kiro_format(&processed_tools);
    if !kiro_tools.is_empty() {
        user_input_context["tools"] = Value::Array(kiro_tools);
    }

    // Process tool_results in current message - convert to Kiro format if present
    if let Some(tool_results) = &current_message.tool_results {
        let kiro_tool_results = convert_tool_results_to_kiro_format(tool_results);
        if !kiro_tool_results.is_empty() {
            user_input_context["toolResults"] = Value::Array(kiro_tool_results);
        }
    } else {
        // Try to extract from content (already in Kiro format)
        let tool_results = extract_tool_results_from_content(&current_message.content);
        if !tool_results.is_empty() {
            user_input_context["toolResults"] = Value::Array(tool_results);
        }
    }

    // Inject thinking tags if enabled
    if inject_thinking && current_message.role == "user" {
        current_content = inject_thinking_tags(current_content, config);
    }

    // Build userInputMessage
    let mut user_input_message = json!({
        "content": current_content,
        "modelId": model_id,
        "origin": "AI_EDITOR",
    });

    // Add images directly to userInputMessage
    if let Some(imgs) = kiro_images {
        if !imgs.is_empty() {
            user_input_message["images"] = Value::Array(imgs);
        }
    }

    // Add user_input_context if present
    if user_input_context
        .as_object()
        .is_some_and(|o| !o.is_empty())
    {
        user_input_message["userInputMessageContext"] = user_input_context;
    }

    // Assemble final payload
    let mut payload = json!({
        "conversationState": {
            "chatTriggerType": "MANUAL",
            "conversationId": conversation_id,
            "currentMessage": {
                "userInputMessage": user_input_message
            }
        }
    });

    // Add history only if not empty
    if !final_history.is_empty() {
        payload["conversationState"]["history"] = Value::Array(final_history);
    }

    // Add profileArn
    if !profile_arn.is_empty() {
        payload["profileArn"] = Value::String(profile_arn.to_string());
    }

    Ok(KiroPayloadResult {
        payload,
        tool_documentation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::openai::{FunctionCall, ToolCall, ToolFunction};
    use serde_json::json;

    fn create_test_config() -> Config {
        Config {
            server_host: "0.0.0.0".to_string(),
            server_port: 8000,
            proxy_api_key: "test".to_string(),
            kiro_region: "us-east-1".to_string(),
            kiro_cli_db_file: std::path::PathBuf::from("/tmp/test.db"),
            streaming_timeout: 300,
            token_refresh_threshold: 300,
            first_token_timeout: 15,
            http_max_connections: 20,
            http_connect_timeout: 30,
            http_request_timeout: 300,
            http_max_retries: 3,
            debug_mode: crate::config::DebugMode::Off,
            log_level: "info".to_string(),
            tool_description_max_length: 10000,
            fake_reasoning_enabled: false,
            fake_reasoning_max_tokens: 4000,
            fake_reasoning_handling: crate::config::FakeReasoningHandling::AsReasoningContent,
            truncation_recovery: true,
            dashboard: false,
            tls_enabled: false,
            tls_cert_path: None,
            tls_key_path: None,
        }
    }

    #[test]
    fn test_convert_openai_messages_basic() {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: Some(json!("You are a helpful assistant.")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: Some(json!("Hello!")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let (system_prompt, unified) = convert_openai_messages_to_unified(&messages);
        assert_eq!(system_prompt, "You are a helpful assistant.");
        assert_eq!(unified.len(), 1);
        assert_eq!(unified[0].role, "user");
    }

    #[test]
    fn test_convert_openai_tools() {
        let tools = vec![Tool {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: "get_weather".to_string(),
                description: Some("Get weather".to_string()),
                parameters: Some(json!({"type": "object"})),
            },
        }];

        let unified = convert_openai_tools_to_unified(&Some(tools));
        assert!(unified.is_some());
        let tools = unified.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "get_weather");
    }

    #[test]
    fn test_convert_openai_tools_none() {
        let unified = convert_openai_tools_to_unified(&None);
        assert!(unified.is_none());
    }

    #[test]
    fn test_convert_openai_tools_empty() {
        let unified = convert_openai_tools_to_unified(&Some(vec![]));
        // Empty vec returns Some(empty vec), not None
        assert!(unified.is_some());
        assert!(unified.unwrap().is_empty());
    }

    #[test]
    fn test_convert_openai_messages_multiple_system() {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: Some(json!("First system message.")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "system".to_string(),
                content: Some(json!("Second system message.")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: Some(json!("Hello!")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let (system_prompt, unified) = convert_openai_messages_to_unified(&messages);
        // Multiple system messages should be concatenated
        assert!(system_prompt.contains("First system message"));
        assert!(system_prompt.contains("Second system message"));
        assert_eq!(unified.len(), 1);
    }

    #[test]
    fn test_convert_openai_messages_with_tool_calls() {
        let messages = vec![
            ChatMessage {
                role: "user".to_string(),
                content: Some(json!("What's the weather?")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: Some(json!("Let me check.")),
                name: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_123".to_string(),
                    tool_type: "function".to_string(),
                    function: FunctionCall {
                        name: "get_weather".to_string(),
                        arguments: "{\"location\": \"SF\"}".to_string(),
                    },
                }]),
                tool_call_id: None,
            },
        ];

        let (_, unified) = convert_openai_messages_to_unified(&messages);
        assert_eq!(unified.len(), 2);
        assert!(unified[1].tool_calls.is_some());
        let tool_calls = unified[1].tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].function.name, "get_weather");
    }

    #[test]
    fn test_convert_openai_messages_with_tool_result() {
        let messages = vec![ChatMessage {
            role: "tool".to_string(),
            content: Some(json!("Sunny, 72°F")),
            name: None,
            tool_calls: None,
            tool_call_id: Some("call_123".to_string()),
        }];

        let (_, unified) = convert_openai_messages_to_unified(&messages);
        assert_eq!(unified.len(), 1);
        assert_eq!(unified[0].role, "user"); // tool messages become user messages
        assert!(unified[0].tool_results.is_some());
    }

    #[test]
    fn test_convert_openai_messages_content_array() {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: Some(json!([
                {"type": "text", "text": "Hello"},
                {"type": "text", "text": " World"}
            ])),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }];

        let (_, unified) = convert_openai_messages_to_unified(&messages);
        assert_eq!(unified.len(), 1);
    }

    #[test]
    fn test_convert_openai_messages_with_image() {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: Some(json!([
                {"type": "text", "text": "What's in this image?"},
                {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,/9j/4AAQSkZJRg=="}}
            ])),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }];

        let (_, unified) = convert_openai_messages_to_unified(&messages);
        assert_eq!(unified.len(), 1);
        assert_eq!(unified[0].role, "user");

        // Verify images were extracted
        assert!(unified[0].images.is_some());
        let images = unified[0].images.as_ref().unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].media_type, "image/jpeg");
        assert_eq!(images[0].data, "/9j/4AAQSkZJRg==");
    }

    #[test]
    fn test_build_kiro_payload_basic() {
        let config = create_test_config();
        let request = ChatCompletionRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some(json!("Hello!")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
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
        };

        let result = build_kiro_payload(&request, "conv-123", "profile-arn", &config);
        assert!(result.is_ok());

        let payload_result = result.unwrap();
        let payload = payload_result.payload;

        assert!(payload["conversationState"]["conversationId"]
            .as_str()
            .is_some());
        assert!(
            payload["conversationState"]["currentMessage"]["userInputMessage"]["content"]
                .as_str()
                .is_some()
        );
    }

    #[test]
    fn test_build_kiro_payload_with_system() {
        let config = create_test_config();
        let request = ChatCompletionRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: Some(json!("You are helpful.")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: Some(json!("Hello!")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
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
        };

        let result = build_kiro_payload(&request, "conv-123", "profile-arn", &config);
        assert!(result.is_ok());

        let payload_result = result.unwrap();
        let payload = payload_result.payload;

        // System prompt should be included in the content
        let content = payload["conversationState"]["currentMessage"]["userInputMessage"]["content"]
            .as_str()
            .unwrap();
        assert!(content.contains("You are helpful"));
    }

    #[test]
    fn test_build_kiro_payload_with_tools() {
        let config = create_test_config();
        let request = ChatCompletionRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Some(json!("What's the weather?")),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            stream: false,
            temperature: None,
            top_p: None,
            n: None,
            max_tokens: None,
            max_completion_tokens: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
            tools: Some(vec![Tool {
                tool_type: "function".to_string(),
                function: ToolFunction {
                    name: "get_weather".to_string(),
                    description: Some("Get weather for a location".to_string()),
                    parameters: Some(json!({
                        "type": "object",
                        "properties": {
                            "location": {"type": "string"}
                        }
                    })),
                },
            }]),
            tool_choice: None,
            stream_options: None,
            logit_bias: None,
            logprobs: None,
            top_logprobs: None,
            user: None,
            seed: None,
            parallel_tool_calls: None,
        };

        let result = build_kiro_payload(&request, "conv-123", "profile-arn", &config);
        assert!(result.is_ok());

        let payload_result = result.unwrap();
        let payload = payload_result.payload;

        // Tools should be in userInputMessageContext
        let tools = &payload["conversationState"]["currentMessage"]["userInputMessage"]
            ["userInputMessageContext"]["tools"];
        assert!(tools.is_array());
        assert_eq!(tools.as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_build_kiro_payload_empty_messages() {
        let config = create_test_config();
        let request = ChatCompletionRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![],
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
        };

        let result = build_kiro_payload(&request, "conv-123", "profile-arn", &config);
        assert!(result.is_err());
    }

    // ==================================================================================================
    // Compaction Scenario Tests
    //
    // OpenCode compaction returns 400 "Improperly formed request"
    // because it sends tool_calls/tool_results in history but WITHOUT tools definitions.
    // ==================================================================================================

    #[test]
    fn test_compaction_without_tools_converts_tool_content_to_text() {
        // Simulates OpenCode compaction scenario - messages with tool content but no tools.
        // Purpose: Ensure build_kiro_payload doesn't crash and converts tool content to text.
        let config = create_test_config();

        let request = ChatCompletionRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![
                ChatMessage {
                    role: "user".to_string(),
                    content: Some(json!("Read the file test.py")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: "assistant".to_string(),
                    content: Some(json!("I'll read that file for you.")),
                    name: None,
                    tool_calls: Some(vec![ToolCall {
                        id: "call_read_123".to_string(),
                        tool_type: "function".to_string(),
                        function: FunctionCall {
                            name: "Read".to_string(),
                            arguments: r#"{"file_path": "test.py"}"#.to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
                ChatMessage {
                    role: "tool".to_string(),
                    content: Some(json!("def hello():\n    print('Hello, World!')")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: Some("call_read_123".to_string()),
                },
                ChatMessage {
                    role: "assistant".to_string(),
                    content: Some(json!("The file contains a simple hello function.")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: Some(json!("Thanks! Now summarize what we did.")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            stream: false,
            temperature: None,
            top_p: None,
            n: None,
            max_tokens: None,
            max_completion_tokens: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
            tools: None, // NO TOOLS - this is the compaction scenario
            tool_choice: None,
            stream_options: None,
            logit_bias: None,
            logprobs: None,
            top_logprobs: None,
            user: None,
            seed: None,
            parallel_tool_calls: None,
        };

        let result = build_kiro_payload(&request, "conv-123", "profile-arn", &config);

        // Should succeed without error
        assert!(result.is_ok(), "Compaction scenario should not fail");

        let payload_result = result.unwrap();
        let payload = payload_result.payload;

        // Verify the payload was built successfully
        assert!(payload["conversationState"]["conversationId"]
            .as_str()
            .is_some());

        // Verify history exists and tool content was converted to text
        let history = payload["conversationState"]["history"].as_array();
        assert!(history.is_some(), "History should exist");

        // Check that tool calls were converted to text in assistant message
        let history_arr = history.unwrap();
        let mut found_tool_text = false;
        for msg in history_arr {
            if let Some(assistant_msg) = msg.get("assistantResponseMessage") {
                let content = assistant_msg["content"].as_str().unwrap_or("");
                if content.contains("[Tool: Read") {
                    found_tool_text = true;
                }
            }
        }
        assert!(
            found_tool_text,
            "Tool calls should be converted to text representation"
        );
    }

    #[test]
    fn test_compaction_preserves_tool_result_content_as_text() {
        // Verifies that tool result content is preserved as text during compaction.
        // Purpose: Ensure the actual tool output is not lost during compaction.
        let config = create_test_config();

        let important_data = "IMPORTANT_DATA_12345";

        let request = ChatCompletionRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![
                ChatMessage {
                    role: "user".to_string(),
                    content: Some(json!("Get the data")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: "assistant".to_string(),
                    content: Some(json!("Fetching data...")),
                    name: None,
                    tool_calls: Some(vec![ToolCall {
                        id: "call_data_456".to_string(),
                        tool_type: "function".to_string(),
                        function: FunctionCall {
                            name: "fetch_data".to_string(),
                            arguments: r#"{}"#.to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
                ChatMessage {
                    role: "tool".to_string(),
                    content: Some(json!(important_data)),
                    name: None,
                    tool_calls: None,
                    tool_call_id: Some("call_data_456".to_string()),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: Some(json!("What was the result?")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            stream: false,
            temperature: None,
            top_p: None,
            n: None,
            max_tokens: None,
            max_completion_tokens: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
            tools: None, // NO TOOLS - compaction scenario
            tool_choice: None,
            stream_options: None,
            logit_bias: None,
            logprobs: None,
            top_logprobs: None,
            user: None,
            seed: None,
            parallel_tool_calls: None,
        };

        let result = build_kiro_payload(&request, "conv-123", "profile-arn", &config);
        assert!(result.is_ok(), "Should handle compaction without error");

        let payload_result = result.unwrap();
        let payload = payload_result.payload;

        // Verify the important data is preserved somewhere in the payload
        let payload_str = payload.to_string();
        assert!(
            payload_str.contains(important_data),
            "Tool result content should be preserved as text: {}",
            payload_str
        );
    }

    #[test]
    fn test_compaction_multiple_tool_only_messages() {
        // Simulates the OpenCode compaction scenario where multiple
        // tool-only messages are sent without text content.
        let config = create_test_config();

        let request = ChatCompletionRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![
                ChatMessage {
                    role: "user".to_string(),
                    content: Some(json!("Help me with files")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                // Assistant with only tool_calls, no text
                ChatMessage {
                    role: "assistant".to_string(),
                    content: Some(json!("")),
                    name: None,
                    tool_calls: Some(vec![
                        ToolCall {
                            id: "call_1".to_string(),
                            tool_type: "function".to_string(),
                            function: FunctionCall {
                                name: "Read".to_string(),
                                arguments: r#"{"file_path": "a.txt"}"#.to_string(),
                            },
                        },
                        ToolCall {
                            id: "call_2".to_string(),
                            tool_type: "function".to_string(),
                            function: FunctionCall {
                                name: "Read".to_string(),
                                arguments: r#"{"file_path": "b.txt"}"#.to_string(),
                            },
                        },
                    ]),
                    tool_call_id: None,
                },
                // Tool results
                ChatMessage {
                    role: "tool".to_string(),
                    content: Some(json!("Content of a.txt")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: Some("call_1".to_string()),
                },
                ChatMessage {
                    role: "tool".to_string(),
                    content: Some(json!("Content of b.txt")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: Some("call_2".to_string()),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: Some(json!("Summarize")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            stream: false,
            temperature: None,
            top_p: None,
            n: None,
            max_tokens: None,
            max_completion_tokens: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
            tools: None, // NO TOOLS
            tool_choice: None,
            stream_options: None,
            logit_bias: None,
            logprobs: None,
            top_logprobs: None,
            user: None,
            seed: None,
            parallel_tool_calls: None,
        };

        let result = build_kiro_payload(&request, "conv-123", "profile-arn", &config);
        assert!(
            result.is_ok(),
            "Multiple tool-only messages should be handled"
        );

        let payload_result = result.unwrap();
        let payload = payload_result.payload;
        let payload_str = payload.to_string();

        // Verify both tool calls are converted to text
        assert!(
            payload_str.contains("[Tool: Read"),
            "Tool calls should be in text form"
        );
        assert!(
            payload_str.contains("a.txt"),
            "First file path should be preserved"
        );
        assert!(
            payload_str.contains("b.txt"),
            "Second file path should be preserved"
        );

        // Verify tool results are converted to text
        assert!(
            payload_str.contains("Content of a.txt"),
            "First tool result should be preserved"
        );
        assert!(
            payload_str.contains("Content of b.txt"),
            "Second tool result should be preserved"
        );
    }

    #[test]
    fn test_compaction_with_tools_keeps_tool_format() {
        // When tools ARE defined, tool content should remain in proper format (not converted to text)
        let config = create_test_config();

        let request = ChatCompletionRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![
                ChatMessage {
                    role: "user".to_string(),
                    content: Some(json!("Read file")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: "assistant".to_string(),
                    content: Some(json!("Reading...")),
                    name: None,
                    tool_calls: Some(vec![ToolCall {
                        id: "call_read".to_string(),
                        tool_type: "function".to_string(),
                        function: FunctionCall {
                            name: "Read".to_string(),
                            arguments: r#"{"file_path": "test.txt"}"#.to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
                ChatMessage {
                    role: "tool".to_string(),
                    content: Some(json!("File contents here")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: Some("call_read".to_string()),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: Some(json!("Thanks")),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            stream: false,
            temperature: None,
            top_p: None,
            n: None,
            max_tokens: None,
            max_completion_tokens: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
            tools: Some(vec![Tool {
                // Tools ARE defined
                tool_type: "function".to_string(),
                function: ToolFunction {
                    name: "Read".to_string(),
                    description: Some("Read a file".to_string()),
                    parameters: Some(json!({"type": "object"})),
                },
            }]),
            tool_choice: None,
            stream_options: None,
            logit_bias: None,
            logprobs: None,
            top_logprobs: None,
            user: None,
            seed: None,
            parallel_tool_calls: None,
        };

        let result = build_kiro_payload(&request, "conv-123", "profile-arn", &config);
        assert!(result.is_ok());

        let payload_result = result.unwrap();
        let payload = payload_result.payload;

        // When tools are defined, toolUses should be in proper Kiro format
        let history = payload["conversationState"]["history"].as_array().unwrap();

        let mut found_tool_uses = false;
        for msg in history {
            if let Some(assistant_msg) = msg.get("assistantResponseMessage") {
                if assistant_msg.get("toolUses").is_some() {
                    found_tool_uses = true;
                }
            }
        }
        assert!(
            found_tool_uses,
            "When tools are defined, toolUses should be in proper format"
        );
    }
}
