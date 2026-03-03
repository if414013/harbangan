#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use axum::http::HeaderMap;
use serde_json::Value;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::transport::McpTransport;
use super::types::{
    JsonRpcRequest, McpClientState, McpConnectionState, McpToolInfo,
};

/// Get available tools from all connected clients, applying two-tier filtering.
///
/// Tools are namespaced as `{clientName}_{toolName}` for chat contexts.
/// Filtering is applied at two levels:
/// 1. Client config `tools_to_execute` field (baseline whitelist)
/// 2. Request headers `x-kgw-mcp-include-clients` / `x-kgw-mcp-include-tools` (per-request)
///
/// Returns tools formatted for injection into chat completion request `tools` arrays.
pub async fn get_available_tools(
    clients: &RwLock<HashMap<Uuid, McpClientState>>,
    headers: &HeaderMap,
) -> Vec<Value> {
    let clients_map = clients.read().await;

    // Parse request-level header filters
    let include_clients = parse_header_list(headers, "x-kgw-mcp-include-clients");
    let include_tools = parse_header_list(headers, "x-kgw-mcp-include-tools");

    let mut result = Vec::new();

    for state in clients_map.values() {
        // Skip disconnected clients
        if state.connection_state != McpConnectionState::Connected {
            continue;
        }

        // Skip disabled clients
        if !state.config.enabled {
            continue;
        }

        let client_name = &state.config.name;

        // Tier 2a: Request-level client filter
        if let Some(ref allowed_clients) = include_clients {
            if !allowed_clients.contains(&"*".to_string())
                && !allowed_clients.contains(client_name)
            {
                continue;
            }
        }

        for (tool_name, tool) in &state.tools {
            // Tier 1: Client config filter (tools_to_execute)
            if !is_tool_allowed_by_config(tool_name, &state.config.tools_to_execute) {
                continue;
            }

            // Tier 2b: Request-level tool filter
            // Format: "clientName-toolName" (hyphen separator for header filtering)
            if let Some(ref allowed_tools) = include_tools {
                let header_tool_name = format!("{}-{}", client_name, tool_name);
                let wildcard = format!("{}-*", client_name);
                if !allowed_tools.contains(&"*".to_string())
                    && !allowed_tools.contains(&wildcard)
                    && !allowed_tools.contains(&header_tool_name)
                {
                    continue;
                }
            }

            // Build namespaced tool name: clientName_toolName (underscore for chat context)
            let prefixed_name = format!("{}_{}", client_name, tool_name);

            // Convert to OpenAI tool format
            let tool_json = serde_json::json!({
                "type": "function",
                "function": {
                    "name": prefixed_name,
                    "description": tool.description.as_deref().unwrap_or(""),
                    "parameters": tool.input_schema,
                }
            });

            result.push(tool_json);
        }
    }

    result
}

/// Get tool info for all available tools (used by pipeline integration).
pub async fn get_tool_info_list(
    clients: &RwLock<HashMap<Uuid, McpClientState>>,
    headers: &HeaderMap,
) -> Vec<McpToolInfo> {
    let clients_map = clients.read().await;

    let include_clients = parse_header_list(headers, "x-kgw-mcp-include-clients");
    let include_tools = parse_header_list(headers, "x-kgw-mcp-include-tools");

    let mut result = Vec::new();

    for state in clients_map.values() {
        if state.connection_state != McpConnectionState::Connected || !state.config.enabled {
            continue;
        }

        let client_name = &state.config.name;

        if let Some(ref allowed_clients) = include_clients {
            if !allowed_clients.contains(&"*".to_string())
                && !allowed_clients.contains(client_name)
            {
                continue;
            }
        }

        for (tool_name, tool) in &state.tools {
            if !is_tool_allowed_by_config(tool_name, &state.config.tools_to_execute) {
                continue;
            }

            if let Some(ref allowed_tools) = include_tools {
                let header_tool_name = format!("{}-{}", client_name, tool_name);
                let wildcard = format!("{}-*", client_name);
                if !allowed_tools.contains(&"*".to_string())
                    && !allowed_tools.contains(&wildcard)
                    && !allowed_tools.contains(&header_tool_name)
                {
                    continue;
                }
            }

            let prefixed_name = format!("{}_{}", client_name, tool_name);

            result.push(McpToolInfo {
                name: prefixed_name,
                description: tool.description.clone(),
                input_schema: tool.input_schema.clone(),
                client_name: client_name.clone(),
            });
        }
    }

    result
}

/// Execute a tool by its prefixed name (`clientName_toolName`).
///
/// 1. Split prefixed name on first `_` to get client name + tool name
/// 2. Verify client is connected
/// 3. Verify tool passes both filter tiers
/// 4. Send `tools/call` JSON-RPC request via transport
/// 5. Return result
pub async fn execute_tool(
    clients: &RwLock<HashMap<Uuid, McpClientState>>,
    transports: &RwLock<HashMap<Uuid, Arc<dyn McpTransport>>>,
    prefixed_name: &str,
    arguments: Value,
    headers: &HeaderMap,
    timeout_secs: u64,
) -> Result<Value, String> {
    // Split on first underscore to get client_name and tool_name
    let (client_name, tool_name) = split_prefixed_tool_name(prefixed_name)?;

    // Find the client
    let client_id = {
        let clients_map = clients.read().await;
        let entry = clients_map
            .iter()
            .find(|(_, s)| s.config.name == client_name)
            .ok_or_else(|| format!("MCP client '{}' not found", client_name))?;

        let state = entry.1;

        // Verify connected
        if state.connection_state != McpConnectionState::Connected {
            return Err(format!("MCP client '{}' is not connected", client_name));
        }

        // Verify tool exists
        if !state.tools.contains_key(tool_name) {
            return Err(format!(
                "Tool '{}' not found on client '{}'",
                tool_name, client_name
            ));
        }

        // Tier 1: Config-level filter
        if !is_tool_allowed_by_config(tool_name, &state.config.tools_to_execute) {
            return Err(format!(
                "Tool '{}' is not allowed by client configuration",
                tool_name
            ));
        }

        *entry.0
    };

    // Tier 2: Request-level filter
    let include_tools = parse_header_list(headers, "x-kgw-mcp-include-tools");
    if let Some(ref allowed_tools) = include_tools {
        let header_tool_name = format!("{}-{}", client_name, tool_name);
        let wildcard = format!("{}-*", client_name);
        if !allowed_tools.contains(&"*".to_string())
            && !allowed_tools.contains(&wildcard)
            && !allowed_tools.contains(&header_tool_name)
        {
            return Err(format!(
                "Tool '{}' is not allowed by request filters",
                tool_name
            ));
        }
    }

    // Build JSON-RPC tools/call request
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": tool_name,
            "arguments": arguments,
        })),
        id: Some(serde_json::json!(Uuid::new_v4().to_string())),
    };

    // Clone transport Arc and release lock before the potentially long await
    let transport = {
        let transports_map = transports.read().await;
        Arc::clone(
            transports_map
                .get(&client_id)
                .ok_or_else(|| format!("No transport for client '{}'", client_name))?,
        )
    };

    let response = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        transport.send_request(&request),
    )
    .await
    .map_err(|_| format!("Tool execution timed out after {}s", timeout_secs))?
    .map_err(|e| format!("Transport error: {}", e))?;

    // Check for JSON-RPC error
    if let Some(error) = response.error {
        return Err(format!("Tool execution error: {}", error.message));
    }

    // Extract result
    Ok(response.result.unwrap_or(Value::Null))
}

/// Get all tools from all connected clients in JSON-RPC format (for /mcp server tools/list).
pub async fn get_all_tools_jsonrpc(
    clients: &RwLock<HashMap<Uuid, McpClientState>>,
) -> Value {
    let clients_map = clients.read().await;
    let mut tools = Vec::new();

    for state in clients_map.values() {
        if state.connection_state != McpConnectionState::Connected || !state.config.enabled {
            continue;
        }

        let client_name = &state.config.name;

        for (tool_name, tool) in &state.tools {
            if !is_tool_allowed_by_config(tool_name, &state.config.tools_to_execute) {
                continue;
            }

            let prefixed_name = format!("{}_{}", client_name, tool_name);
            tools.push(serde_json::json!({
                "name": prefixed_name,
                "description": tool.description.as_deref().unwrap_or(""),
                "inputSchema": tool.input_schema,
            }));
        }
    }

    serde_json::json!({ "tools": tools })
}

/// Route a `tools/call` JSON-RPC request to the appropriate client (for /mcp server).
pub async fn call_tool_jsonrpc(
    clients: &RwLock<HashMap<Uuid, McpClientState>>,
    transports: &RwLock<HashMap<Uuid, Arc<dyn McpTransport>>>,
    name: &str,
    arguments: Value,
    timeout_secs: u64,
) -> Result<Value, String> {
    let empty_headers = HeaderMap::new();
    execute_tool(clients, transports, name, arguments, &empty_headers, timeout_secs).await
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Check if a tool is allowed by the client's `tools_to_execute` config.
fn is_tool_allowed_by_config(tool_name: &str, tools_to_execute: &[String]) -> bool {
    if tools_to_execute.is_empty() {
        return false;
    }
    tools_to_execute.iter().any(|t| t == "*" || t == tool_name)
}

/// Parse a comma-separated header value into a list of trimmed strings.
fn parse_header_list(headers: &HeaderMap, key: &str) -> Option<Vec<String>> {
    headers.get(key).and_then(|v| v.to_str().ok()).map(|s| {
        s.split(',')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect()
    })
}

/// Validate that a client name does not contain underscores.
///
/// Client names are used as prefixes in namespaced tool names (`clientName_toolName`),
/// so underscores in client names would make the split ambiguous.
pub fn validate_client_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Client name cannot be empty".to_string());
    }
    if name.contains('_') {
        return Err(
            "Client name cannot contain underscores (used as tool name separator)".to_string(),
        );
    }
    Ok(())
}

/// Split a prefixed tool name `clientName_toolName` on the first underscore.
fn split_prefixed_tool_name(prefixed: &str) -> Result<(&str, &str), String> {
    let idx = prefixed
        .find('_')
        .ok_or_else(|| format!("Invalid tool name '{}': expected 'clientName_toolName'", prefixed))?;
    Ok((&prefixed[..idx], &prefixed[idx + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_prefixed_tool_name() {
        let (client, tool) = split_prefixed_tool_name("myClient_myTool").unwrap();
        assert_eq!(client, "myClient");
        assert_eq!(tool, "myTool");
    }

    #[test]
    fn test_split_prefixed_tool_name_with_underscores() {
        let (client, tool) = split_prefixed_tool_name("myClient_my_complex_tool").unwrap();
        assert_eq!(client, "myClient");
        assert_eq!(tool, "my_complex_tool");
    }

    #[test]
    fn test_split_prefixed_tool_name_invalid() {
        assert!(split_prefixed_tool_name("nounderscores").is_err());
    }

    #[test]
    fn test_is_tool_allowed_by_config_wildcard() {
        assert!(is_tool_allowed_by_config("any_tool", &["*".to_string()]));
    }

    #[test]
    fn test_is_tool_allowed_by_config_specific() {
        let allowed = vec!["tool_a".to_string(), "tool_b".to_string()];
        assert!(is_tool_allowed_by_config("tool_a", &allowed));
        assert!(!is_tool_allowed_by_config("tool_c", &allowed));
    }

    #[test]
    fn test_is_tool_allowed_by_config_empty() {
        assert!(!is_tool_allowed_by_config("any", &[]));
    }

    #[test]
    fn test_parse_header_list() {
        let mut headers = HeaderMap::new();
        headers.insert("x-kgw-mcp-include-clients", "client1, client2".parse().unwrap());

        let result = parse_header_list(&headers, "x-kgw-mcp-include-clients");
        assert_eq!(result, Some(vec!["client1".to_string(), "client2".to_string()]));
    }

    #[test]
    fn test_parse_header_list_missing() {
        let headers = HeaderMap::new();
        assert!(parse_header_list(&headers, "x-kgw-mcp-include-clients").is_none());
    }

    #[test]
    fn test_validate_client_name_valid() {
        assert!(validate_client_name("myClient").is_ok());
        assert!(validate_client_name("client-with-dashes").is_ok());
    }

    #[test]
    fn test_validate_client_name_underscore() {
        assert!(validate_client_name("my_client").is_err());
    }

    #[test]
    fn test_validate_client_name_empty() {
        assert!(validate_client_name("").is_err());
    }
}
