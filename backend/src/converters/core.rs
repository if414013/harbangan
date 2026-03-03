// Core converter types and utilities
//
// This module contains shared logic used by all converters:
// - Unified message and tool formats (API-agnostic)
// - Text content extraction
// - Image processing
// - Tool processing and sanitization
// - Message merging
// - Kiro history building

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::{debug, warn};

use crate::config::Config;

// ==================================================================================================
// Unified Data Types
// ==================================================================================================

/// Unified message format used internally by converters.
///
/// This format is API-agnostic and can be created from both OpenAI and Anthropic formats.
#[derive(Debug, Clone)]
pub struct UnifiedMessage {
    pub role: String,
    pub content: MessageContent,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_results: Option<Vec<ToolResult>>,
    pub images: Option<Vec<UnifiedImage>>,
}

/// Message content can be either text or structured content blocks
#[derive(Debug, Clone)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

/// Content block for structured messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        source: ImageSource,
    },
    ImageUrl {
        image_url: ImageUrl,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: Option<String>,
    pub data: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
}

/// Tool call in unified format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub arguments: String,
}

/// Tool result in unified format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    #[serde(rename = "type")]
    pub result_type: String,
    pub tool_use_id: String,
    pub content: String,
}

/// Unified image format
#[derive(Debug, Clone)]
pub struct UnifiedImage {
    pub media_type: String,
    pub data: String,
}

/// Unified tool format
#[derive(Debug, Clone)]
pub struct UnifiedTool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<Value>,
}

/// Result of building Kiro payload
#[derive(Debug)]
#[allow(dead_code)]
pub struct KiroPayloadResult {
    pub payload: Value,
    pub tool_documentation: String,
}

// ==================================================================================================
// Text Content Extraction
// ==================================================================================================

/// Extracts text content from various formats.
///
/// Supports:
/// - String: "Hello, world!"
/// - List of content blocks: [{"type": "text", "text": "Hello"}]
/// - Empty/None: returns empty string
pub fn extract_text_content(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::Blocks(blocks) => {
            let mut text_parts = Vec::new();
            for block in blocks {
                match block {
                    ContentBlock::Text { text } => text_parts.push(text.clone()),
                    ContentBlock::ToolResult { content, .. } => text_parts.push(content.clone()),
                    // Skip images - they're handled separately
                    _ => {}
                }
            }
            text_parts.join("")
        }
    }
}

/// Extracts images from message content in unified format.
///
/// Supports multiple image formats:
/// - OpenAI: {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,..."}}
/// - Anthropic: {"type": "image", "source": {"type": "base64", "media_type": "...", "data": "..."}}
pub fn extract_images_from_content(content: &MessageContent) -> Vec<UnifiedImage> {
    let mut images = Vec::new();

    if let MessageContent::Blocks(blocks) = content {
        for block in blocks {
            match block {
                // OpenAI format
                ContentBlock::ImageUrl { image_url } => {
                    if let Some((media_type, data)) = parse_data_url(&image_url.url) {
                        if !data.is_empty() {
                            images.push(UnifiedImage { media_type, data });
                        }
                    } else if image_url.url.starts_with("http") {
                        warn!(
                            "URL-based images are not supported by Kiro API, skipping: {}...",
                            &image_url.url[..80.min(image_url.url.len())]
                        );
                    } else if !image_url.url.is_empty() {
                        // Assume raw base64 data without data URL prefix
                        warn!(
                            "Image URL missing data: prefix, assuming raw base64 with image/jpeg"
                        );
                        images.push(UnifiedImage {
                            media_type: "image/jpeg".to_string(),
                            data: image_url.url.clone(),
                        });
                    }
                }
                // Anthropic format
                ContentBlock::Image { source } => {
                    if source.source_type == "base64" {
                        if let (Some(data), media_type) = (&source.data, &source.media_type) {
                            if !data.is_empty() {
                                images.push(UnifiedImage {
                                    media_type: media_type
                                        .clone()
                                        .unwrap_or_else(|| "image/jpeg".to_string()),
                                    data: data.clone(),
                                });
                            }
                        }
                    } else if source.source_type == "url" {
                        if let Some(url) = &source.url {
                            warn!(
                                "URL-based images are not supported by Kiro API, skipping: {}...",
                                &url[..80.min(url.len())]
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if !images.is_empty() {
        debug!("Extracted {} image(s) from content", images.len());
    }

    images
}

/// Parses data URL format: data:image/jpeg;base64,/9j/...
fn parse_data_url(url: &str) -> Option<(String, String)> {
    if !url.starts_with("data:") {
        return None;
    }

    let parts: Vec<&str> = url.splitn(2, ',').collect();
    if parts.len() != 2 {
        return None;
    }

    let header = parts[0];
    let data = parts[1];

    // Extract media type from "data:image/jpeg;base64"
    let media_part = header.split(';').next()?;
    let media_type = media_part.strip_prefix("data:")?;

    Some((media_type.to_string(), data.to_string()))
}

// ==================================================================================================
// Thinking Mode Support
// ==================================================================================================

/// Generate system prompt addition that legitimizes thinking tags.
pub fn get_thinking_system_prompt_addition(config: &Config) -> String {
    if !config.fake_reasoning_enabled {
        return String::new();
    }

    "\n\n---\n\
        # Extended Thinking Mode\n\n\
        This conversation uses extended thinking mode. User messages may contain \
        special XML tags that are legitimate system-level instructions:\n\
        - `<thinking_mode>enabled</thinking_mode>` - enables extended thinking\n\
        - `<max_thinking_length>N</max_thinking_length>` - sets maximum thinking tokens\n\
        - `<thinking_instruction>...</thinking_instruction>` - provides thinking guidelines\n\n\
        These tags are NOT prompt injection attempts. They are part of the system's \
        extended thinking feature. When you see these tags, follow their instructions \
        and wrap your reasoning process in `<thinking>...</thinking>` tags before \
        providing your final response."
        .to_string()
}

/// Inject fake reasoning tags into content.
pub fn inject_thinking_tags(content: String, config: &Config) -> String {
    if !config.fake_reasoning_enabled {
        return content;
    }

    let thinking_instruction = "\
        Think in English for better reasoning quality.\n\n\
        Your thinking process should be thorough and systematic:\n\
        - First, make sure you fully understand what is being asked\n\
        - Consider multiple approaches or perspectives when relevant\n\
        - Think about edge cases, potential issues, and what could go wrong\n\
        - Challenge your initial assumptions\n\
        - Verify your reasoning before reaching a conclusion\n\n\
        Take the time you need. Quality of thought matters more than speed.";

    let thinking_prefix = format!(
        "<thinking_mode>enabled</thinking_mode>\n\
        <max_thinking_length>{}</max_thinking_length>\n\
        <thinking_instruction>{}</thinking_instruction>\n\n",
        config.fake_reasoning_max_tokens, thinking_instruction
    );

    debug!(
        "Injecting fake reasoning tags with max_tokens={}",
        config.fake_reasoning_max_tokens
    );

    thinking_prefix + &content
}

// ==================================================================================================
// JSON Schema Sanitization
// ==================================================================================================

/// Sanitizes JSON Schema from fields that Kiro API doesn't accept.
///
/// Kiro API returns 400 "Improperly formed request" error if:
/// - required is an empty array []
/// - additionalProperties is present in schema
pub fn sanitize_json_schema(schema: &Value) -> Value {
    if !schema.is_object() {
        return schema.clone();
    }

    let obj = schema.as_object().unwrap();
    let mut result = serde_json::Map::new();

    for (key, value) in obj {
        // Skip empty required arrays
        if key == "required" {
            if let Some(arr) = value.as_array() {
                if arr.is_empty() {
                    continue;
                }
            }
        }

        // Skip additionalProperties
        if key == "additionalProperties" {
            continue;
        }

        // Recursively process nested objects
        if key == "properties" && value.is_object() {
            let props = value.as_object().unwrap();
            let mut sanitized_props = serde_json::Map::new();
            for (prop_name, prop_value) in props {
                sanitized_props.insert(prop_name.clone(), sanitize_json_schema(prop_value));
            }
            result.insert(key.clone(), Value::Object(sanitized_props));
        } else if value.is_object() {
            result.insert(key.clone(), sanitize_json_schema(value));
        } else if value.is_array() {
            let arr = value.as_array().unwrap();
            let sanitized_arr: Vec<Value> = arr
                .iter()
                .map(|item| {
                    if item.is_object() {
                        sanitize_json_schema(item)
                    } else {
                        item.clone()
                    }
                })
                .collect();
            result.insert(key.clone(), Value::Array(sanitized_arr));
        } else {
            result.insert(key.clone(), value.clone());
        }
    }

    Value::Object(result)
}

// ==================================================================================================
// Tool Processing
// ==================================================================================================

/// Processes tools with long descriptions.
///
/// If description exceeds the limit, full description is moved to system prompt,
/// and a reference remains in the tool.
pub fn process_tools_with_long_descriptions(
    tools: Option<Vec<UnifiedTool>>,
    config: &Config,
) -> (Option<Vec<UnifiedTool>>, String) {
    let Some(tools) = tools else {
        return (None, String::new());
    };

    // If limit is disabled (0), return tools unchanged
    if config.tool_description_max_length == 0 {
        return (Some(tools), String::new());
    }

    let mut tool_documentation_parts = Vec::new();
    let mut processed_tools = Vec::new();

    for tool in tools {
        let description = tool.description.as_deref().unwrap_or("");

        if description.len() <= config.tool_description_max_length {
            // Description is short - leave as is
            processed_tools.push(tool);
        } else {
            // Description is too long - move to system prompt
            debug!(
                "Tool '{}' has long description ({} chars > {}), moving to system prompt",
                tool.name,
                description.len(),
                config.tool_description_max_length
            );

            tool_documentation_parts.push(format!("## Tool: {}\n\n{}", tool.name, description));

            // Create copy with reference description
            let reference_description = format!(
                "[Full documentation in system prompt under '## Tool: {}']",
                tool.name
            );

            processed_tools.push(UnifiedTool {
                name: tool.name,
                description: Some(reference_description),
                input_schema: tool.input_schema,
            });
        }
    }

    let tool_documentation = if tool_documentation_parts.is_empty() {
        String::new()
    } else {
        format!(
            "\n\n---\n# Tool Documentation\n\
            The following tools have detailed documentation that couldn't fit in the tool definition.\n\n{}",
            tool_documentation_parts.join("\n\n---\n\n")
        )
    };

    let result_tools = if processed_tools.is_empty() {
        None
    } else {
        Some(processed_tools)
    };

    (result_tools, tool_documentation)
}

/// Converts unified tools to Kiro API format.
pub fn convert_tools_to_kiro_format(tools: &Option<Vec<UnifiedTool>>) -> Vec<Value> {
    let Some(tools) = tools else {
        return Vec::new();
    };

    let mut kiro_tools = Vec::new();
    for tool in tools {
        // Sanitize parameters
        let sanitized_params = tool
            .input_schema
            .as_ref()
            .map(sanitize_json_schema)
            .unwrap_or(json!({}));

        // Kiro API requires non-empty description
        let description = tool
            .description
            .as_ref()
            .filter(|d| !d.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| {
                debug!(
                    "Tool '{}' has empty description, using placeholder",
                    tool.name
                );
                format!("Tool: {}", tool.name)
            });

        kiro_tools.push(json!({
            "toolSpecification": {
                "name": tool.name,
                "description": description,
                "inputSchema": {"json": sanitized_params}
            }
        }));
    }

    kiro_tools
}

// ==================================================================================================
// Image Conversion to Kiro Format
// ==================================================================================================

/// Converts unified images to Kiro API format.
///
/// Unified: [{"media_type": "image/jpeg", "data": "base64..."}]
/// Kiro: [{"format": "jpeg", "source": {"bytes": "base64..."}}]
pub fn convert_images_to_kiro_format(images: &Option<Vec<UnifiedImage>>) -> Vec<Value> {
    let Some(images) = images else {
        return Vec::new();
    };

    let mut kiro_images = Vec::new();
    for img in images {
        let mut data = img.data.clone();
        let mut media_type = img.media_type.clone();

        // Strip data URL prefix if present
        if data.starts_with("data:") {
            if let Some((extracted_media, extracted_data)) = parse_data_url(&data) {
                media_type = extracted_media;
                data = extracted_data;
                debug!(
                    "Stripped data URL prefix, extracted media_type: {}",
                    media_type
                );
            }
        }

        if data.is_empty() {
            warn!("Skipping image with empty data");
            continue;
        }

        // Extract format from media_type: "image/jpeg" -> "jpeg"
        let format_str = media_type
            .split('/')
            .next_back()
            .unwrap_or(&media_type)
            .to_string();

        kiro_images.push(json!({
            "format": format_str,
            "source": {
                "bytes": data
            }
        }));
    }

    if !kiro_images.is_empty() {
        debug!("Converted {} image(s) to Kiro format", kiro_images.len());
    }

    kiro_images
}

// ==================================================================================================
// Tool Results and Tool Uses
// ==================================================================================================

/// Converts unified tool results to Kiro API format.
pub fn convert_tool_results_to_kiro_format(tool_results: &[ToolResult]) -> Vec<Value> {
    tool_results
        .iter()
        .map(|tr| {
            let content_text = if tr.content.is_empty() {
                "(empty result)"
            } else {
                &tr.content
            };

            json!({
                "content": [{"text": content_text}],
                "status": "success",
                "toolUseId": tr.tool_use_id
            })
        })
        .collect()
}

/// Extracts tool results from message content and converts to Kiro API format.
///
/// Looks for content blocks with type="tool_result" and converts them
/// to Kiro API format. This is used as a fallback when tool_results
/// are embedded in content blocks rather than in the tool_results field.
pub fn extract_tool_results_from_content(content: &MessageContent) -> Vec<Value> {
    let mut tool_results = Vec::new();

    if let MessageContent::Blocks(blocks) = content {
        for block in blocks {
            if let ContentBlock::ToolResult {
                tool_use_id,
                content,
            } = block
            {
                let content_text = if content.is_empty() {
                    "(empty result)"
                } else {
                    content
                };

                tool_results.push(json!({
                    "content": [{"text": content_text}],
                    "status": "success",
                    "toolUseId": tool_use_id
                }));
            }
        }
    }

    tool_results
}

/// Extracts tool uses from assistant message.
///
/// This function also deduplicates tool uses by ID, keeping the one with more
/// content in the input field. This handles cases where the Kiro API sends
/// duplicate tool calls (one with arguments, one empty).
pub fn extract_tool_uses_from_message(
    content: &MessageContent,
    tool_calls: &Option<Vec<ToolCall>>,
) -> Vec<Value> {
    let mut tool_uses = Vec::new();

    // From tool_calls field
    if let Some(calls) = tool_calls {
        for tc in calls {
            let input_data: Value =
                serde_json::from_str(&tc.function.arguments).unwrap_or(json!({}));
            tool_uses.push(json!({
                "name": tc.function.name,
                "input": input_data,
                "toolUseId": tc.id
            }));
        }
    }

    // From content blocks (Anthropic format)
    if let MessageContent::Blocks(blocks) = content {
        for block in blocks {
            if let ContentBlock::ToolUse { id, name, input } = block {
                tool_uses.push(json!({
                    "name": name,
                    "input": input,
                    "toolUseId": id
                }));
            }
        }
    }

    // Deduplicate by toolUseId, keeping the one with more content
    deduplicate_tool_uses_json(tool_uses)
}

/// Deduplicates tool uses by toolUseId, keeping the one with more content.
///
/// This handles the case where Kiro API sends duplicate tool calls - one with
/// proper arguments and one with empty arguments.
fn deduplicate_tool_uses_json(tool_uses: Vec<Value>) -> Vec<Value> {
    use std::collections::HashMap;

    if tool_uses.len() <= 1 {
        return tool_uses;
    }

    let original_count = tool_uses.len();
    let mut by_id: HashMap<String, Value> = HashMap::new();

    for tool_use in tool_uses {
        let id = tool_use
            .get("toolUseId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if id.is_empty() {
            // No ID, can't deduplicate - keep it
            by_id.insert(format!("__no_id_{}", by_id.len()), tool_use);
            continue;
        }

        // Calculate "content size" - prefer the one with more input data
        let input_size = tool_use
            .get("input")
            .map(|v| v.to_string().len())
            .unwrap_or(0);

        if let Some(existing) = by_id.get(&id) {
            let existing_size = existing
                .get("input")
                .map(|v| v.to_string().len())
                .unwrap_or(0);

            // Keep the one with more content
            if input_size > existing_size {
                by_id.insert(id, tool_use);
            }
        } else {
            by_id.insert(id, tool_use);
        }
    }

    let unique: Vec<Value> = by_id.into_values().collect();

    if unique.len() != original_count {
        tracing::debug!(
            "Deduplicated tool uses in conversation history: {} -> {}",
            original_count,
            unique.len()
        );
    }

    unique
}

// ==================================================================================================
// Tool Content to Text Conversion (for stripping when no tools defined)
// ==================================================================================================

/// Converts tool_calls to human-readable text representation.
///
/// This is used when stripping tool content from messages (when no tools are defined).
/// Instead of losing the context, we convert tool calls to text so the model
/// can still understand what happened in the conversation.
pub fn tool_calls_to_text(tool_calls: &[ToolCall]) -> String {
    if tool_calls.is_empty() {
        return String::new();
    }

    let parts: Vec<String> = tool_calls
        .iter()
        .map(|tc| {
            let name = &tc.function.name;
            let arguments = &tc.function.arguments;
            let tool_id = &tc.id;

            if tool_id.is_empty() {
                format!("[Tool: {}]\n{}", name, arguments)
            } else {
                format!("[Tool: {} ({})]\n{}", name, tool_id, arguments)
            }
        })
        .collect();

    parts.join("\n\n")
}

/// Converts tool_results to human-readable text representation.
///
/// This is used when stripping tool content from messages (when no tools are defined).
/// Instead of losing the context, we convert tool results to text so the model
/// can still understand what happened in the conversation.
pub fn tool_results_to_text(tool_results: &[ToolResult]) -> String {
    if tool_results.is_empty() {
        return String::new();
    }

    let parts: Vec<String> = tool_results
        .iter()
        .map(|tr| {
            let content = if tr.content.is_empty() {
                "(empty result)"
            } else {
                &tr.content
            };
            let tool_use_id = &tr.tool_use_id;

            if tool_use_id.is_empty() {
                format!("[Tool Result]\n{}", content)
            } else {
                format!("[Tool Result ({})]\n{}", tool_use_id, content)
            }
        })
        .collect();

    parts.join("\n\n")
}

/// Strips ALL tool-related content from messages, converting it to text representation.
///
/// This is used when no tools are defined in the request. Kiro API rejects
/// requests that have toolResults but no tools defined.
///
/// Instead of simply removing tool content, this function converts tool_calls
/// and tool_results to human-readable text, preserving the context for
/// summarization and other use cases.
///
/// Returns a tuple of (processed messages, whether any tool content was converted).
pub fn strip_all_tool_content(messages: Vec<UnifiedMessage>) -> (Vec<UnifiedMessage>, bool) {
    if messages.is_empty() {
        return (Vec::new(), false);
    }

    let mut result = Vec::new();
    let mut total_tool_calls_stripped = 0;
    let mut total_tool_results_stripped = 0;

    for msg in messages {
        let has_tool_calls = msg.tool_calls.as_ref().is_some_and(|tc| !tc.is_empty());
        let has_tool_results = msg.tool_results.as_ref().is_some_and(|tr| !tr.is_empty());

        if has_tool_calls || has_tool_results {
            if has_tool_calls {
                total_tool_calls_stripped += msg.tool_calls.as_ref().unwrap().len();
            }
            if has_tool_results {
                total_tool_results_stripped += msg.tool_results.as_ref().unwrap().len();
            }

            // Start with existing text content
            let existing_content = extract_text_content(&msg.content);
            let mut content_parts = Vec::new();

            if !existing_content.is_empty() {
                content_parts.push(existing_content);
            }

            // Convert tool_calls to text (for assistant messages)
            if has_tool_calls {
                let tool_text = tool_calls_to_text(msg.tool_calls.as_ref().unwrap());
                if !tool_text.is_empty() {
                    content_parts.push(tool_text);
                }
            }

            // Convert tool_results to text (for user messages)
            if has_tool_results {
                let result_text = tool_results_to_text(msg.tool_results.as_ref().unwrap());
                if !result_text.is_empty() {
                    content_parts.push(result_text);
                }
            }

            // Join all parts with double newline
            let content = if content_parts.is_empty() {
                "(empty)".to_string()
            } else {
                content_parts.join("\n\n")
            };

            // Create a copy of the message without tool content but with text representation
            let cleaned_msg = UnifiedMessage {
                role: msg.role,
                content: MessageContent::Text(content),
                tool_calls: None,
                tool_results: None,
                images: msg.images,
            };
            result.push(cleaned_msg);
        } else {
            result.push(msg);
        }
    }

    let had_tool_content = total_tool_calls_stripped > 0 || total_tool_results_stripped > 0;

    if had_tool_content {
        debug!(
            "Converted tool content to text (no tools defined): {} tool_calls, {} tool_results",
            total_tool_calls_stripped, total_tool_results_stripped
        );
    }

    (result, had_tool_content)
}

/// Ensures that messages with tool_results have a preceding assistant message with tool_calls.
///
/// Kiro API requires that when toolResults are present, there must be a preceding
/// assistantResponseMessage with toolUses. Some clients (like Cline/Roo) may send
/// truncated conversations where the assistant message is missing.
///
/// Since we don't know the original tool name and arguments when the assistant message
/// is missing, we cannot create a valid synthetic assistant message. Instead, we strip
/// the tool_results from such messages to avoid Kiro API rejection.
///
/// Returns a tuple of (processed messages, whether any tool_results were stripped).
pub fn ensure_assistant_before_tool_results(
    messages: Vec<UnifiedMessage>,
) -> (Vec<UnifiedMessage>, bool) {
    if messages.is_empty() {
        return (Vec::new(), false);
    }

    let mut result = Vec::new();
    let mut stripped_any_tool_results = false;

    for msg in messages {
        // Check if this message has tool_results
        if let Some(ref tool_results) = msg.tool_results {
            if !tool_results.is_empty() {
                // Check if the previous message is an assistant with tool_calls
                let has_preceding_assistant = result.last().is_some_and(|last: &UnifiedMessage| {
                    last.role == "assistant"
                        && last.tool_calls.as_ref().is_some_and(|tc| !tc.is_empty())
                });

                if !has_preceding_assistant {
                    // Strip the tool_results to avoid "Improperly formed request" error
                    let tool_ids: Vec<&str> = tool_results
                        .iter()
                        .map(|tr| tr.tool_use_id.as_str())
                        .collect();
                    warn!(
                        "Stripping {} orphaned tool_results (no preceding assistant message with tool_calls). Tool IDs: {:?}",
                        tool_results.len(),
                        tool_ids
                    );

                    // Create a copy of the message without tool_results
                    let cleaned_msg = UnifiedMessage {
                        role: msg.role,
                        content: msg.content,
                        tool_calls: msg.tool_calls,
                        tool_results: None,
                        images: msg.images,
                    };
                    result.push(cleaned_msg);
                    stripped_any_tool_results = true;
                    continue;
                }
            }
        }

        result.push(msg);
    }

    (result, stripped_any_tool_results)
}

// ==================================================================================================
// Message Merging
// ==================================================================================================

/// Merges adjacent messages with the same role.
pub fn merge_adjacent_messages(messages: Vec<UnifiedMessage>) -> Vec<UnifiedMessage> {
    if messages.is_empty() {
        return Vec::new();
    }

    let mut merged = Vec::new();
    let mut merge_counts: HashMap<String, usize> = HashMap::new();

    for msg in messages {
        if merged.is_empty() {
            merged.push(msg);
            continue;
        }

        let last_idx = merged.len() - 1;
        if msg.role == merged[last_idx].role {
            // Merge content
            let last_text = extract_text_content(&merged[last_idx].content);
            let current_text = extract_text_content(&msg.content);
            merged[last_idx].content =
                MessageContent::Text(format!("{}\n{}", last_text, current_text));

            // Merge tool_calls for assistant
            if msg.role == "assistant" {
                if let Some(mut calls) = msg.tool_calls {
                    merged[last_idx]
                        .tool_calls
                        .get_or_insert_with(Vec::new)
                        .append(&mut calls);
                }
            }

            // Merge tool_results for user
            if msg.role == "user" {
                if let Some(mut results) = msg.tool_results {
                    merged[last_idx]
                        .tool_results
                        .get_or_insert_with(Vec::new)
                        .append(&mut results);
                }
            }

            *merge_counts.entry(msg.role.clone()).or_insert(0) += 1;
        } else {
            merged.push(msg);
        }
    }

    let total_merges: usize = merge_counts.values().sum();
    if total_merges > 0 {
        debug!("Merged {} adjacent messages", total_merges);
    }

    merged
}

// ==================================================================================================
// Kiro History Building
// ==================================================================================================

/// Builds history array for Kiro API from unified messages.
pub fn build_kiro_history(messages: &[UnifiedMessage], model_id: &str) -> Vec<Value> {
    let mut history = Vec::new();

    for msg in messages {
        match msg.role.as_str() {
            "user" => {
                let mut content = extract_text_content(&msg.content);
                if content.is_empty() {
                    content = "(empty)".to_string();
                }

                let mut user_input = json!({
                    "content": content,
                    "modelId": model_id,
                    "origin": "AI_EDITOR",
                });

                // Process images
                let images_to_convert = if let Some(imgs) = &msg.images {
                    Some(imgs.clone())
                } else {
                    let extracted = extract_images_from_content(&msg.content);
                    if extracted.is_empty() {
                        None
                    } else {
                        Some(extracted)
                    }
                };

                if let Some(imgs) = images_to_convert {
                    let kiro_images = convert_images_to_kiro_format(&Some(imgs));
                    if !kiro_images.is_empty() {
                        user_input["images"] = Value::Array(kiro_images);
                    }
                }

                // Build userInputMessageContext for toolResults only
                let mut user_input_context = json!({});

                // Process tool_results - convert to Kiro format if present
                if let Some(tool_results) = &msg.tool_results {
                    let kiro_tool_results = convert_tool_results_to_kiro_format(tool_results);
                    if !kiro_tool_results.is_empty() {
                        user_input_context["toolResults"] = Value::Array(kiro_tool_results);
                    }
                } else {
                    // Try to extract from content (already in Kiro format)
                    let tool_results = extract_tool_results_from_content(&msg.content);
                    if !tool_results.is_empty() {
                        user_input_context["toolResults"] = Value::Array(tool_results);
                    }
                }

                if user_input_context
                    .as_object()
                    .is_some_and(|o| !o.is_empty())
                {
                    user_input["userInputMessageContext"] = user_input_context;
                }

                history.push(json!({"userInputMessage": user_input}));
            }
            "assistant" => {
                let mut content = extract_text_content(&msg.content);
                if content.is_empty() {
                    content = "(empty)".to_string();
                }

                let mut assistant_response = json!({"content": content});

                let tool_uses = extract_tool_uses_from_message(&msg.content, &msg.tool_calls);
                if !tool_uses.is_empty() {
                    assistant_response["toolUses"] = Value::Array(tool_uses);
                }

                history.push(json!({"assistantResponseMessage": assistant_response}));
            }
            _ => {}
        }
    }

    history
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_text_content() {
        let content = MessageContent::Text("Hello".to_string());
        assert_eq!(extract_text_content(&content), "Hello");

        let blocks = MessageContent::Blocks(vec![
            ContentBlock::Text {
                text: "Hello ".to_string(),
            },
            ContentBlock::Text {
                text: "World".to_string(),
            },
        ]);
        assert_eq!(extract_text_content(&blocks), "Hello World");
    }

    #[test]
    fn test_parse_data_url() {
        let url = "data:image/jpeg;base64,/9j/4AAQ";
        let result = parse_data_url(url);
        assert!(result.is_some());
        let (media_type, data) = result.unwrap();
        assert_eq!(media_type, "image/jpeg");
        assert_eq!(data, "/9j/4AAQ");
    }

    #[test]
    fn test_extract_tool_results_from_content() {
        // Test with content blocks containing tool_result
        let content = MessageContent::Blocks(vec![
            ContentBlock::Text {
                text: "Some text".to_string(),
            },
            ContentBlock::ToolResult {
                tool_use_id: "call_123".to_string(),
                content: "result output".to_string(),
            },
        ]);

        let results = extract_tool_results_from_content(&content);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["toolUseId"], "call_123");
        assert_eq!(results[0]["status"], "success");
        assert_eq!(results[0]["content"][0]["text"], "result output");
    }

    #[test]
    fn test_extract_tool_results_from_content_empty() {
        // Test with text content (no tool_results)
        let content = MessageContent::Text("Hello".to_string());
        let results = extract_tool_results_from_content(&content);
        assert!(results.is_empty());
    }

    #[test]
    fn test_extract_tool_results_from_content_empty_result() {
        // Test with empty tool result content
        let content = MessageContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: "call_456".to_string(),
            content: "".to_string(),
        }]);

        let results = extract_tool_results_from_content(&content);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["content"][0]["text"], "(empty result)");
    }

    #[test]
    fn test_sanitize_json_schema() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"}
            },
            "required": [],
            "additionalProperties": false
        });

        let sanitized = sanitize_json_schema(&schema);
        assert!(!sanitized["required"].is_array());
        assert!(sanitized["additionalProperties"].is_null());
        assert!(sanitized["properties"].is_object());
    }

    #[test]
    fn test_tool_calls_to_text() {
        let tool_calls = vec![ToolCall {
            id: "call_123".to_string(),
            call_type: "function".to_string(),
            function: ToolFunction {
                name: "bash".to_string(),
                arguments: r#"{"command": "ls"}"#.to_string(),
            },
        }];

        let result = tool_calls_to_text(&tool_calls);
        assert!(result.contains("[Tool: bash (call_123)]"));
        assert!(result.contains(r#"{"command": "ls"}"#));
    }

    #[test]
    fn test_tool_calls_to_text_empty() {
        let tool_calls: Vec<ToolCall> = vec![];
        let result = tool_calls_to_text(&tool_calls);
        assert!(result.is_empty());
    }

    #[test]
    fn test_tool_results_to_text() {
        let tool_results = vec![ToolResult {
            result_type: "tool_result".to_string(),
            tool_use_id: "call_123".to_string(),
            content: "file1.txt\nfile2.txt".to_string(),
        }];

        let result = tool_results_to_text(&tool_results);
        assert!(result.contains("[Tool Result (call_123)]"));
        assert!(result.contains("file1.txt\nfile2.txt"));
    }

    #[test]
    fn test_tool_results_to_text_empty_content() {
        let tool_results = vec![ToolResult {
            result_type: "tool_result".to_string(),
            tool_use_id: "call_456".to_string(),
            content: "".to_string(),
        }];

        let result = tool_results_to_text(&tool_results);
        assert!(result.contains("[Tool Result (call_456)]"));
        assert!(result.contains("(empty result)"));
    }

    #[test]
    fn test_strip_all_tool_content() {
        let messages = vec![
            UnifiedMessage {
                role: "user".to_string(),
                content: MessageContent::Text("Hello".to_string()),
                tool_calls: None,
                tool_results: None,
                images: None,
            },
            UnifiedMessage {
                role: "assistant".to_string(),
                content: MessageContent::Text("Let me help".to_string()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".to_string(),
                    call_type: "function".to_string(),
                    function: ToolFunction {
                        name: "bash".to_string(),
                        arguments: r#"{"cmd": "ls"}"#.to_string(),
                    },
                }]),
                tool_results: None,
                images: None,
            },
            UnifiedMessage {
                role: "user".to_string(),
                content: MessageContent::Text("".to_string()),
                tool_calls: None,
                tool_results: Some(vec![ToolResult {
                    result_type: "tool_result".to_string(),
                    tool_use_id: "call_1".to_string(),
                    content: "output".to_string(),
                }]),
                images: None,
            },
        ];

        let (result, had_tool_content) = strip_all_tool_content(messages);

        assert!(had_tool_content);
        assert_eq!(result.len(), 3);

        // First message unchanged
        assert_eq!(extract_text_content(&result[0].content), "Hello");
        assert!(result[0].tool_calls.is_none());

        // Second message: tool_calls converted to text
        let assistant_content = extract_text_content(&result[1].content);
        assert!(assistant_content.contains("Let me help"));
        assert!(assistant_content.contains("[Tool: bash (call_1)]"));
        assert!(result[1].tool_calls.is_none());

        // Third message: tool_results converted to text
        let user_content = extract_text_content(&result[2].content);
        assert!(user_content.contains("[Tool Result (call_1)]"));
        assert!(user_content.contains("output"));
        assert!(result[2].tool_results.is_none());
    }

    #[test]
    fn test_strip_all_tool_content_no_tools() {
        let messages = vec![UnifiedMessage {
            role: "user".to_string(),
            content: MessageContent::Text("Hello".to_string()),
            tool_calls: None,
            tool_results: None,
            images: None,
        }];

        let (result, had_tool_content) = strip_all_tool_content(messages);

        assert!(!had_tool_content);
        assert_eq!(result.len(), 1);
        assert_eq!(extract_text_content(&result[0].content), "Hello");
    }

    #[test]
    fn test_ensure_assistant_before_tool_results_valid() {
        let messages = vec![
            UnifiedMessage {
                role: "user".to_string(),
                content: MessageContent::Text("Hello".to_string()),
                tool_calls: None,
                tool_results: None,
                images: None,
            },
            UnifiedMessage {
                role: "assistant".to_string(),
                content: MessageContent::Text("Using tool".to_string()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".to_string(),
                    call_type: "function".to_string(),
                    function: ToolFunction {
                        name: "bash".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                tool_results: None,
                images: None,
            },
            UnifiedMessage {
                role: "user".to_string(),
                content: MessageContent::Text("".to_string()),
                tool_calls: None,
                tool_results: Some(vec![ToolResult {
                    result_type: "tool_result".to_string(),
                    tool_use_id: "call_1".to_string(),
                    content: "result".to_string(),
                }]),
                images: None,
            },
        ];

        let (result, stripped) = ensure_assistant_before_tool_results(messages);

        // Should not strip anything - valid sequence
        assert!(!stripped);
        assert_eq!(result.len(), 3);
        assert!(result[2].tool_results.is_some());
    }

    #[test]
    fn test_ensure_assistant_before_tool_results_orphaned() {
        let messages = vec![
            UnifiedMessage {
                role: "user".to_string(),
                content: MessageContent::Text("Hello".to_string()),
                tool_calls: None,
                tool_results: None,
                images: None,
            },
            // Missing assistant message with tool_calls
            UnifiedMessage {
                role: "user".to_string(),
                content: MessageContent::Text("".to_string()),
                tool_calls: None,
                tool_results: Some(vec![ToolResult {
                    result_type: "tool_result".to_string(),
                    tool_use_id: "call_orphan".to_string(),
                    content: "orphaned result".to_string(),
                }]),
                images: None,
            },
        ];

        let (result, stripped) = ensure_assistant_before_tool_results(messages);

        // Should strip orphaned tool_results
        assert!(stripped);
        assert_eq!(result.len(), 2);
        assert!(result[1].tool_results.is_none());
    }

    #[test]
    fn test_merge_adjacent_messages() {
        let messages = vec![
            UnifiedMessage {
                role: "user".to_string(),
                content: MessageContent::Text("Hello".to_string()),
                tool_calls: None,
                tool_results: None,
                images: None,
            },
            UnifiedMessage {
                role: "user".to_string(),
                content: MessageContent::Text("World".to_string()),
                tool_calls: None,
                tool_results: None,
                images: None,
            },
        ];

        let result = merge_adjacent_messages(messages);
        assert_eq!(result.len(), 1);
        let content = extract_text_content(&result[0].content);
        assert!(content.contains("Hello"));
        assert!(content.contains("World"));
    }

    #[test]
    fn test_merge_adjacent_messages_different_roles() {
        let messages = vec![
            UnifiedMessage {
                role: "user".to_string(),
                content: MessageContent::Text("Hello".to_string()),
                tool_calls: None,
                tool_results: None,
                images: None,
            },
            UnifiedMessage {
                role: "assistant".to_string(),
                content: MessageContent::Text("Hi there".to_string()),
                tool_calls: None,
                tool_results: None,
                images: None,
            },
        ];

        let result = merge_adjacent_messages(messages);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_merge_adjacent_messages_empty() {
        let messages: Vec<UnifiedMessage> = vec![];
        let result = merge_adjacent_messages(messages);
        assert!(result.is_empty());
    }

    #[test]
    fn test_convert_tool_results_to_kiro_format() {
        let tool_results = vec![ToolResult {
            result_type: "tool_result".to_string(),
            tool_use_id: "call_123".to_string(),
            content: "Success output".to_string(),
        }];

        let result = convert_tool_results_to_kiro_format(&tool_results);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["toolUseId"], "call_123");
        assert_eq!(result[0]["status"], "success");
        assert_eq!(result[0]["content"][0]["text"], "Success output");
    }

    #[test]
    fn test_convert_images_to_kiro_format() {
        let images = Some(vec![UnifiedImage {
            media_type: "image/jpeg".to_string(),
            data: "base64data".to_string(),
        }]);

        let result = convert_images_to_kiro_format(&images);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["format"], "jpeg");
        assert_eq!(result[0]["source"]["bytes"], "base64data");
    }

    #[test]
    fn test_convert_images_to_kiro_format_none() {
        let result = convert_images_to_kiro_format(&None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_convert_images_to_kiro_format_empty_data() {
        let images = Some(vec![UnifiedImage {
            media_type: "image/png".to_string(),
            data: "".to_string(),
        }]);

        let result = convert_images_to_kiro_format(&images);
        assert!(result.is_empty()); // Empty data should be skipped
    }

    #[test]
    fn test_convert_tools_to_kiro_format() {
        let tools = Some(vec![UnifiedTool {
            name: "get_weather".to_string(),
            description: Some("Get weather for a location".to_string()),
            input_schema: Some(json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string"}
                }
            })),
        }]);

        let result = convert_tools_to_kiro_format(&tools);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["toolSpecification"]["name"], "get_weather");
        assert_eq!(
            result[0]["toolSpecification"]["description"],
            "Get weather for a location"
        );
    }

    #[test]
    fn test_convert_tools_to_kiro_format_empty_description() {
        let tools = Some(vec![UnifiedTool {
            name: "my_tool".to_string(),
            description: Some("".to_string()),
            input_schema: None,
        }]);

        let result = convert_tools_to_kiro_format(&tools);
        assert_eq!(result.len(), 1);
        // Empty description should get a placeholder
        assert_eq!(
            result[0]["toolSpecification"]["description"],
            "Tool: my_tool"
        );
    }

    #[test]
    fn test_convert_tools_to_kiro_format_none() {
        let result = convert_tools_to_kiro_format(&None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_extract_images_from_content_openai_format() {
        let content = MessageContent::Blocks(vec![ContentBlock::ImageUrl {
            image_url: ImageUrl {
                url: "data:image/jpeg;base64,/9j/4AAQ".to_string(),
            },
        }]);

        let result = extract_images_from_content(&content);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].media_type, "image/jpeg");
        assert_eq!(result[0].data, "/9j/4AAQ");
    }

    #[test]
    fn test_extract_images_from_content_anthropic_format() {
        let content = MessageContent::Blocks(vec![ContentBlock::Image {
            source: ImageSource {
                source_type: "base64".to_string(),
                media_type: Some("image/png".to_string()),
                data: Some("pngdata".to_string()),
                url: None,
            },
        }]);

        let result = extract_images_from_content(&content);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].media_type, "image/png");
        assert_eq!(result[0].data, "pngdata");
    }

    #[test]
    fn test_extract_images_from_content_text_only() {
        let content = MessageContent::Text("No images here".to_string());
        let result = extract_images_from_content(&content);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_data_url_invalid() {
        assert!(parse_data_url("not a data url").is_none());
        assert!(parse_data_url("data:").is_none());
        assert!(parse_data_url("http://example.com/image.jpg").is_none());
    }

    #[test]
    fn test_sanitize_json_schema_nested() {
        let schema = json!({
            "type": "object",
            "properties": {
                "nested": {
                    "type": "object",
                    "properties": {
                        "field": {"type": "string"}
                    },
                    "required": [],
                    "additionalProperties": false
                }
            }
        });

        let sanitized = sanitize_json_schema(&schema);
        // Nested required and additionalProperties should also be removed
        assert!(sanitized["properties"]["nested"]["required"].is_null());
        assert!(sanitized["properties"]["nested"]["additionalProperties"].is_null());
    }

    #[test]
    fn test_sanitize_json_schema_non_object() {
        let schema = json!("string");
        let sanitized = sanitize_json_schema(&schema);
        assert_eq!(sanitized, json!("string"));
    }

    #[test]
    fn test_extract_tool_uses_from_message() {
        let content = MessageContent::Blocks(vec![ContentBlock::ToolUse {
            id: "tool_123".to_string(),
            name: "bash".to_string(),
            input: json!({"command": "ls"}),
        }]);

        let result = extract_tool_uses_from_message(&content, &None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["toolUseId"], "tool_123");
        assert_eq!(result[0]["name"], "bash");
    }

    #[test]
    fn test_extract_tool_uses_from_tool_calls() {
        let content = MessageContent::Text("".to_string());
        let tool_calls = Some(vec![ToolCall {
            id: "call_456".to_string(),
            call_type: "function".to_string(),
            function: ToolFunction {
                name: "get_weather".to_string(),
                arguments: r#"{"location": "NYC"}"#.to_string(),
            },
        }]);

        let result = extract_tool_uses_from_message(&content, &tool_calls);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["toolUseId"], "call_456");
        assert_eq!(result[0]["name"], "get_weather");
        assert_eq!(result[0]["input"]["location"], "NYC");
    }

    #[test]
    fn test_extract_tool_uses_deduplicates_by_id() {
        // Simulate duplicate tool uses with same ID - one with args, one empty
        let content = MessageContent::Blocks(vec![
            ContentBlock::ToolUse {
                id: "tool_123".to_string(),
                name: "Read".to_string(),
                input: json!({"file_path": "/path/to/file.txt"}),
            },
            ContentBlock::ToolUse {
                id: "tool_123".to_string(),
                name: "Read".to_string(),
                input: json!({}), // Empty - should be discarded
            },
        ]);

        let result = extract_tool_uses_from_message(&content, &None);
        assert_eq!(result.len(), 1, "Should deduplicate to 1 tool use");
        assert_eq!(result[0]["toolUseId"], "tool_123");
        assert_eq!(result[0]["input"]["file_path"], "/path/to/file.txt");
    }

    #[test]
    fn test_extract_tool_uses_keeps_larger_input() {
        // When both have content, keep the one with more
        let content = MessageContent::Blocks(vec![
            ContentBlock::ToolUse {
                id: "tool_abc".to_string(),
                name: "Write".to_string(),
                input: json!({"path": "a"}), // Smaller
            },
            ContentBlock::ToolUse {
                id: "tool_abc".to_string(),
                name: "Write".to_string(),
                input: json!({"path": "a", "content": "lots of content here"}), // Larger
            },
        ]);

        let result = extract_tool_uses_from_message(&content, &None);
        assert_eq!(result.len(), 1);
        // Should keep the one with more content
        assert!(result[0]["input"]["content"].is_string());
    }
}
