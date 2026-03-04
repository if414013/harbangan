#![allow(dead_code)]

use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::stream::StreamExt;
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::routes::AppState;

use super::tool_manager;
use super::types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};

/// POST /mcp — JSON-RPC 2.0 handler for MCP server protocol.
///
/// Handles standard MCP methods:
/// - `initialize` — return server capabilities
/// - `notifications/initialized` — acknowledgement (no response)
/// - `tools/list` — list all aggregated tools
/// - `tools/call` — route tool call to appropriate client
/// - `ping` — health check
pub async fn mcp_post_handler(
    State(state): State<AppState>,
    Json(request): Json<JsonRpcRequest>,
) -> Result<Response, ApiError> {
    let mcp = state
        .mcp_manager
        .as_ref()
        .ok_or_else(|| ApiError::McpProtocolError("MCP Gateway not enabled".to_string()))?;

    let method = request.method.as_str();

    match method {
        "initialize" => {
            let response = JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: Some(json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {
                            "listChanged": true
                        }
                    },
                    "serverInfo": {
                        "name": "kiro-gateway",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                })),
                error: None,
                id: request.id,
            };
            Ok(Json(response).into_response())
        }

        "notifications/initialized" => {
            // Notification — no response needed
            Ok(StatusCode::OK.into_response())
        }

        "tools/list" => {
            let clients = mcp.clients_ref();
            let tools_result = tool_manager::get_all_tools_jsonrpc(&clients).await;

            let response = JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: Some(tools_result),
                error: None,
                id: request.id,
            };
            Ok(Json(response).into_response())
        }

        "tools/call" => {
            let params = request.params.unwrap_or(Value::Null);
            let tool_name = params
                .get("name")
                .and_then(|n| n.as_str())
                .ok_or_else(|| {
                    ApiError::McpProtocolError("Missing 'name' in tools/call params".to_string())
                })?
                .to_string();
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

            let config = state
                .config
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .clone();
            let timeout = config.mcp_tool_execution_timeout;

            let clients = mcp.clients_ref();
            let transports = mcp.transports_ref();

            match tool_manager::call_tool_jsonrpc(
                &clients,
                &transports,
                &tool_name,
                arguments,
                timeout,
            )
            .await
            {
                Ok(result) => {
                    // Format as MCP tool result with content array
                    let content = if let Some(text) = result.as_str() {
                        json!([{"type": "text", "text": text}])
                    } else {
                        let text = serde_json::to_string(&result).unwrap_or_default();
                        json!([{"type": "text", "text": text}])
                    };

                    let response = JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        result: Some(json!({
                            "content": content,
                            "isError": false,
                        })),
                        error: None,
                        id: request.id,
                    };
                    Ok(Json(response).into_response())
                }
                Err(e) => {
                    let response = JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        result: Some(json!({
                            "content": [{"type": "text", "text": e}],
                            "isError": true,
                        })),
                        error: None,
                        id: request.id,
                    };
                    Ok(Json(response).into_response())
                }
            }
        }

        "ping" => {
            let response = JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: Some(json!({})),
                error: None,
                id: request.id,
            };
            Ok(Json(response).into_response())
        }

        _ => {
            let response = JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32601,
                    message: format!("Method not found: {}", method),
                    data: None,
                }),
                id: request.id,
            };
            Ok(Json(response).into_response())
        }
    }
}

/// GET /mcp — SSE stream for MCP server protocol.
///
/// Sends an initial connection/opened event, then keeps
/// the connection alive with periodic ping events.
pub async fn mcp_sse_handler(State(state): State<AppState>) -> Result<Response, ApiError> {
    let _mcp = state
        .mcp_manager
        .as_ref()
        .ok_or_else(|| ApiError::McpProtocolError("MCP Gateway not enabled".to_string()))?;

    // Use tokio mpsc channel to build the SSE stream
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, std::io::Error>>(16);

    tokio::spawn(async move {
        // Send initial connection event
        let endpoint_data = json!({
            "jsonrpc": "2.0",
            "method": "connection/opened"
        });
        let initial = format!(
            "data: {}\n\n",
            serde_json::to_string(&endpoint_data).unwrap_or_default()
        );
        if tx.send(Ok(initial)).await.is_err() {
            return;
        }

        // Keep alive with periodic pings
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let ping = json!({"jsonrpc": "2.0", "method": "ping"});
            let msg = format!(
                "data: {}\n\n",
                serde_json::to_string(&ping).unwrap_or_default()
            );
            if tx.send(Ok(msg)).await.is_err() {
                break; // Client disconnected
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    let byte_stream = stream.map(|result| result.map(bytes::Bytes::from));

    let response = Response::builder()
        .status(200)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(Body::from_stream(byte_stream))
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to build SSE response: {}", e)))?;

    Ok(response)
}
