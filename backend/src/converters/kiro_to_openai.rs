// Kiro to OpenAI converter
//
// This module converts Kiro API responses to OpenAI API format.
// Handles both streaming and non-streaming responses.

#![allow(dead_code)]

use serde_json::json;

use crate::models::kiro::KiroResponse;
use crate::models::openai::{
    ChatCompletionChoice, ChatCompletionResponse, ChatCompletionUsage, ChatMessage, FunctionCall,
    ToolCall,
};

/// Converts Kiro response to OpenAI ChatCompletionResponse.
///
/// This handles the non-streaming case where we have a complete response.
pub fn convert_kiro_to_openai_response(
    kiro_response: &KiroResponse,
    model: &str,
    request_id: &str,
) -> ChatCompletionResponse {
    // Extract content from assistant response message
    let content_text = kiro_response
        .assistant_response_message
        .content
        .iter()
        .map(|block| match block {
            crate::models::kiro::ContentBlock::Text { text } => text.as_str(),
        })
        .collect::<Vec<_>>()
        .join("");

    let content = if content_text.is_empty() {
        json!("")
    } else {
        json!(content_text)
    };

    // Extract tool uses if present
    let tool_calls = if let Some(tool_uses) = &kiro_response.assistant_response_message.tool_uses {
        let calls: Vec<ToolCall> = tool_uses
            .iter()
            .map(|tool_use| {
                let tool_use_id = tool_use.tool_use_id.clone();
                let name = tool_use.name.clone();
                let arguments =
                    serde_json::to_string(&tool_use.input).unwrap_or_else(|_| "{}".to_string());

                ToolCall {
                    id: tool_use_id,
                    tool_type: "function".to_string(),
                    function: FunctionCall { name, arguments },
                }
            })
            .collect();

        if calls.is_empty() {
            None
        } else {
            Some(calls)
        }
    } else {
        None
    };

    // Determine finish_reason
    let finish_reason = if tool_calls.is_some() {
        Some("tool_calls".to_string())
    } else {
        Some("stop".to_string())
    };

    // Create message
    let message = ChatMessage {
        role: "assistant".to_string(),
        content: Some(content),
        name: None,
        tool_calls,
        tool_call_id: None,
    };

    // Create choice
    let choice = ChatCompletionChoice {
        index: 0,
        message,
        finish_reason,
        logprobs: None,
    };

    // Calculate usage
    let usage = if let Some(kiro_usage) = &kiro_response.usage {
        ChatCompletionUsage {
            prompt_tokens: kiro_usage.input_tokens,
            completion_tokens: kiro_usage.output_tokens,
            total_tokens: kiro_usage.input_tokens + kiro_usage.output_tokens,
            credits_used: None,
        }
    } else {
        ChatCompletionUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            credits_used: None,
        }
    };

    ChatCompletionResponse {
        id: request_id.to_string(),
        object: "chat.completion".to_string(),
        created: chrono::Utc::now().timestamp(),
        model: model.to_string(),
        choices: vec![choice],
        usage: Some(usage),
        system_fingerprint: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_kiro_to_openai_simple() {
        let kiro_response = KiroResponse {
            conversation_id: "test-conv".to_string(),
            assistant_response_message: crate::models::kiro::AssistantResponseMessage {
                content: vec![crate::models::kiro::ContentBlock::Text {
                    text: "Hello, world!".to_string(),
                }],
                tool_uses: None,
            },
            usage: None,
        };

        let response =
            convert_kiro_to_openai_response(&kiro_response, "claude-sonnet-4", "test-123");

        assert_eq!(response.model, "claude-sonnet-4");
        assert_eq!(response.id, "test-123");
        assert_eq!(response.choices.len(), 1);
        assert_eq!(response.choices[0].message.role, "assistant");
        assert_eq!(
            response.choices[0].message.content,
            Some(json!("Hello, world!"))
        );
        assert_eq!(response.choices[0].finish_reason, Some("stop".to_string()));
    }

    #[test]
    fn test_convert_kiro_to_openai_with_tools() {
        let kiro_response = KiroResponse {
            conversation_id: "test-conv".to_string(),
            assistant_response_message: crate::models::kiro::AssistantResponseMessage {
                content: vec![crate::models::kiro::ContentBlock::Text {
                    text: "Let me check that.".to_string(),
                }],
                tool_uses: Some(vec![crate::models::kiro::ToolUse {
                    tool_use_id: "call_abc123".to_string(),
                    name: "get_weather".to_string(),
                    input: json!({"location": "San Francisco"}),
                }]),
            },
            usage: None,
        };

        let response =
            convert_kiro_to_openai_response(&kiro_response, "claude-sonnet-4", "test-456");

        assert_eq!(response.choices.len(), 1);
        assert!(response.choices[0].message.tool_calls.is_some());
        let tool_calls = response.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_abc123");
        assert_eq!(tool_calls[0].function.name, "get_weather");
        assert_eq!(
            response.choices[0].finish_reason,
            Some("tool_calls".to_string())
        );
    }
}
