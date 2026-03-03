#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::transport::McpTransport;
use super::types::{JsonRpcRequest, McpClientState, McpConnectionState};

/// Start a health monitor background task for a specific client.
///
/// The monitor periodically checks client health via `ping` (if available)
/// or `tools/list` as fallback. On consecutive failures exceeding the
/// threshold, it marks the client as `Error` state.
///
/// Returns a `JoinHandle` that can be used to stop the monitor.
pub fn start_health_monitor(
    client_id: Uuid,
    clients: Arc<RwLock<HashMap<Uuid, McpClientState>>>,
    transports: Arc<RwLock<HashMap<Uuid, Arc<dyn McpTransport>>>>,
    interval: Duration,
    max_consecutive_failures: u32,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        // Skip the first immediate tick
        ticker.tick().await;

        loop {
            ticker.tick().await;

            // Check if client still exists
            let (is_ping_available, client_name) = {
                let clients_map = clients.read().await;
                match clients_map.get(&client_id) {
                    Some(state) => (
                        state.config.is_ping_available,
                        state.config.name.clone(),
                    ),
                    None => {
                        tracing::debug!(id = %client_id, "Health monitor stopping: client removed");
                        return;
                    }
                }
            };

            // Check if transport exists
            let has_transport = transports.read().await.contains_key(&client_id);
            if !has_transport {
                // No transport — mark disconnected, increment failures
                let mut clients_map = clients.write().await;
                if let Some(state) = clients_map.get_mut(&client_id) {
                    state.consecutive_failures += 1;
                    if state.consecutive_failures >= max_consecutive_failures
                        && state.connection_state != McpConnectionState::Error
                    {
                        state.connection_state = McpConnectionState::Error;
                        state.last_error = Some("Health check: no transport".to_string());
                        tracing::warn!(
                            name = %client_name,
                            failures = state.consecutive_failures,
                            "MCP client marked as error: no transport"
                        );
                    }
                }
                continue;
            }

            // Perform health check
            let check_result = perform_health_check(
                &transports,
                client_id,
                is_ping_available,
            )
            .await;

            // Update state based on result
            let mut clients_map = clients.write().await;
            if let Some(state) = clients_map.get_mut(&client_id) {
                match check_result {
                    Ok(()) => {
                        // Health check passed
                        if state.consecutive_failures > 0 {
                            tracing::debug!(
                                name = %client_name,
                                "MCP client health check recovered"
                            );
                        }
                        state.consecutive_failures = 0;
                        if state.connection_state != McpConnectionState::Connected {
                            state.connection_state = McpConnectionState::Connected;
                            state.last_error = None;
                        }
                    }
                    Err(e) => {
                        // Health check failed
                        state.consecutive_failures += 1;
                        tracing::debug!(
                            name = %client_name,
                            failures = state.consecutive_failures,
                            error = %e,
                            "MCP client health check failed"
                        );

                        if state.consecutive_failures >= max_consecutive_failures
                            && state.connection_state != McpConnectionState::Error
                        {
                            state.connection_state = McpConnectionState::Error;
                            state.last_error = Some(format!("Health check failed: {}", e));
                            tracing::warn!(
                                name = %client_name,
                                failures = state.consecutive_failures,
                                "MCP client marked as error after {} consecutive failures",
                                max_consecutive_failures
                            );
                        }
                    }
                }
            }
        }
    })
}

/// Perform a single health check against a client's transport.
///
/// Uses `ping` if available, falls back to `tools/list`.
async fn perform_health_check(
    transports: &RwLock<HashMap<Uuid, Arc<dyn McpTransport>>>,
    client_id: Uuid,
    is_ping_available: bool,
) -> Result<(), String> {
    let check_timeout = Duration::from_secs(5);

    let request = if is_ping_available {
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "ping".to_string(),
            params: None,
            id: Some(serde_json::json!("health")),
        }
    } else {
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "tools/list".to_string(),
            params: None,
            id: Some(serde_json::json!("health")),
        }
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

    let result = tokio::time::timeout(check_timeout, transport.send_request(&request))
        .await
        .map_err(|_| "Health check timed out".to_string())?
        .map_err(|e| format!("Health check transport error: {}", e))?;

    if let Some(error) = &result.error {
        return Err(format!("Health check JSON-RPC error: {}", error.message));
    }

    Ok(())
}
