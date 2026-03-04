use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::ApiError;
use crate::routes::{AppState, SessionInfo};

use super::types::{
    CreateMcpClientRequest, McpAuthType, McpClientConfig, McpConnectionType, UpdateMcpClientRequest,
};

// ── Helpers ─────────────────────────────────────────────────────────

/// Get the McpManager from AppState or return an error.
fn require_mcp_manager(state: &AppState) -> Result<&std::sync::Arc<super::McpManager>, ApiError> {
    state
        .mcp_manager
        .as_ref()
        .ok_or_else(|| ApiError::ConfigError("MCP Gateway not enabled".to_string()))
}

/// Serialize a client response to JSON.
/// STDIO environment variables are omitted from the response for security.
fn client_to_json(c: &super::types::McpClientResponse) -> Value {
    // For STDIO configs, redact environment variable values
    let safe_stdio_config = c.config.stdio_config.as_ref().map(|cfg| {
        json!({
            "command": cfg.command,
            "args": cfg.args,
            "envs": cfg.envs.keys().map(|k| (k.clone(), "***".to_string()))
                .collect::<std::collections::HashMap<String, String>>(),
        })
    });

    json!({
        "id": c.config.id,
        "name": c.config.name,
        "connection_type": c.config.connection_type,
        "connection_string": c.config.connection_string,
        "stdio_config": safe_stdio_config,
        "auth_type": c.config.auth_type,
        "tools_to_execute": c.config.tools_to_execute,
        "is_ping_available": c.config.is_ping_available,
        "tool_sync_interval_secs": c.config.tool_sync_interval_secs,
        "enabled": c.config.enabled,
        "created_at": c.config.created_at.to_rfc3339(),
        "updated_at": c.config.updated_at.to_rfc3339(),
        "connection_state": c.connection_state,
        "tools": c.tools,
        "tools_count": c.tools.len(),
        "last_error": c.last_error,
    })
}

// ── Handlers ────────────────────────────────────────────────────────

/// GET /admin/mcp/clients — List all MCP clients with state + tools.
async fn list_clients(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionInfo>,
) -> Result<Json<Value>, ApiError> {
    let mcp = require_mcp_manager(&state)?;
    let clients = mcp.get_clients().await;

    let items: Vec<Value> = clients.iter().map(client_to_json).collect();
    Ok(Json(json!({ "clients": items, "count": items.len() })))
}

/// POST /admin/mcp/clients — Register a new MCP connection.
async fn create_client(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionInfo>,
    Json(body): Json<CreateMcpClientRequest>,
) -> Result<Json<Value>, ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::ValidationError(
            "Client name cannot be empty".to_string(),
        ));
    }

    // Validate connection_type + connection_string/stdio_config
    match body.connection_type {
        McpConnectionType::Http | McpConnectionType::Sse => {
            if body
                .connection_string
                .as_ref()
                .is_none_or(|s| s.trim().is_empty())
            {
                return Err(ApiError::ValidationError(
                    "connection_string is required for HTTP/SSE connections".to_string(),
                ));
            }
        }
        McpConnectionType::Stdio => {
            if body.stdio_config.is_none() {
                return Err(ApiError::ValidationError(
                    "stdio_config is required for STDIO connections".to_string(),
                ));
            }
        }
    }

    let mcp = require_mcp_manager(&state)?;

    let config = McpClientConfig {
        id: Uuid::new_v4(),
        name: body.name.trim().to_string(),
        connection_type: body.connection_type,
        connection_string: body.connection_string,
        stdio_config: body.stdio_config,
        auth_type: body.auth_type.unwrap_or(McpAuthType::None),
        headers: body.headers.unwrap_or_default(),
        tools_to_execute: body
            .tools_to_execute
            .unwrap_or_else(|| vec!["*".to_string()]),
        is_ping_available: body.is_ping_available.unwrap_or(true),
        tool_sync_interval_secs: body.tool_sync_interval_secs.unwrap_or(0),
        enabled: body.enabled.unwrap_or(true),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    mcp.add_client(&config).await.map_err(ApiError::Internal)?;

    tracing::info!(client_id = %config.id, name = %config.name, "mcp_client_created");

    // Return the newly created client
    let response = mcp
        .get_client(config.id)
        .await
        .map(|c| client_to_json(&c))
        .unwrap_or_else(|| json!({"id": config.id, "name": config.name}));

    Ok(Json(response))
}

/// PUT /admin/mcp/clients/:id — Update an existing MCP client config.
async fn update_client(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionInfo>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateMcpClientRequest>,
) -> Result<Json<Value>, ApiError> {
    let mcp = require_mcp_manager(&state)?;

    // Get existing config
    let existing = mcp
        .get_client(id)
        .await
        .ok_or_else(|| ApiError::McpClientNotFound(format!("Client '{}' not found", id)))?;

    // Merge updates onto existing config
    let updated = McpClientConfig {
        id,
        name: body
            .name
            .map(|n| n.trim().to_string())
            .unwrap_or(existing.config.name),
        connection_type: body
            .connection_type
            .unwrap_or(existing.config.connection_type),
        connection_string: body.connection_string.or(existing.config.connection_string),
        stdio_config: body.stdio_config.or(existing.config.stdio_config),
        auth_type: body.auth_type.unwrap_or(existing.config.auth_type),
        headers: body.headers.unwrap_or(existing.config.headers),
        tools_to_execute: body
            .tools_to_execute
            .unwrap_or(existing.config.tools_to_execute),
        is_ping_available: body
            .is_ping_available
            .unwrap_or(existing.config.is_ping_available),
        tool_sync_interval_secs: body
            .tool_sync_interval_secs
            .unwrap_or(existing.config.tool_sync_interval_secs),
        enabled: body.enabled.unwrap_or(existing.config.enabled),
        created_at: existing.config.created_at,
        updated_at: chrono::Utc::now(),
    };

    mcp.update_client(&updated)
        .await
        .map_err(ApiError::Internal)?;

    tracing::info!(client_id = %id, "mcp_client_updated");

    // Return updated client
    let response = mcp
        .get_client(id)
        .await
        .map(|c| client_to_json(&c))
        .unwrap_or_else(|| json!({"id": id}));

    Ok(Json(response))
}

/// DELETE /admin/mcp/clients/:id — Remove a client and disconnect.
async fn delete_client(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionInfo>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let mcp = require_mcp_manager(&state)?;

    // Verify client exists
    mcp.get_client(id)
        .await
        .ok_or_else(|| ApiError::McpClientNotFound(format!("Client '{}' not found", id)))?;

    mcp.remove_client(id).await.map_err(ApiError::Internal)?;

    tracing::info!(client_id = %id, "mcp_client_deleted");
    Ok(Json(json!({ "ok": true })))
}

/// POST /admin/mcp/clients/:id/reconnect — Force reconnect a client.
async fn reconnect_client(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionInfo>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let mcp = require_mcp_manager(&state)?;

    // Verify client exists
    mcp.get_client(id)
        .await
        .ok_or_else(|| ApiError::McpClientNotFound(format!("Client '{}' not found", id)))?;

    mcp.reconnect_client(id).await.map_err(ApiError::Internal)?;

    tracing::info!(client_id = %id, "mcp_client_reconnected");

    // Return updated state
    let response = mcp
        .get_client(id)
        .await
        .map(|c| client_to_json(&c))
        .unwrap_or_else(|| json!({"id": id}));

    Ok(Json(response))
}

/// GET /admin/mcp/clients/:id/tools — List a specific client's tools.
async fn list_client_tools(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionInfo>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let mcp = require_mcp_manager(&state)?;

    let client = mcp
        .get_client(id)
        .await
        .ok_or_else(|| ApiError::McpClientNotFound(format!("Client '{}' not found", id)))?;

    Ok(Json(json!({
        "tools": client.tools,
        "count": client.tools.len(),
    })))
}

/// POST /v1/mcp/tool/execute — Execute a tool (authenticated via API key).
pub async fn execute_tool_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<super::types::ToolExecuteRequest>,
) -> Result<Json<super::types::ToolExecuteResponse>, ApiError> {
    let mcp = require_mcp_manager(&state)?;

    let config = state
        .config
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    let timeout = config.mcp_tool_execution_timeout;

    let clients = mcp.clients_ref();
    let transports = mcp.transports_ref();

    match super::tool_manager::execute_tool(
        &clients,
        &transports,
        &request.tool_name,
        request.arguments.clone(),
        &headers,
        timeout,
    )
    .await
    {
        Ok(result) => Ok(Json(super::types::ToolExecuteResponse {
            call_id: request.call_id,
            tool_name: request.tool_name,
            result,
            is_error: false,
        })),
        Err(e) => Ok(Json(super::types::ToolExecuteResponse {
            call_id: request.call_id,
            tool_name: request.tool_name,
            result: serde_json::json!({"error": e}),
            is_error: true,
        })),
    }
}

// ── Router ──────────────────────────────────────────────────────────

/// Build the MCP admin router (admin-only, nested under admin_api_routes).
///
/// All routes are admin-only — the middleware stack (session + CSRF + admin)
/// is applied by `web_ui_routes()` in `web_ui/mod.rs`.
pub fn mcp_admin_routes() -> Router<AppState> {
    Router::new()
        .route("/clients", get(list_clients).post(create_client))
        .route(
            "/clients/{id}",
            axum::routing::put(update_client).delete(delete_client),
        )
        .route("/clients/{id}/reconnect", post(reconnect_client))
        .route("/clients/{id}/tools", get(list_client_tools))
}
