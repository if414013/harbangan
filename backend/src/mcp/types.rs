use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ── Connection & auth enums ──────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpConnectionType {
    Http,
    Sse,
    Stdio,
}

impl McpConnectionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            McpConnectionType::Http => "http",
            McpConnectionType::Sse => "sse",
            McpConnectionType::Stdio => "stdio",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "http" => Some(McpConnectionType::Http),
            "sse" => Some(McpConnectionType::Sse),
            "stdio" => Some(McpConnectionType::Stdio),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpAuthType {
    None,
    Headers,
}

impl McpAuthType {
    pub fn as_str(&self) -> &'static str {
        match self {
            McpAuthType::None => "none",
            McpAuthType::Headers => "headers",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "none" => Some(McpAuthType::None),
            "headers" => Some(McpAuthType::Headers),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpConnectionState {
    Connected,
    Connecting,
    Disconnected,
    Error,
}

// ── STDIO config ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpStdioConfig {
    pub command: String,
    pub args: Vec<String>,
    #[serde(default)]
    pub envs: std::collections::HashMap<String, String>,
}

// ── Client config (DB-persisted) ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpClientConfig {
    pub id: Uuid,
    pub name: String,
    pub connection_type: McpConnectionType,
    pub connection_string: Option<String>,
    pub stdio_config: Option<McpStdioConfig>,
    pub auth_type: McpAuthType,
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
    pub tools_to_execute: Vec<String>,
    pub is_ping_available: bool,
    pub tool_sync_interval_secs: i32,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ── Client runtime state ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct McpClientState {
    pub config: McpClientConfig,
    pub connection_state: McpConnectionState,
    pub tools: std::collections::HashMap<String, McpTool>,
    pub last_error: Option<String>,
    pub consecutive_failures: u32,
}

// ── Tool definition ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub input_schema: Value,
}

// ── JSON-RPC 2.0 types ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// ── Tool execution request/response ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecuteRequest {
    pub tool_name: String,
    pub arguments: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecuteResponse {
    pub call_id: Option<String>,
    pub tool_name: String,
    pub result: Value,
    pub is_error: bool,
}

// ── Web UI API request/response types ────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct CreateMcpClientRequest {
    pub name: String,
    pub connection_type: McpConnectionType,
    pub connection_string: Option<String>,
    pub stdio_config: Option<McpStdioConfig>,
    #[serde(default)]
    pub auth_type: Option<McpAuthType>,
    #[serde(default)]
    pub headers: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    pub tools_to_execute: Option<Vec<String>>,
    #[serde(default)]
    pub is_ping_available: Option<bool>,
    #[serde(default)]
    pub tool_sync_interval_secs: Option<i32>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateMcpClientRequest {
    pub name: Option<String>,
    pub connection_type: Option<McpConnectionType>,
    pub connection_string: Option<String>,
    pub stdio_config: Option<McpStdioConfig>,
    pub auth_type: Option<McpAuthType>,
    pub headers: Option<std::collections::HashMap<String, String>>,
    pub tools_to_execute: Option<Vec<String>>,
    pub is_ping_available: Option<bool>,
    pub tool_sync_interval_secs: Option<i32>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpClientResponse {
    pub config: McpClientConfig,
    pub connection_state: McpConnectionState,
    pub tools: Vec<McpTool>,
    pub last_error: Option<String>,
}

/// Tool info for injecting into chat request tool lists.
#[derive(Debug, Clone, Serialize)]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
    pub client_name: String,
}
