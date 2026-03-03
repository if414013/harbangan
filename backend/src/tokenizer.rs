use crate::models::anthropic::AnthropicTool;
use crate::models::openai::{ChatMessage, Tool};
use serde_json::Value;
use std::sync::OnceLock;
use tiktoken_rs::CoreBPE;

/// Correction coefficient for Claude models
/// Claude tokenizes text approximately 15% more than GPT-4 (cl100k_base)
const CLAUDE_CORRECTION_FACTOR: f64 = 1.15;

/// Service tokens per message (role, delimiters, etc.)
const TOKENS_PER_MESSAGE: i32 = 4;

/// Service tokens per tool definition
const TOKENS_PER_TOOL: i32 = 4;

/// Service tokens per tool call
const TOKENS_PER_TOOL_CALL: i32 = 4;

/// Final service tokens added to request
const FINAL_SERVICE_TOKENS: i32 = 3;

/// Approximate tokens per image
const TOKENS_PER_IMAGE: i32 = 100;

/// Global tiktoken encoding (lazily initialized)
static ENCODING: OnceLock<CoreBPE> = OnceLock::new();

/// Get the tiktoken encoding (cl100k_base), initializing if needed
fn get_encoding() -> &'static CoreBPE {
    ENCODING.get_or_init(|| {
        tiktoken_rs::cl100k_base().expect("Failed to initialize cl100k_base encoding")
    })
}

/// Counts the approximate number of tokens in text.
///
/// Uses tiktoken cl100k_base encoding for accurate counting.
/// Optionally applies Claude correction factor.
///
/// # Arguments
/// * `text` - Text to count tokens for
/// * `apply_claude_correction` - Whether to apply the Claude correction factor
///
/// # Returns
/// Approximate number of tokens
pub fn count_tokens(text: &str, apply_claude_correction: bool) -> i32 {
    if text.is_empty() {
        return 0;
    }

    let base_tokens = get_encoding().encode_with_special_tokens(text).len() as f64;

    if apply_claude_correction {
        (base_tokens * CLAUDE_CORRECTION_FACTOR) as i32
    } else {
        base_tokens as i32
    }
}

/// Counts tokens in a list of OpenAI chat messages.
///
/// Accounts for message structure:
/// - role: tokens for role string
/// - content: text tokens or multimodal content
/// - tool_calls: function name and arguments
/// - tool_call_id: for tool response messages
/// - Service tokens per message: ~4 tokens
///
/// # Arguments
/// * `messages` - List of messages in OpenAI format
/// * `apply_claude_correction` - Whether to apply the Claude correction factor
///
/// # Returns
/// Approximate number of tokens
pub fn count_message_tokens(messages: &[ChatMessage], apply_claude_correction: bool) -> i32 {
    if messages.is_empty() {
        return 0;
    }

    let mut total_tokens = 0;

    for message in messages {
        // Base tokens per message (role, delimiters)
        total_tokens += TOKENS_PER_MESSAGE;

        // Role tokens
        total_tokens += count_tokens(&message.role, false);

        // Content tokens
        if let Some(content) = &message.content {
            match content {
                Value::String(s) => {
                    total_tokens += count_tokens(s, false);
                }
                Value::Array(arr) => {
                    for item in arr {
                        if let Some(obj) = item.as_object() {
                            match obj.get("type").and_then(|t| t.as_str()) {
                                Some("text") => {
                                    if let Some(text) = obj.get("text").and_then(|t| t.as_str()) {
                                        total_tokens += count_tokens(text, false);
                                    }
                                }
                                Some("image_url") | Some("image") => {
                                    total_tokens += TOKENS_PER_IMAGE;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Tool calls tokens (for assistant messages with function calls)
        if let Some(tool_calls) = &message.tool_calls {
            for tool_call in tool_calls {
                total_tokens += TOKENS_PER_TOOL_CALL;
                total_tokens += count_tokens(&tool_call.id, false);
                total_tokens += count_tokens(&tool_call.tool_type, false);
                total_tokens += count_tokens(&tool_call.function.name, false);
                total_tokens += count_tokens(&tool_call.function.arguments, false);
            }
        }

        // Tool call ID tokens (for tool response messages)
        if let Some(tool_call_id) = &message.tool_call_id {
            total_tokens += count_tokens(tool_call_id, false);
        }

        // Name tokens (optional sender name)
        if let Some(name) = &message.name {
            total_tokens += count_tokens(name, false);
        }
    }

    // Final service tokens
    total_tokens += FINAL_SERVICE_TOKENS;

    if apply_claude_correction {
        (total_tokens as f64 * CLAUDE_CORRECTION_FACTOR) as i32
    } else {
        total_tokens
    }
}

/// Counts tokens in OpenAI tool definitions.
///
/// Accounts for tool structure:
/// - type: "function"
/// - function.name: function name
/// - function.description: optional description
/// - function.parameters: JSON schema
/// - Service tokens per tool: ~4 tokens
///
/// # Arguments
/// * `tools` - Optional list of tools in OpenAI format
/// * `apply_claude_correction` - Whether to apply the Claude correction factor
///
/// # Returns
/// Approximate number of tokens
pub fn count_tools_tokens(tools: Option<&Vec<Tool>>, apply_claude_correction: bool) -> i32 {
    let Some(tools_list) = tools else {
        return 0;
    };

    if tools_list.is_empty() {
        return 0;
    }

    let mut total_tokens = 0;

    for tool in tools_list {
        total_tokens += TOKENS_PER_TOOL;
        total_tokens += count_tokens(&tool.tool_type, false);
        total_tokens += count_tokens(&tool.function.name, false);

        if let Some(ref desc) = tool.function.description {
            total_tokens += count_tokens(desc, false);
        }

        if let Some(ref params) = tool.function.parameters {
            let params_str = serde_json::to_string(params).unwrap_or_default();
            total_tokens += count_tokens(&params_str, false);
        }
    }

    if apply_claude_correction {
        (total_tokens as f64 * CLAUDE_CORRECTION_FACTOR) as i32
    } else {
        total_tokens
    }
}

/// Counts tokens in a list of Anthropic messages.
///
/// Accounts for message structure:
/// - role: ~1 token
/// - content: text tokens
/// - Service tokens between messages: ~3-4 tokens
///
/// # Arguments
/// * `messages` - List of messages in Anthropic format
/// * `system` - Optional system prompt
/// * `tools` - Optional tools definition
///
/// # Returns
/// Approximate number of input tokens
pub fn count_anthropic_message_tokens(
    messages: &[crate::models::anthropic::AnthropicMessage],
    system: Option<&Value>,
    tools: Option<&Vec<AnthropicTool>>,
) -> i32 {
    if messages.is_empty() && system.is_none() && tools.is_none() {
        return 0;
    }

    let mut total_tokens = 0;

    // Count system prompt tokens
    if let Some(sys) = system {
        total_tokens += 4; // Service tokens
        match sys {
            Value::String(s) => {
                total_tokens += count_tokens(s, false);
            }
            Value::Array(arr) => {
                for item in arr {
                    if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                        total_tokens += count_tokens(text, false);
                    }
                }
            }
            _ => {}
        }
    }

    // Count message tokens
    for message in messages {
        // Base tokens per message (role, delimiters)
        total_tokens += 4;

        // Role tokens
        total_tokens += count_tokens(&message.role, false);

        // Content tokens
        match &message.content {
            Value::String(s) => {
                total_tokens += count_tokens(s, false);
            }
            Value::Array(arr) => {
                for item in arr {
                    if let Some(obj) = item.as_object() {
                        match obj.get("type").and_then(|t| t.as_str()) {
                            Some("text") => {
                                if let Some(text) = obj.get("text").and_then(|t| t.as_str()) {
                                    total_tokens += count_tokens(text, false);
                                }
                            }
                            Some("image") | Some("image_url") => {
                                // Images take ~85-170 tokens depending on size
                                total_tokens += 100;
                            }
                            Some("tool_use") => {
                                total_tokens += 4; // Service tokens
                                if let Some(name) = obj.get("name").and_then(|n| n.as_str()) {
                                    total_tokens += count_tokens(name, false);
                                }
                                if let Some(input) = obj.get("input") {
                                    let input_str =
                                        serde_json::to_string(input).unwrap_or_default();
                                    total_tokens += count_tokens(&input_str, false);
                                }
                            }
                            Some("tool_result") => {
                                total_tokens += 4; // Service tokens
                                if let Some(tool_use_id) =
                                    obj.get("tool_use_id").and_then(|id| id.as_str())
                                {
                                    total_tokens += count_tokens(tool_use_id, false);
                                }
                                if let Some(content) = obj.get("content") {
                                    match content {
                                        Value::String(s) => {
                                            total_tokens += count_tokens(s, false);
                                        }
                                        Value::Array(arr) => {
                                            for c in arr {
                                                if let Some(text) =
                                                    c.get("text").and_then(|t| t.as_str())
                                                {
                                                    total_tokens += count_tokens(text, false);
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Some("thinking") => {
                                if let Some(thinking) = obj.get("thinking").and_then(|t| t.as_str())
                                {
                                    total_tokens += count_tokens(thinking, false);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Count tools tokens
    if let Some(tools_list) = tools {
        for tool in tools_list {
            total_tokens += 4; // Service tokens

            total_tokens += count_tokens(&tool.name, false);
            if let Some(ref desc) = tool.description {
                total_tokens += count_tokens(desc, false);
            }
            let schema_str = serde_json::to_string(&tool.input_schema).unwrap_or_default();
            total_tokens += count_tokens(&schema_str, false);
        }
    }

    // Final service tokens
    total_tokens += 3;

    // Apply Claude correction to total count
    (total_tokens as f64 * CLAUDE_CORRECTION_FACTOR) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::anthropic::AnthropicMessage;
    use crate::models::openai::{FunctionCall, ToolCall, ToolFunction};
    use serde_json::json;

    #[test]
    fn test_count_tokens_empty() {
        assert_eq!(count_tokens("", true), 0);
        assert_eq!(count_tokens("", false), 0);
    }

    #[test]
    fn test_count_tokens_simple() {
        // "Hello world" = 2 tokens with tiktoken
        let tokens = count_tokens("Hello world", false);
        assert_eq!(tokens, 2);

        // With correction factor
        let tokens_corrected = count_tokens("Hello world", true);
        assert_eq!(tokens_corrected, 2); // 2 * 1.15 = 2.3 -> 2
    }

    #[test]
    fn test_count_tokens_without_correction() {
        // Use a longer string so the correction factor makes a visible difference
        let long_text = "This is a much longer text that should have enough tokens to show the difference between corrected and uncorrected counts.";
        let with_correction = count_tokens(long_text, true);
        let without_correction = count_tokens(long_text, false);
        assert!(with_correction >= without_correction);
        // For longer text, correction should be strictly greater
        assert!(with_correction > without_correction);
    }

    #[test]
    fn test_count_tokens_tiktoken_accuracy() {
        // Test known token counts with tiktoken cl100k_base
        // "Hello" = 1 token
        assert_eq!(count_tokens("Hello", false), 1);
        // "The quick brown fox" = 4 tokens
        assert_eq!(count_tokens("The quick brown fox", false), 4);
    }

    // ==================== OpenAI Message Token Tests ====================

    #[test]
    fn test_count_message_tokens_empty() {
        let messages: Vec<ChatMessage> = vec![];
        assert_eq!(count_message_tokens(&messages, false), 0);
    }

    #[test]
    fn test_count_message_tokens_simple() {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: Some(json!("Hello, how are you?")),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }];
        let tokens = count_message_tokens(&messages, false);
        // Should include: TOKENS_PER_MESSAGE (4) + role tokens + content tokens + FINAL_SERVICE_TOKENS (3)
        assert!(tokens > 7); // At least service tokens + some content
    }

    #[test]
    fn test_count_message_tokens_with_correction() {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: Some(json!(
                "This is a longer message to test the correction factor application."
            )),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }];
        let without_correction = count_message_tokens(&messages, false);
        let with_correction = count_message_tokens(&messages, true);
        assert!(with_correction > without_correction);
    }

    #[test]
    fn test_count_message_tokens_multimodal() {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: Some(json!([
                {"type": "text", "text": "What's in this image?"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,..."}}
            ])),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }];
        let tokens = count_message_tokens(&messages, false);
        // Should include at least TOKENS_PER_IMAGE (100)
        assert!(tokens >= 100);
    }

    #[test]
    fn test_count_message_tokens_with_tool_calls() {
        let messages = vec![ChatMessage {
            role: "assistant".to_string(),
            content: None,
            name: None,
            tool_calls: Some(vec![ToolCall {
                id: "call_123".to_string(),
                tool_type: "function".to_string(),
                function: FunctionCall {
                    name: "get_weather".to_string(),
                    arguments: r#"{"location": "San Francisco"}"#.to_string(),
                },
            }]),
            tool_call_id: None,
        }];
        let tokens = count_message_tokens(&messages, false);
        // Should include tool call tokens
        assert!(tokens > 10);
    }

    #[test]
    fn test_count_message_tokens_tool_response() {
        let messages = vec![ChatMessage {
            role: "tool".to_string(),
            content: Some(json!("The weather in San Francisco is 72°F and sunny.")),
            name: None,
            tool_calls: None,
            tool_call_id: Some("call_123".to_string()),
        }];
        let tokens = count_message_tokens(&messages, false);
        assert!(tokens > 7);
    }

    // ==================== OpenAI Tools Token Tests ====================

    #[test]
    fn test_count_tools_tokens_none() {
        assert_eq!(count_tools_tokens(None, false), 0);
    }

    #[test]
    fn test_count_tools_tokens_empty() {
        let tools: Vec<Tool> = vec![];
        assert_eq!(count_tools_tokens(Some(&tools), false), 0);
    }

    #[test]
    fn test_count_tools_tokens_simple() {
        let tools = vec![Tool {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: "get_weather".to_string(),
                description: Some("Get the current weather in a location".to_string()),
                parameters: Some(json!({
                    "type": "object",
                    "properties": {
                        "location": {
                            "type": "string",
                            "description": "The city and state"
                        }
                    },
                    "required": ["location"]
                })),
            },
        }];
        let tokens = count_tools_tokens(Some(&tools), false);
        // Should include: TOKENS_PER_TOOL (4) + type + name + description + parameters
        assert!(tokens > 10);
    }

    #[test]
    fn test_count_tools_tokens_with_correction() {
        let tools = vec![Tool {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: "search_database".to_string(),
                description: Some("Search the database for records matching the query".to_string()),
                parameters: Some(json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"},
                        "limit": {"type": "integer"}
                    }
                })),
            },
        }];
        let without_correction = count_tools_tokens(Some(&tools), false);
        let with_correction = count_tools_tokens(Some(&tools), true);
        assert!(with_correction > without_correction);
    }

    #[test]
    fn test_count_tools_tokens_multiple() {
        let tools = vec![
            Tool {
                tool_type: "function".to_string(),
                function: ToolFunction {
                    name: "tool_one".to_string(),
                    description: Some("First tool".to_string()),
                    parameters: None,
                },
            },
            Tool {
                tool_type: "function".to_string(),
                function: ToolFunction {
                    name: "tool_two".to_string(),
                    description: Some("Second tool".to_string()),
                    parameters: None,
                },
            },
        ];
        let tokens = count_tools_tokens(Some(&tools), false);
        // Should be roughly double a single tool
        let single_tool = vec![tools[0].clone()];
        let single_tokens = count_tools_tokens(Some(&single_tool), false);
        assert!(tokens > single_tokens);
    }

    // ==================== Anthropic Message Token Tests ====================

    #[test]
    fn test_count_anthropic_message_tokens_empty() {
        let messages: Vec<AnthropicMessage> = vec![];
        assert_eq!(count_anthropic_message_tokens(&messages, None, None), 0);
    }

    #[test]
    fn test_count_anthropic_message_tokens_simple() {
        let messages = vec![AnthropicMessage {
            role: "user".to_string(),
            content: json!("Hello, how are you?"),
        }];
        let tokens = count_anthropic_message_tokens(&messages, None, None);
        assert!(tokens > 0);
    }

    #[test]
    fn test_count_anthropic_message_tokens_with_system() {
        let messages = vec![AnthropicMessage {
            role: "user".to_string(),
            content: json!("Hello"),
        }];
        let system = json!("You are a helpful assistant.");
        let tokens = count_anthropic_message_tokens(&messages, Some(&system), None);
        assert!(tokens > 0);
    }

    #[test]
    fn test_count_anthropic_message_tokens_multimodal() {
        let messages = vec![AnthropicMessage {
            role: "user".to_string(),
            content: json!([
                {"type": "text", "text": "What's in this image?"},
                {"type": "image", "source": {"type": "base64", "data": "..."}}
            ]),
        }];
        let tokens = count_anthropic_message_tokens(&messages, None, None);
        assert!(tokens >= 100); // At least image tokens
    }
}
