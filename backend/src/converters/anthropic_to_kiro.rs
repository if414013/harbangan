// Anthropic to Kiro converter
//
// This module converts Anthropic Messages API format to Kiro API format.
// It acts as an adapter layer that converts Anthropic-specific formats
// to the unified format used by the core converter.

use serde_json::Value;
use tracing::debug;

use crate::config::Config;
use crate::models::anthropic::{AnthropicMessage, AnthropicMessagesRequest, AnthropicTool};
use crate::resolver::normalize_model_name;

use super::core::{
    extract_images_from_content, extract_text_content, ContentBlock, MessageContent, ToolCall,
    ToolFunction, ToolResult, UnifiedMessage, UnifiedTool,
};
use super::openai_to_kiro::build_kiro_payload_core;

// ==================================================================================================
// Anthropic-specific Content Processing
// ==================================================================================================

/// Converts Anthropic content to MessageContent.
///
/// Anthropic content can be:
/// - String: "Hello, world!"
/// - List of content blocks: [{"type": "text", "text": "Hello"}]
fn convert_anthropic_content(content: &Value) -> MessageContent {
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
                    "image" => {
                        let source = block.get("source")?;
                        let source_type = source.get("type")?.as_str()?.to_string();
                        let media_type = source
                            .get("media_type")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        let data = source
                            .get("data")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        let url = source.get("url").and_then(|v| v.as_str()).map(String::from);

                        Some(ContentBlock::Image {
                            source: super::core::ImageSource {
                                source_type,
                                media_type,
                                data,
                                url,
                            },
                        })
                    }
                    "tool_result" => {
                        let tool_use_id = block.get("tool_use_id")?.as_str()?.to_string();
                        let result_content = block.get("content")?;
                        let content_text = if result_content.is_string() {
                            result_content.as_str()?.to_string()
                        } else {
                            // Extract text from content blocks
                            extract_text_content(&convert_anthropic_content(result_content))
                        };
                        Some(ContentBlock::ToolResult {
                            tool_use_id,
                            content: if content_text.is_empty() {
                                "(empty result)".to_string()
                            } else {
                                content_text
                            },
                        })
                    }
                    "tool_use" => {
                        let id = block.get("id")?.as_str()?.to_string();
                        let name = block.get("name")?.as_str()?.to_string();
                        let input = block.get("input")?.clone();
                        Some(ContentBlock::ToolUse { id, name, input })
                    }
                    _ => None,
                }
            })
            .collect();

        return MessageContent::Blocks(content_blocks);
    }

    MessageContent::Text(content.to_string())
}

/// Extracts system prompt text from Anthropic system field.
///
/// Anthropic API supports system in two formats:
/// 1. String: "You are helpful"
/// 2. List of content blocks: [{"type": "text", "text": "...", "cache_control": {...}}]
fn extract_system_prompt(system: &Option<Value>) -> String {
    let Some(system) = system else {
        return String::new();
    };

    if let Some(text) = system.as_str() {
        return text.to_string();
    }

    if let Some(blocks) = system.as_array() {
        let text_parts: Vec<String> = blocks
            .iter()
            .filter_map(|block| {
                if block.get("type")?.as_str()? == "text" {
                    Some(block.get("text")?.as_str()?.to_string())
                } else {
                    None
                }
            })
            .collect();
        return text_parts.join("\n");
    }

    system.to_string()
}

/// Extracts tool results from Anthropic message content.
fn extract_tool_results_from_anthropic_content(content: &MessageContent) -> Vec<ToolResult> {
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
                    content: content.clone(),
                });
            }
        }
    }

    tool_results
}

/// Extracts tool uses from Anthropic assistant message content.
fn extract_tool_uses_from_anthropic_content(content: &MessageContent) -> Vec<ToolCall> {
    let mut tool_calls = Vec::new();

    if let MessageContent::Blocks(blocks) = content {
        for block in blocks {
            if let ContentBlock::ToolUse { id, name, input } = block {
                // Convert input to JSON string for unified format
                let arguments = serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string());

                tool_calls.push(ToolCall {
                    id: id.clone(),
                    call_type: "function".to_string(),
                    function: ToolFunction {
                        name: name.clone(),
                        arguments,
                    },
                });
            }
        }
    }

    tool_calls
}

// ==================================================================================================
// Message and Tool Conversion
// ==================================================================================================

/// Converts Anthropic messages to unified format.
pub fn convert_anthropic_messages(messages: &[AnthropicMessage]) -> Vec<UnifiedMessage> {
    let mut unified_messages = Vec::new();
    let mut total_tool_calls = 0;
    let mut total_tool_results = 0;
    let mut total_images = 0;

    for msg in messages {
        let content = convert_anthropic_content(&msg.content);

        // Extract tool-related data and images based on role
        let (tool_calls, tool_results, images) = match msg.role.as_str() {
            "assistant" => {
                // Assistant messages may contain tool_use blocks
                let calls = extract_tool_uses_from_anthropic_content(&content);
                if !calls.is_empty() {
                    total_tool_calls += calls.len();
                    (Some(calls), None, None)
                } else {
                    (None, None, None)
                }
            }
            "user" => {
                // User messages may contain tool_result blocks and images
                let results = extract_tool_results_from_anthropic_content(&content);
                let imgs = extract_images_from_content(&content);

                if !results.is_empty() {
                    total_tool_results += results.len();
                }
                if !imgs.is_empty() {
                    total_images += imgs.len();
                }

                (
                    None,
                    if results.is_empty() {
                        None
                    } else {
                        Some(results)
                    },
                    if imgs.is_empty() { None } else { Some(imgs) },
                )
            }
            _ => (None, None, None),
        };

        unified_messages.push(UnifiedMessage {
            role: msg.role.clone(),
            content,
            tool_calls,
            tool_results,
            images,
        });
    }

    // Log summary if any tool content or images were found
    if total_tool_calls > 0 || total_tool_results > 0 || total_images > 0 {
        debug!(
            "Converted {} Anthropic messages: {} tool_calls, {} tool_results, {} images",
            messages.len(),
            total_tool_calls,
            total_tool_results,
            total_images
        );
    }

    unified_messages
}

/// Converts Anthropic tools to unified format.
pub fn convert_anthropic_tools(tools: &Option<Vec<AnthropicTool>>) -> Option<Vec<UnifiedTool>> {
    tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|tool| UnifiedTool {
                name: tool.name.clone(),
                description: tool.description.clone(),
                input_schema: Some(tool.input_schema.clone()),
            })
            .collect()
    })
}

// ==================================================================================================
// Main Entry Point
// ==================================================================================================

/// Converts Anthropic Messages API request to Kiro API payload.
///
/// This is the main entry point for Anthropic → Kiro conversion.
///
/// Key differences from OpenAI:
/// - System prompt is a separate field (not in messages)
/// - Content can be string or list of content blocks
/// - Tool format uses input_schema instead of parameters
pub fn build_kiro_payload(
    request: &AnthropicMessagesRequest,
    conversation_id: &str,
    profile_arn: &str,
    config: &Config,
) -> Result<super::core::KiroPayloadResult, String> {
    // Convert messages to unified format
    let unified_messages = convert_anthropic_messages(&request.messages);

    // Convert tools to unified format
    let unified_tools = convert_anthropic_tools(&request.tools);

    // System prompt is already separate in Anthropic format
    let system_prompt = extract_system_prompt(&request.system);

    // Normalize model name
    let model_id = normalize_model_name(&request.model);

    debug!(
        "Converting Anthropic request: model={} -> {}, messages={}, tools={}, system_prompt_length={}",
        request.model,
        model_id,
        unified_messages.len(),
        unified_tools.as_ref().map_or(0, |t| t.len()),
        system_prompt.len()
    );

    // Use core function to build payload
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ==================================================================================================
    // extract_system_prompt tests
    // ==================================================================================================

    #[test]
    fn test_extract_system_prompt_string() {
        let system = Some(json!("You are a helpful assistant."));
        let result = extract_system_prompt(&system);
        assert_eq!(result, "You are a helpful assistant.");
    }

    #[test]
    fn test_extract_system_prompt_blocks() {
        let system = Some(json!([
            {"type": "text", "text": "You are helpful."},
            {"type": "text", "text": "Be concise."}
        ]));
        let result = extract_system_prompt(&system);
        assert_eq!(result, "You are helpful.\nBe concise.");
    }

    #[test]
    fn test_extract_system_prompt_none() {
        let result = extract_system_prompt(&None);
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_system_prompt_blocks_with_cache_control() {
        let system = Some(json!([
            {"type": "text", "text": "System prompt", "cache_control": {"type": "ephemeral"}}
        ]));
        let result = extract_system_prompt(&system);
        assert_eq!(result, "System prompt");
    }

    #[test]
    fn test_extract_system_prompt_non_text_blocks() {
        let system = Some(json!([
            {"type": "image", "source": {}},
            {"type": "text", "text": "Only text"}
        ]));
        let result = extract_system_prompt(&system);
        assert_eq!(result, "Only text");
    }

    // ==================================================================================================
    // convert_anthropic_content tests
    // ==================================================================================================

    #[test]
    fn test_convert_anthropic_content_string() {
        let content = json!("Hello, world!");
        let result = convert_anthropic_content(&content);
        match result {
            MessageContent::Text(text) => assert_eq!(text, "Hello, world!"),
            _ => panic!("Expected Text variant"),
        }
    }

    #[test]
    fn test_convert_anthropic_content_text_blocks() {
        let content = json!([
            {"type": "text", "text": "Hello"},
            {"type": "text", "text": "World"}
        ]);
        let result = convert_anthropic_content(&content);
        match result {
            MessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 2);
                match &blocks[0] {
                    ContentBlock::Text { text } => assert_eq!(text, "Hello"),
                    _ => panic!("Expected Text block"),
                }
            }
            _ => panic!("Expected Blocks variant"),
        }
    }

    #[test]
    fn test_convert_anthropic_content_image_block() {
        let content = json!([
            {
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/png",
                    "data": "iVBORw0KGgo="
                }
            }
        ]);
        let result = convert_anthropic_content(&content);
        match result {
            MessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    ContentBlock::Image { source } => {
                        assert_eq!(source.source_type, "base64");
                        assert_eq!(source.media_type, Some("image/png".to_string()));
                        assert_eq!(source.data, Some("iVBORw0KGgo=".to_string()));
                    }
                    _ => panic!("Expected Image block"),
                }
            }
            _ => panic!("Expected Blocks variant"),
        }
    }

    #[test]
    fn test_convert_anthropic_content_image_url() {
        let content = json!([
            {
                "type": "image",
                "source": {
                    "type": "url",
                    "url": "https://example.com/image.png"
                }
            }
        ]);
        let result = convert_anthropic_content(&content);
        match result {
            MessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    ContentBlock::Image { source } => {
                        assert_eq!(source.source_type, "url");
                        assert_eq!(
                            source.url,
                            Some("https://example.com/image.png".to_string())
                        );
                    }
                    _ => panic!("Expected Image block"),
                }
            }
            _ => panic!("Expected Blocks variant"),
        }
    }

    #[test]
    fn test_convert_anthropic_content_tool_use() {
        let content = json!([
            {
                "type": "tool_use",
                "id": "tool_123",
                "name": "get_weather",
                "input": {"location": "Seattle"}
            }
        ]);
        let result = convert_anthropic_content(&content);
        match result {
            MessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    ContentBlock::ToolUse { id, name, input } => {
                        assert_eq!(id, "tool_123");
                        assert_eq!(name, "get_weather");
                        assert_eq!(input["location"], "Seattle");
                    }
                    _ => panic!("Expected ToolUse block"),
                }
            }
            _ => panic!("Expected Blocks variant"),
        }
    }

    #[test]
    fn test_convert_anthropic_content_tool_result_string() {
        let content = json!([
            {
                "type": "tool_result",
                "tool_use_id": "tool_123",
                "content": "The weather is sunny"
            }
        ]);
        let result = convert_anthropic_content(&content);
        match result {
            MessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                    } => {
                        assert_eq!(tool_use_id, "tool_123");
                        assert_eq!(content, "The weather is sunny");
                    }
                    _ => panic!("Expected ToolResult block"),
                }
            }
            _ => panic!("Expected Blocks variant"),
        }
    }

    #[test]
    fn test_convert_anthropic_content_tool_result_blocks() {
        let content = json!([
            {
                "type": "tool_result",
                "tool_use_id": "tool_456",
                "content": [{"type": "text", "text": "Result text"}]
            }
        ]);
        let result = convert_anthropic_content(&content);
        match result {
            MessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                    } => {
                        assert_eq!(tool_use_id, "tool_456");
                        assert_eq!(content, "Result text");
                    }
                    _ => panic!("Expected ToolResult block"),
                }
            }
            _ => panic!("Expected Blocks variant"),
        }
    }

    #[test]
    fn test_convert_anthropic_content_tool_result_empty() {
        let content = json!([
            {
                "type": "tool_result",
                "tool_use_id": "tool_789",
                "content": ""
            }
        ]);
        let result = convert_anthropic_content(&content);
        match result {
            MessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    ContentBlock::ToolResult { content, .. } => {
                        assert_eq!(content, "(empty result)");
                    }
                    _ => panic!("Expected ToolResult block"),
                }
            }
            _ => panic!("Expected Blocks variant"),
        }
    }

    #[test]
    fn test_convert_anthropic_content_unknown_type() {
        let content = json!([
            {"type": "unknown", "data": "something"},
            {"type": "text", "text": "Valid text"}
        ]);
        let result = convert_anthropic_content(&content);
        match result {
            MessageContent::Blocks(blocks) => {
                // Unknown type should be filtered out
                assert_eq!(blocks.len(), 1);
            }
            _ => panic!("Expected Blocks variant"),
        }
    }

    #[test]
    fn test_convert_anthropic_content_fallback() {
        let content = json!({"some": "object"});
        let result = convert_anthropic_content(&content);
        match result {
            MessageContent::Text(text) => {
                assert!(text.contains("some"));
            }
            _ => panic!("Expected Text variant for fallback"),
        }
    }

    // ==================================================================================================
    // extract_tool_results_from_anthropic_content tests
    // ==================================================================================================

    #[test]
    fn test_extract_tool_results_from_blocks() {
        let content = MessageContent::Blocks(vec![
            ContentBlock::ToolResult {
                tool_use_id: "tool_1".to_string(),
                content: "Result 1".to_string(),
            },
            ContentBlock::Text {
                text: "Some text".to_string(),
            },
            ContentBlock::ToolResult {
                tool_use_id: "tool_2".to_string(),
                content: "Result 2".to_string(),
            },
        ]);
        let results = extract_tool_results_from_anthropic_content(&content);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].tool_use_id, "tool_1");
        assert_eq!(results[1].tool_use_id, "tool_2");
    }

    #[test]
    fn test_extract_tool_results_from_text() {
        let content = MessageContent::Text("Just text".to_string());
        let results = extract_tool_results_from_anthropic_content(&content);
        assert!(results.is_empty());
    }

    // ==================================================================================================
    // extract_tool_uses_from_anthropic_content tests
    // ==================================================================================================

    #[test]
    fn test_extract_tool_uses_from_blocks() {
        let content = MessageContent::Blocks(vec![
            ContentBlock::Text {
                text: "Let me check".to_string(),
            },
            ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "get_weather".to_string(),
                input: json!({"location": "NYC"}),
            },
        ]);
        let calls = extract_tool_uses_from_anthropic_content(&content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].function.name, "get_weather");
        assert!(calls[0].function.arguments.contains("NYC"));
    }

    #[test]
    fn test_extract_tool_uses_from_text() {
        let content = MessageContent::Text("No tools here".to_string());
        let calls = extract_tool_uses_from_anthropic_content(&content);
        assert!(calls.is_empty());
    }

    // ==================================================================================================
    // convert_anthropic_messages tests
    // ==================================================================================================

    #[test]
    fn test_convert_anthropic_messages_basic() {
        let messages = vec![
            AnthropicMessage {
                role: "user".to_string(),
                content: json!("Hello!"),
            },
            AnthropicMessage {
                role: "assistant".to_string(),
                content: json!("Hi there!"),
            },
        ];

        let unified = convert_anthropic_messages(&messages);
        assert_eq!(unified.len(), 2);
        assert_eq!(unified[0].role, "user");
        assert_eq!(unified[1].role, "assistant");
    }

    #[test]
    fn test_convert_anthropic_messages_with_tool_use() {
        let messages = vec![AnthropicMessage {
            role: "assistant".to_string(),
            content: json!([
                {"type": "text", "text": "Let me check the weather."},
                {"type": "tool_use", "id": "call_1", "name": "get_weather", "input": {"city": "Seattle"}}
            ]),
        }];

        let unified = convert_anthropic_messages(&messages);
        assert_eq!(unified.len(), 1);
        assert!(unified[0].tool_calls.is_some());
        let calls = unified[0].tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "get_weather");
    }

    #[test]
    fn test_convert_anthropic_messages_with_tool_result() {
        let messages = vec![AnthropicMessage {
            role: "user".to_string(),
            content: json!([
                {"type": "tool_result", "tool_use_id": "call_1", "content": "72°F and sunny"}
            ]),
        }];

        let unified = convert_anthropic_messages(&messages);
        assert_eq!(unified.len(), 1);
        assert!(unified[0].tool_results.is_some());
        let results = unified[0].tool_results.as_ref().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "72°F and sunny");
    }

    #[test]
    fn test_convert_anthropic_messages_with_images() {
        let messages = vec![AnthropicMessage {
            role: "user".to_string(),
            content: json!([
                {"type": "text", "text": "What's in this image?"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": "abc123"}}
            ]),
        }];

        let unified = convert_anthropic_messages(&messages);
        assert_eq!(unified.len(), 1);
        assert!(unified[0].images.is_some());
        let images = unified[0].images.as_ref().unwrap();
        assert_eq!(images.len(), 1);
    }

    #[test]
    fn test_convert_anthropic_messages_unknown_role() {
        let messages = vec![AnthropicMessage {
            role: "system".to_string(), // Not user or assistant
            content: json!("System message"),
        }];

        let unified = convert_anthropic_messages(&messages);
        assert_eq!(unified.len(), 1);
        assert!(unified[0].tool_calls.is_none());
        assert!(unified[0].tool_results.is_none());
    }

    // ==================================================================================================
    // convert_anthropic_tools tests
    // ==================================================================================================

    #[test]
    fn test_convert_anthropic_tools() {
        let tools = vec![AnthropicTool {
            name: "get_weather".to_string(),
            description: Some("Get weather".to_string()),
            input_schema: json!({"type": "object"}),
        }];

        let unified = convert_anthropic_tools(&Some(tools));
        assert!(unified.is_some());
        let tools = unified.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "get_weather");
    }

    #[test]
    fn test_convert_anthropic_tools_none() {
        let unified = convert_anthropic_tools(&None);
        assert!(unified.is_none());
    }

    #[test]
    fn test_convert_anthropic_tools_empty() {
        let unified = convert_anthropic_tools(&Some(vec![]));
        assert!(unified.is_some());
        assert!(unified.unwrap().is_empty());
    }

    #[test]
    fn test_convert_anthropic_tools_multiple() {
        let tools = vec![
            AnthropicTool {
                name: "tool_a".to_string(),
                description: Some("Tool A".to_string()),
                input_schema: json!({"type": "object", "properties": {"x": {"type": "string"}}}),
            },
            AnthropicTool {
                name: "tool_b".to_string(),
                description: None,
                input_schema: json!({"type": "object"}),
            },
        ];

        let unified = convert_anthropic_tools(&Some(tools));
        assert!(unified.is_some());
        let tools = unified.unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "tool_a");
        assert!(tools[0].description.is_some());
        assert_eq!(tools[1].name, "tool_b");
        assert!(tools[1].description.is_none());
    }
}
