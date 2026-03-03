#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::transport::http::HttpTransport;
use super::transport::sse::SseTransport;
use super::transport::stdio::StdioTransport;
use super::transport::{McpTransport, McpTransportError};
use super::types::{
    JsonRpcRequest, McpClientConfig, McpClientState, McpConnectionState, McpConnectionType, McpTool,
};

/// Manages the lifecycle of MCP client connections.
///
/// Handles adding, removing, reconnecting, and updating clients.
/// Each client has an associated transport (HTTP, SSE, or STDIO) and
/// a runtime state tracking connection status and discovered tools.
pub struct ClientManager {
    clients: Arc<RwLock<HashMap<Uuid, McpClientState>>>,
    transports: Arc<RwLock<HashMap<Uuid, Arc<dyn McpTransport>>>>,
    default_timeout_secs: u64,
}

impl ClientManager {
    pub fn new(
        clients: Arc<RwLock<HashMap<Uuid, McpClientState>>>,
        default_timeout_secs: u64,
    ) -> Self {
        Self {
            clients,
            transports: Arc::new(RwLock::new(HashMap::new())),
            default_timeout_secs,
        }
    }

    /// Get a reference to the transports map (for sending requests externally).
    pub fn transports(&self) -> &Arc<RwLock<HashMap<Uuid, Arc<dyn McpTransport>>>> {
        &self.transports
    }

    /// Add a new client: create transport, connect, initialize, discover tools.
    pub async fn add_client(&self, config: &McpClientConfig) -> Result<()> {
        if !config.enabled {
            tracing::debug!(name = %config.name, "Skipping disabled client");
            // Still register it in disconnected state
            let state = McpClientState {
                config: config.clone(),
                connection_state: McpConnectionState::Disconnected,
                tools: HashMap::new(),
                last_error: None,
                consecutive_failures: 0,
            };
            self.clients.write().await.insert(config.id, state);
            return Ok(());
        }

        // Set connecting state
        let state = McpClientState {
            config: config.clone(),
            connection_state: McpConnectionState::Connecting,
            tools: HashMap::new(),
            last_error: None,
            consecutive_failures: 0,
        };
        self.clients.write().await.insert(config.id, state);

        // Create and connect transport
        match self.connect_client(config).await {
            Ok(transport) => {
                // Send initialize request
                let init_result = self.initialize_client(&*transport, config).await;
                if let Err(e) = &init_result {
                    tracing::warn!(
                        name = %config.name,
                        error = %e,
                        "MCP initialize handshake failed, continuing anyway"
                    );
                }

                // Discover tools
                let tools = self.discover_tools(&*transport, config).await;

                // Store transport
                self.transports
                    .write()
                    .await
                    .insert(config.id, transport);

                // Update state to connected with discovered tools
                if let Some(client) = self.clients.write().await.get_mut(&config.id) {
                    client.connection_state = McpConnectionState::Connected;
                    client.tools = tools;
                    client.last_error = None;
                    client.consecutive_failures = 0;
                }

                let tool_count = self.clients.read().await.get(&config.id)
                    .map(|c| c.tools.len()).unwrap_or(0);
                tracing::info!(
                    name = %config.name,
                    tools = tool_count,
                    "MCP client connected"
                );
                Ok(())
            }
            Err(e) => {
                let error_msg = e.to_string();
                if let Some(client) = self.clients.write().await.get_mut(&config.id) {
                    client.connection_state = McpConnectionState::Error;
                    client.last_error = Some(error_msg.clone());
                }
                tracing::error!(
                    name = %config.name,
                    error = %error_msg,
                    "Failed to connect MCP client"
                );
                Err(anyhow::anyhow!("Failed to connect: {}", error_msg))
            }
        }
    }

    /// Remove a client: close transport, remove state.
    pub async fn remove_client(&self, id: Uuid) -> Result<()> {
        // Close transport (Arc::get_mut succeeds since we just removed the only reference)
        if let Some(mut transport) = self.transports.write().await.remove(&id) {
            if let Some(t) = Arc::get_mut(&mut transport) {
                if let Err(e) = t.close().await {
                    tracing::warn!(id = %id, error = %e, "Error closing transport during removal");
                }
            }
        }

        // Remove state
        let removed = self.clients.write().await.remove(&id);
        if let Some(client) = &removed {
            tracing::info!(name = %client.config.name, "MCP client removed");
        }

        Ok(())
    }

    /// Reconnect an existing client: close current transport, create new one.
    pub async fn reconnect_client(&self, id: Uuid) -> Result<()> {
        let config = {
            let clients = self.clients.read().await;
            clients
                .get(&id)
                .map(|c| c.config.clone())
                .ok_or_else(|| anyhow::anyhow!("Client not found: {}", id))?
        };

        // Close existing transport
        if let Some(mut transport) = self.transports.write().await.remove(&id) {
            if let Some(t) = Arc::get_mut(&mut transport) {
                let _ = t.close().await;
            }
        }

        // Set connecting state
        if let Some(client) = self.clients.write().await.get_mut(&id) {
            client.connection_state = McpConnectionState::Connecting;
            client.last_error = None;
        }

        // Reconnect
        match self.connect_client(&config).await {
            Ok(transport) => {
                let _ = self.initialize_client(&*transport, &config).await;
                let tools = self.discover_tools(&*transport, &config).await;

                self.transports.write().await.insert(id, transport);

                if let Some(client) = self.clients.write().await.get_mut(&id) {
                    client.connection_state = McpConnectionState::Connected;
                    client.tools = tools;
                    client.last_error = None;
                    client.consecutive_failures = 0;
                }

                tracing::info!(name = %config.name, "MCP client reconnected");
                Ok(())
            }
            Err(e) => {
                let error_msg = e.to_string();
                if let Some(client) = self.clients.write().await.get_mut(&id) {
                    client.connection_state = McpConnectionState::Error;
                    client.last_error = Some(error_msg.clone());
                }
                Err(anyhow::anyhow!("Reconnect failed: {}", error_msg))
            }
        }
    }

    /// Update a client's config. Reconnects if connection params changed.
    pub async fn update_client(&self, config: &McpClientConfig) -> Result<()> {
        let needs_reconnect = {
            let clients = self.clients.read().await;
            if let Some(existing) = clients.get(&config.id) {
                let old = &existing.config;
                old.connection_type != config.connection_type
                    || old.connection_string != config.connection_string
                    || old.stdio_config.is_some() != config.stdio_config.is_some()
                    || old.headers != config.headers
                    || old.enabled != config.enabled
            } else {
                true
            }
        };

        // Update the config in state
        if let Some(client) = self.clients.write().await.get_mut(&config.id) {
            client.config = config.clone();
        }

        if needs_reconnect {
            if config.enabled {
                self.reconnect_client(config.id).await?;
            } else {
                // Disable: close transport, set disconnected
                if let Some(mut transport) = self.transports.write().await.remove(&config.id) {
                    if let Some(t) = Arc::get_mut(&mut transport) {
                        let _ = t.close().await;
                    }
                }
                if let Some(client) = self.clients.write().await.get_mut(&config.id) {
                    client.connection_state = McpConnectionState::Disconnected;
                    client.tools.clear();
                }
            }
        }

        Ok(())
    }

    /// Shut down all client connections.
    pub async fn shutdown_all(&self) {
        let ids: Vec<Uuid> = self.transports.read().await.keys().copied().collect();
        for id in ids {
            if let Some(mut transport) = self.transports.write().await.remove(&id) {
                if let Some(t) = Arc::get_mut(&mut transport) {
                    let _ = t.close().await;
                }
            }
        }
        self.clients.write().await.clear();
        tracing::info!("All MCP client connections shut down");
    }

    // ── Private helpers ─────────────────────────────────────────────

    /// Create and connect a transport based on the client's connection type.
    async fn connect_client(
        &self,
        config: &McpClientConfig,
    ) -> Result<Arc<dyn McpTransport>, McpTransportError> {
        match config.connection_type {
            McpConnectionType::Http => {
                let url = config
                    .connection_string
                    .clone()
                    .unwrap_or_default();
                let mut transport =
                    HttpTransport::new(url, config.headers.clone(), self.default_timeout_secs);
                transport.connect().await?;
                Ok(Arc::new(transport))
            }
            McpConnectionType::Sse => {
                let url = config
                    .connection_string
                    .clone()
                    .unwrap_or_default();
                let mut transport =
                    SseTransport::new(url, config.headers.clone(), self.default_timeout_secs);
                transport.connect().await?;
                Ok(Arc::new(transport))
            }
            McpConnectionType::Stdio => {
                let stdio_config = config
                    .stdio_config
                    .clone()
                    .ok_or_else(|| {
                        McpTransportError::ConnectionFailed(
                            "STDIO config required for stdio connection type".to_string(),
                        )
                    })?;
                let mut transport =
                    StdioTransport::new(stdio_config, self.default_timeout_secs);
                transport.connect().await?;
                Ok(Arc::new(transport))
            }
        }
    }

    /// Send the MCP `initialize` handshake.
    async fn initialize_client(
        &self,
        transport: &dyn McpTransport,
        config: &McpClientConfig,
    ) -> Result<()> {
        let init_request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "kiro-gateway",
                    "version": "1.0"
                }
            })),
            id: Some(serde_json::json!(0)),
        };

        let response = transport
            .send_request(&init_request)
            .await
            .map_err(|e| anyhow::anyhow!("Initialize failed: {}", e))?;

        if let Some(error) = &response.error {
            tracing::warn!(
                name = %config.name,
                code = error.code,
                message = %error.message,
                "MCP server returned error on initialize"
            );
        }

        // Send initialized notification (no id = notification)
        let initialized_notification = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "notifications/initialized".to_string(),
            params: None,
            id: None,
        };
        // Fire-and-forget; notification has no response
        let _ = transport.send_request(&initialized_notification).await;

        Ok(())
    }

    /// Discover tools via `tools/list` JSON-RPC call.
    async fn discover_tools(
        &self,
        transport: &dyn McpTransport,
        config: &McpClientConfig,
    ) -> HashMap<String, McpTool> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "tools/list".to_string(),
            params: None,
            id: Some(serde_json::json!(1)),
        };

        match transport.send_request(&request).await {
            Ok(response) => {
                if let Some(result) = response.result {
                    if let Some(tools_array) = result.get("tools").and_then(|t| t.as_array()) {
                        let mut tools = HashMap::new();
                        for tool_val in tools_array {
                            if let Ok(tool) = serde_json::from_value::<McpTool>(tool_val.clone()) {
                                tools.insert(tool.name.clone(), tool);
                            }
                        }
                        tracing::debug!(
                            name = %config.name,
                            count = tools.len(),
                            "Discovered MCP tools"
                        );
                        return tools;
                    }
                }
                tracing::debug!(name = %config.name, "No tools discovered");
                HashMap::new()
            }
            Err(e) => {
                tracing::warn!(
                    name = %config.name,
                    error = %e,
                    "Failed to discover tools"
                );
                HashMap::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::types::{McpAuthType, McpConnectionType};
    use chrono::Utc;

    fn test_client_config() -> McpClientConfig {
        McpClientConfig {
            id: Uuid::new_v4(),
            name: "test-client".to_string(),
            connection_type: McpConnectionType::Http,
            connection_string: Some("https://example.com/mcp".to_string()),
            stdio_config: None,
            auth_type: McpAuthType::None,
            headers: HashMap::new(),
            tools_to_execute: vec!["*".to_string()],
            is_ping_available: true,
            tool_sync_interval_secs: 0,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_client_manager_creation() {
        let clients = Arc::new(RwLock::new(HashMap::new()));
        let manager = ClientManager::new(clients, 30);
        assert_eq!(manager.default_timeout_secs, 30);
    }

    #[tokio::test]
    async fn test_add_disabled_client() {
        let clients = Arc::new(RwLock::new(HashMap::new()));
        let manager = ClientManager::new(clients.clone(), 30);

        let mut config = test_client_config();
        config.enabled = false;

        let result = manager.add_client(&config).await;
        assert!(result.is_ok());

        let clients_map = clients.read().await;
        let state = clients_map.get(&config.id).unwrap();
        assert_eq!(state.connection_state, McpConnectionState::Disconnected);
    }

    #[tokio::test]
    async fn test_remove_nonexistent_client() {
        let clients = Arc::new(RwLock::new(HashMap::new()));
        let manager = ClientManager::new(clients, 30);

        let result = manager.remove_client(Uuid::new_v4()).await;
        assert!(result.is_ok()); // No-op, not an error
    }

    #[tokio::test]
    async fn test_reconnect_nonexistent_client() {
        let clients = Arc::new(RwLock::new(HashMap::new()));
        let manager = ClientManager::new(clients, 30);

        let result = manager.reconnect_client(Uuid::new_v4()).await;
        assert!(result.is_err()); // Should fail - client not found
    }

    #[tokio::test]
    async fn test_shutdown_all() {
        let clients = Arc::new(RwLock::new(HashMap::new()));
        let manager = ClientManager::new(clients.clone(), 30);

        // Add a disabled client (doesn't need real connection)
        let mut config = test_client_config();
        config.enabled = false;
        manager.add_client(&config).await.unwrap();

        assert!(!clients.read().await.is_empty());
        manager.shutdown_all().await;
        assert!(clients.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_update_client_with_disabled_toggle() {
        let clients = Arc::new(RwLock::new(HashMap::new()));
        let manager = ClientManager::new(clients.clone(), 30);

        // Start with disabled client
        let mut config = test_client_config();
        config.enabled = false;
        manager.add_client(&config).await.unwrap();

        // "Disable" it again (no-op since already disabled)
        config.enabled = false;
        let result = manager.update_client(&config).await;
        assert!(result.is_ok());

        let state = clients.read().await;
        let client = state.get(&config.id).unwrap();
        assert_eq!(client.connection_state, McpConnectionState::Disconnected);
    }
}
