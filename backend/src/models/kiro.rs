#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ==================================================================================================
// Request Models
// ==================================================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroRequest {
    pub conversation_id: String,
    pub model_id: String,
    pub user_input_message: UserInputMessage,
    pub user_input_message_context: UserInputMessageContext,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_configuration: Option<ToolConfiguration>,
}

impl KiroRequest {
    pub fn new(model_id: String, message: String) -> Self {
        Self {
            conversation_id: Uuid::new_v4().to_string(),
            model_id,
            user_input_message: UserInputMessage {
                content: vec![ContentBlock::Text { text: message }],
                images: None,
            },
            user_input_message_context: UserInputMessageContext {
                system: None,
                previous_turns: vec![],
            },
            tool_configuration: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInputMessage {
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<KiroImage>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInputMessageContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<Vec<SystemBlock>>,
    #[serde(default)]
    pub previous_turns: Vec<Turn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemBlock {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub user_input_message: UserInputMessage,
    pub assistant_response_message: AssistantResponseMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantResponseMessage {
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_uses: Option<Vec<ToolUse>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroImage {
    pub format: String,
    pub source: ImageSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    pub bytes: String,
}

// ==================================================================================================
// Tool Models
// ==================================================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfiguration {
    pub tools: Vec<ToolSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSpec {
    pub tool_specification: ToolSpecification,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSpecification {
    pub name: String,
    pub description: String,
    pub input_schema: InputSchema,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputSchema {
    pub json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolUse {
    pub tool_use_id: String,
    pub name: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub content: Vec<ContentBlock>,
    pub status: String,
    pub tool_use_id: String,
}

// ==================================================================================================
// Response Models
// ==================================================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroResponse {
    pub conversation_id: String,
    pub assistant_response_message: AssistantResponseMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<KiroUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroUsage {
    pub input_tokens: i32,
    pub output_tokens: i32,
}

// ==================================================================================================
// Streaming Models
// ==================================================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum KiroStreamEvent {
    #[serde(rename = "messageStart")]
    MessageStart {
        #[serde(rename = "conversationId")]
        conversation_id: String,
    },
    #[serde(rename = "contentBlockStart")]
    ContentBlockStart {
        index: i32,
        #[serde(rename = "contentBlock")]
        content_block: serde_json::Value,
    },
    #[serde(rename = "contentBlockDelta")]
    ContentBlockDelta { index: i32, delta: Delta },
    #[serde(rename = "contentBlockStop")]
    ContentBlockStop { index: i32 },
    #[serde(rename = "messageStop")]
    MessageStop {
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<KiroUsage>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[allow(clippy::enum_variant_names)]
pub enum Delta {
    #[serde(rename = "textDelta")]
    TextDelta { text: String },
    #[serde(rename = "thinkingDelta")]
    ThinkingDelta { thinking: String },
    #[serde(rename = "toolUseDelta")]
    ToolUseDelta {
        #[serde(rename = "toolUseId")]
        tool_use_id: String,
        name: String,
        input: String,
    },
}

// ==================================================================================================
// Helper Functions
// ==================================================================================================

impl KiroRequest {
    /// Add system prompt to the request
    pub fn with_system(mut self, system: String) -> Self {
        if !system.is_empty() {
            self.user_input_message_context.system = Some(vec![SystemBlock { text: system }]);
        }
        self
    }

    /// Add tools to the request
    pub fn with_tools(mut self, tools: Vec<ToolSpec>) -> Self {
        if !tools.is_empty() {
            self.tool_configuration = Some(ToolConfiguration { tools });
        }
        self
    }

    /// Add previous conversation turns
    pub fn with_turns(mut self, turns: Vec<Turn>) -> Self {
        self.user_input_message_context.previous_turns = turns;
        self
    }

    /// Add images to the user message
    pub fn with_images(mut self, images: Vec<KiroImage>) -> Self {
        if !images.is_empty() {
            self.user_input_message.images = Some(images);
        }
        self
    }
}
