pub mod http;
pub mod sse;
pub mod stdio;

use async_trait::async_trait;
use thiserror::Error;

use super::types::{JsonRpcRequest, JsonRpcResponse};

// ── Transport error ─────────────────────────────────────────────────

#[derive(Error, Debug)]
pub enum McpTransportError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Request failed: {0}")]
    RequestFailed(String),

    #[error("Response parse error: {0}")]
    ParseError(String),

    #[error("Transport closed")]
    Closed,

    #[error("Request timed out")]
    Timeout,

    #[error("Process error: {0}")]
    ProcessError(String),
}

// ── Transport trait ─────────────────────────────────────────────────

/// Abstraction over MCP transport mechanisms (HTTP, SSE, STDIO).
///
/// Each transport sends JSON-RPC 2.0 requests and receives responses.
#[async_trait]
pub trait McpTransport: Send + Sync {
    /// Send a JSON-RPC 2.0 request and await the response.
    async fn send_request(
        &self,
        request: &JsonRpcRequest,
    ) -> Result<JsonRpcResponse, McpTransportError>;

    /// Check if the transport connection is alive.
    async fn is_connected(&self) -> bool;

    /// Establish the transport connection.
    async fn connect(&mut self) -> Result<(), McpTransportError>;

    /// Close the transport connection and clean up resources.
    async fn close(&mut self) -> Result<(), McpTransportError>;
}
