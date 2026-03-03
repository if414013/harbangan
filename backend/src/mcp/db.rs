#![allow(dead_code)]

use anyhow::{Context, Result};
use sqlx::PgPool;
use uuid::Uuid;

use super::types::{McpAuthType, McpClientConfig, McpConnectionType, McpStdioConfig};

/// Database access layer for MCP client configuration.
pub struct McpDb {
    pool: PgPool,
}

impl McpDb {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// List all MCP client configurations.
    pub async fn list_clients(&self) -> Result<Vec<McpClientConfig>> {
        let rows = sqlx::query_as::<_, McpClientRow>(
            "SELECT id, name, connection_type, connection_string, stdio_config,
                    auth_type, headers_encrypted AS headers_encoded, tools_to_execute,
                    is_ping_available, tool_sync_interval_secs, enabled,
                    created_at, updated_at
             FROM mcp_clients
             ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to list MCP clients")?;

        Ok(rows.into_iter().map(|r| r.into_config()).collect())
    }

    /// Get a single MCP client configuration by ID.
    pub async fn get_client(&self, id: Uuid) -> Result<Option<McpClientConfig>> {
        let row = sqlx::query_as::<_, McpClientRow>(
            "SELECT id, name, connection_type, connection_string, stdio_config,
                    auth_type, headers_encrypted AS headers_encoded, tools_to_execute,
                    is_ping_available, tool_sync_interval_secs, enabled,
                    created_at, updated_at
             FROM mcp_clients WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get MCP client")?;

        Ok(row.map(|r| r.into_config()))
    }

    /// Create a new MCP client configuration.
    pub async fn create_client(&self, config: &McpClientConfig) -> Result<()> {
        let connection_type = config.connection_type.as_str();
        let auth_type = config.auth_type.as_str();
        let stdio_json = config
            .stdio_config
            .as_ref()
            .and_then(|s| serde_json::to_value(s).ok());
        let headers_encoded = encode_headers(&config.headers);
        let tools_json = serde_json::to_value(&config.tools_to_execute)
            .unwrap_or_else(|_| serde_json::json!(["*"]));

        sqlx::query(
            "INSERT INTO mcp_clients (id, name, connection_type, connection_string, stdio_config,
                                      auth_type, headers_encrypted, tools_to_execute,
                                      is_ping_available, tool_sync_interval_secs, enabled)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(config.id)
        .bind(&config.name)
        .bind(connection_type)
        .bind(&config.connection_string)
        .bind(&stdio_json)
        .bind(auth_type)
        .bind(&headers_encoded)
        .bind(&tools_json)
        .bind(config.is_ping_available)
        .bind(config.tool_sync_interval_secs)
        .bind(config.enabled)
        .execute(&self.pool)
        .await
        .context("Failed to create MCP client")?;

        Ok(())
    }

    /// Update an existing MCP client configuration.
    pub async fn update_client(&self, config: &McpClientConfig) -> Result<()> {
        let connection_type = config.connection_type.as_str();
        let auth_type = config.auth_type.as_str();
        let stdio_json = config
            .stdio_config
            .as_ref()
            .and_then(|s| serde_json::to_value(s).ok());
        let headers_encoded = encode_headers(&config.headers);
        let tools_json = serde_json::to_value(&config.tools_to_execute)
            .unwrap_or_else(|_| serde_json::json!(["*"]));

        sqlx::query(
            "UPDATE mcp_clients
             SET name = $2, connection_type = $3, connection_string = $4,
                 stdio_config = $5, auth_type = $6, headers_encrypted = $7,
                 tools_to_execute = $8, is_ping_available = $9,
                 tool_sync_interval_secs = $10, enabled = $11,
                 updated_at = NOW()
             WHERE id = $1",
        )
        .bind(config.id)
        .bind(&config.name)
        .bind(connection_type)
        .bind(&config.connection_string)
        .bind(&stdio_json)
        .bind(auth_type)
        .bind(&headers_encoded)
        .bind(&tools_json)
        .bind(config.is_ping_available)
        .bind(config.tool_sync_interval_secs)
        .bind(config.enabled)
        .execute(&self.pool)
        .await
        .context("Failed to update MCP client")?;

        Ok(())
    }

    /// Delete an MCP client by ID.
    pub async fn delete_client(&self, id: Uuid) -> Result<u64> {
        let result = sqlx::query("DELETE FROM mcp_clients WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to delete MCP client")?;

        Ok(result.rows_affected())
    }
}

// ── Internal row mapping ─────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct McpClientRow {
    id: Uuid,
    name: String,
    connection_type: String,
    connection_string: Option<String>,
    stdio_config: Option<serde_json::Value>,
    auth_type: String,
    headers_encoded: Option<String>,
    tools_to_execute: serde_json::Value,
    is_ping_available: bool,
    tool_sync_interval_secs: i32,
    enabled: bool,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

impl McpClientRow {
    fn into_config(self) -> McpClientConfig {
        let connection_type =
            McpConnectionType::parse_str(&self.connection_type).unwrap_or(McpConnectionType::Http);
        let auth_type = McpAuthType::parse_str(&self.auth_type).unwrap_or(McpAuthType::None);
        let stdio_config: Option<McpStdioConfig> = self
            .stdio_config
            .and_then(|v| serde_json::from_value(v).ok());
        let headers = decode_headers(self.headers_encoded.as_deref());
        let tools_to_execute: Vec<String> =
            serde_json::from_value(self.tools_to_execute).unwrap_or_else(|_| vec!["*".to_string()]);

        McpClientConfig {
            id: self.id,
            name: self.name,
            connection_type,
            connection_string: self.connection_string,
            stdio_config,
            auth_type,
            headers,
            tools_to_execute,
            is_ping_available: self.is_ping_available,
            tool_sync_interval_secs: self.tool_sync_interval_secs,
            enabled: self.enabled,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

// ── Headers encoding (base64 JSON) ──────────────────────────────────

fn encode_headers(headers: &std::collections::HashMap<String, String>) -> Option<String> {
    if headers.is_empty() {
        return None;
    }
    let json = serde_json::to_string(headers).ok()?;
    Some(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        json.as_bytes(),
    ))
}

fn decode_headers(encoded: Option<&str>) -> std::collections::HashMap<String, String> {
    encoded
        .and_then(|s| {
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s).ok()
        })
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}
