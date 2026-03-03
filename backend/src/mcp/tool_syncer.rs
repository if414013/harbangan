#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::transport::McpTransport;
use super::types::{JsonRpcRequest, McpClientState, McpConnectionState, McpTool};

/// Start a tool syncer background task for a specific client.
///
/// Periodically sends `tools/list` to the MCP server and diffs the result
/// against the current tool map. On change, updates the tool map atomically.
/// On failure, keeps existing tools and logs a warning.
///
/// Returns a `JoinHandle` that can be used to stop the syncer.
pub fn start_tool_syncer(
    client_id: Uuid,
    clients: Arc<RwLock<HashMap<Uuid, McpClientState>>>,
    transports: Arc<RwLock<HashMap<Uuid, Arc<dyn McpTransport>>>>,
    interval: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        // Skip the first immediate tick
        ticker.tick().await;

        loop {
            ticker.tick().await;

            // Check if client still exists and is connected
            let client_name = {
                let clients_map = clients.read().await;
                match clients_map.get(&client_id) {
                    Some(state) => {
                        if state.connection_state != McpConnectionState::Connected {
                            tracing::debug!(
                                name = %state.config.name,
                                "Skipping tool sync: client not connected"
                            );
                            continue;
                        }
                        state.config.name.clone()
                    }
                    None => {
                        tracing::debug!(id = %client_id, "Tool syncer stopping: client removed");
                        return;
                    }
                }
            };

            // Check if transport exists
            if !transports.read().await.contains_key(&client_id) {
                tracing::debug!(name = %client_name, "Skipping tool sync: no transport");
                continue;
            }

            // Perform tool sync
            match discover_tools(&transports, client_id).await {
                Ok(new_tools) => {
                    let mut clients_map = clients.write().await;
                    if let Some(state) = clients_map.get_mut(&client_id) {
                        let old_count = state.tools.len();
                        let new_count = new_tools.len();

                        state.tools = new_tools;

                        if old_count != new_count {
                            tracing::info!(
                                name = %client_name,
                                old = old_count,
                                new = new_count,
                                "Tool sync: tool count changed"
                            );
                        } else {
                            tracing::debug!(
                                name = %client_name,
                                count = new_count,
                                "Tool sync completed (no change)"
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        name = %client_name,
                        error = %e,
                        "Tool sync failed, keeping existing tools"
                    );
                }
            }
        }
    })
}

/// Send `tools/list` JSON-RPC and parse the response into a tool map.
async fn discover_tools(
    transports: &RwLock<HashMap<Uuid, Arc<dyn McpTransport>>>,
    client_id: Uuid,
) -> Result<HashMap<String, McpTool>, String> {
    let sync_timeout = Duration::from_secs(10);

    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "tools/list".to_string(),
        params: None,
        id: Some(serde_json::json!("sync")),
    };

    // Clone transport Arc and release lock before the potentially long await
    let transport = {
        let transports_map = transports.read().await;
        Arc::clone(
            transports_map
                .get(&client_id)
                .ok_or_else(|| "No transport".to_string())?,
        )
    };

    let response = tokio::time::timeout(sync_timeout, transport.send_request(&request))
        .await
        .map_err(|_| "Tool sync timed out".to_string())?
        .map_err(|e| format!("Transport error: {}", e))?;

    if let Some(error) = &response.error {
        return Err(format!("JSON-RPC error: {}", error.message));
    }

    let result = response.result.unwrap_or(serde_json::Value::Null);
    let tools_array = result
        .get("tools")
        .and_then(|t| t.as_array())
        .cloned()
        .unwrap_or_default();

    let mut tools = HashMap::new();
    for tool_val in tools_array {
        if let Ok(tool) = serde_json::from_value::<McpTool>(tool_val) {
            tools.insert(tool.name.clone(), tool);
        }
    }

    Ok(tools)
}
