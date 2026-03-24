use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::{Config, DebugMode};
use crate::web_ui::crypto;

/// Query row for expiring Kiro tokens with OAuth credentials.
type KiroTokenOAuthRow = (Uuid, String, Option<String>, Option<String>, Option<String>);

/// Query row for a model registry entry (13-column tuple).
type RegistryModelRow = (
    Uuid,
    String,
    String,
    String,
    String,
    i32,
    i32,
    serde_json::Value,
    bool,
    String,
    Option<serde_json::Value>,
    DateTime<Utc>,
    DateTime<Utc>,
);

/// Query row for Copilot tokens (7-column tuple).
type CopilotTokenQueryRow = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<i64>,
);

/// Query row for expiring Copilot tokens (8-column tuple, includes user_id).
type CopilotTokenExpiringRow = (
    Uuid,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<i64>,
);

/// A row from the `user_copilot_tokens` table.
#[derive(Debug, Clone)]
pub struct CopilotTokenRow {
    pub user_id: Uuid,
    pub github_token: String,
    pub github_username: Option<String>,
    pub copilot_token: Option<String>,
    pub copilot_plan: Option<String>,
    pub base_url: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    #[allow(dead_code)]
    pub refresh_in: Option<i64>,
}

/// Tuple representing a user row: (id, email, name, picture_url, role, created_at).
pub type UserRow = (Uuid, String, String, Option<String>, String, DateTime<Utc>);

/// A row from the `model_registry` table.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RegistryModel {
    pub id: Uuid,
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub prefixed_id: String,
    pub context_length: i32,
    pub max_output_tokens: i32,
    pub capabilities: serde_json::Value,
    pub enabled: bool,
    pub source: String,
    pub upstream_meta: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A row from the `user_provider_tokens` table (multi-account aware).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct UserProviderTokenRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub provider_id: String,
    pub account_label: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
    pub email: String,
    pub base_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A row from the `admin_provider_pool` table.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminPoolRow {
    pub id: Uuid,
    pub provider_id: String,
    pub account_label: String,
    pub api_key: String,
    pub key_prefix: String,
    pub base_url: Option<String>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A record of a configuration change.
#[derive(Debug, Clone)]
pub struct ConfigChange {
    pub key: String,
    pub old_value: Option<String>,
    pub new_value: String,
    pub changed_at: String,
    pub source: String,
}

/// Summary of usage records grouped by a key (day, model, or provider).
#[derive(Debug, serde::Serialize)]
pub struct UsageSummary {
    pub group_key: String,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_cost: f64,
    pub request_count: i64,
}

/// Summary of usage records grouped by user.
#[derive(Debug, serde::Serialize)]
pub struct UserUsageSummary {
    pub user_id: uuid::Uuid,
    pub email: String,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_cost: f64,
    pub request_count: i64,
}

/// PostgreSQL-backed configuration persistence.
pub struct ConfigDb {
    pool: PgPool,
}

impl ConfigDb {
    /// Connect to the PostgreSQL database and run migrations.
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPool::connect(database_url)
            .await
            .context("Failed to connect to PostgreSQL")?;
        let db = Self { pool };
        db.run_migrations().await?;
        Ok(db)
    }

    /// Create tables if they don't already exist.
    async fn run_migrations(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version    INTEGER NOT NULL,
                applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create schema_version table")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS config (
                key         TEXT PRIMARY KEY NOT NULL,
                value       TEXT NOT NULL,
                value_type  TEXT NOT NULL DEFAULT 'string',
                updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                description TEXT
            )",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create config table")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS config_history (
                id         SERIAL PRIMARY KEY,
                key        TEXT NOT NULL,
                old_value  TEXT,
                new_value  TEXT NOT NULL,
                changed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                source     TEXT NOT NULL DEFAULT 'web_ui'
            )",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create config_history table")?;

        // Record schema version 1 if not present
        let count: Option<i64> = sqlx::query_scalar("SELECT COUNT(*) FROM schema_version")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(Some(0));

        if count.unwrap_or(0) == 0 {
            sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
                .bind(1_i32)
                .execute(&self.pool)
                .await
                .context("Failed to insert schema version")?;
        }

        // Version 3: Multi-user auth tables
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 3 {
            self.migrate_to_v3().await?;
        }

        // Re-read max version after v3 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 4 {
            self.migrate_to_v4().await?;
        }

        // Re-read max version after v4 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 5 {
            self.migrate_to_v5().await?;
        }

        // Re-read max version after v5 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 6 {
            self.migrate_to_v6().await?;
        }

        // Re-read max version after v6 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 7 {
            self.migrate_to_v7().await?;
        }

        // Re-read max version after v7 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 8 {
            self.migrate_to_v8().await?;
        }

        // Re-read max version after v8 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 9 {
            self.migrate_to_v9().await?;
        }

        // Re-read max version after v9 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 10 {
            self.migrate_to_v10().await?;
        }

        // Re-read max version after v10 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 11 {
            self.migrate_to_v11().await?;
        }

        // Re-read max version after v11 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 12 {
            self.migrate_to_v12().await?;
        }

        // Re-read max version after v12 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 13 {
            self.migrate_to_v13().await?;
        }

        // Re-read max version after v13 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 14 {
            self.migrate_to_v14().await?;
        }

        // Re-read max version after v14 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 15 {
            self.migrate_to_v15().await?;
        }

        // Re-read max version after v15 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 16 {
            self.migrate_to_v16().await?;
        }

        // Re-read max version after v16 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 17 {
            self.migrate_to_v17().await?;
        }

        // Re-read max version after v17 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 18 {
            self.migrate_to_v18().await?;
        }

        // Re-read max version after v18 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 19 {
            self.migrate_to_v19().await?;
        }

        // Re-read max version after v19 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 20 {
            self.migrate_to_v20().await?;
        }

        if max_version.unwrap_or(1) < 21 {
            self.migrate_to_v21().await?;
        }

        // Re-read max version after v21 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 22 {
            self.migrate_to_v22().await?;
        }

        // Re-read max version after v22 migration
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(1));

        if max_version.unwrap_or(1) < 23 {
            self.migrate_to_v23().await?;
        }

        Ok(())
    }

    /// Version 3 migration: multi-user auth tables.
    async fn migrate_to_v3(&self) -> Result<()> {
        tracing::info!("Running database migration to version 3 (multi-user auth)...");

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS users (
                id          UUID PRIMARY KEY,
                email       TEXT UNIQUE NOT NULL,
                name        TEXT NOT NULL,
                picture_url TEXT,
                role        TEXT NOT NULL DEFAULT 'user' CHECK (role IN ('admin', 'user')),
                created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                last_login  TIMESTAMPTZ
            )",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create users table")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS sessions (
                id         UUID PRIMARY KEY,
                user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                expires_at TIMESTAMPTZ NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create sessions table")?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id)")
            .execute(&self.pool)
            .await
            .context("Failed to create sessions user index")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS user_kiro_tokens (
                user_id        UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
                refresh_token  TEXT NOT NULL,
                access_token   TEXT,
                token_expiry   TIMESTAMPTZ,
                updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create user_kiro_tokens table")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS api_keys (
                id         UUID PRIMARY KEY,
                user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                key_hash   TEXT UNIQUE NOT NULL,
                key_prefix TEXT NOT NULL,
                label      TEXT NOT NULL DEFAULT '',
                last_used  TIMESTAMPTZ,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create api_keys table")?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash)")
            .execute(&self.pool)
            .await
            .context("Failed to create api_keys hash index")?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_api_keys_user ON api_keys(user_id)")
            .execute(&self.pool)
            .await
            .context("Failed to create api_keys user index")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS allowed_domains (
                domain     TEXT PRIMARY KEY,
                added_by   UUID REFERENCES users(id),
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create allowed_domains table")?;

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(3_i32)
            .execute(&self.pool)
            .await
            .context("Failed to record schema version 3")?;

        tracing::info!("Database migration to version 3 complete");
        Ok(())
    }

    /// Version 4 migration: guardrails tables.
    async fn migrate_to_v4(&self) -> Result<()> {
        tracing::info!("Running database migration to version 4 (guardrails)...");

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v4 migration transaction")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS guardrail_profiles (
                id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                name              TEXT NOT NULL,
                provider_name     TEXT NOT NULL DEFAULT 'bedrock',
                enabled           BOOLEAN NOT NULL DEFAULT true,
                guardrail_id      TEXT NOT NULL,
                guardrail_version TEXT NOT NULL DEFAULT '1',
                region            TEXT NOT NULL DEFAULT 'us-east-1',
                access_key        TEXT NOT NULL,
                secret_key        TEXT NOT NULL,
                created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create guardrail_profiles table")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS guardrail_rules (
                id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                name            TEXT NOT NULL,
                description     TEXT NOT NULL DEFAULT '',
                enabled         BOOLEAN NOT NULL DEFAULT true,
                cel_expression  TEXT NOT NULL DEFAULT '',
                apply_to        TEXT NOT NULL DEFAULT 'both' CHECK (apply_to IN ('input', 'output', 'both')),
                sampling_rate   SMALLINT NOT NULL DEFAULT 100 CHECK (sampling_rate BETWEEN 0 AND 100),
                timeout_ms      INTEGER NOT NULL DEFAULT 5000,
                created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create guardrail_rules table")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS guardrail_rule_profiles (
                rule_id    UUID NOT NULL REFERENCES guardrail_rules(id) ON DELETE CASCADE,
                profile_id UUID NOT NULL REFERENCES guardrail_profiles(id) ON DELETE CASCADE,
                PRIMARY KEY (rule_id, profile_id)
            )",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create guardrail_rule_profiles table")?;

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(4_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 4")?;

        tx.commit().await.context("Failed to commit v4 migration")?;

        tracing::info!("Database migration to version 4 complete");
        Ok(())
    }

    /// Version 5 migration: MCP clients table.
    async fn migrate_to_v5(&self) -> Result<()> {
        tracing::info!("Running database migration to version 5 (MCP clients)...");

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v5 migration transaction")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS mcp_clients (
                id                      UUID PRIMARY KEY,
                name                    TEXT UNIQUE NOT NULL,
                connection_type         TEXT NOT NULL CHECK (connection_type IN ('http', 'sse', 'stdio')),
                connection_string       TEXT,
                stdio_config            JSONB,
                auth_type               TEXT NOT NULL DEFAULT 'none' CHECK (auth_type IN ('none', 'headers')),
                headers_encrypted       TEXT,
                tools_to_execute        JSONB NOT NULL DEFAULT '[\"*\"]'::jsonb,
                is_ping_available       BOOLEAN NOT NULL DEFAULT TRUE,
                tool_sync_interval_secs INTEGER NOT NULL DEFAULT 0,
                enabled                 BOOLEAN NOT NULL DEFAULT TRUE,
                created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at              TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create mcp_clients table")?;

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(5_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 5")?;

        tx.commit().await.context("Failed to commit v5 migration")?;

        tracing::info!("Database migration to version 5 complete");
        Ok(())
    }

    /// Version 6 migration: per-user OAuth client credentials on user_kiro_tokens.
    async fn migrate_to_v6(&self) -> Result<()> {
        tracing::info!("Running database migration to version 6 (per-user OAuth creds)...");

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v6 migration transaction")?;

        sqlx::query("ALTER TABLE user_kiro_tokens ADD COLUMN IF NOT EXISTS oauth_client_id TEXT")
            .execute(&mut *tx)
            .await
            .context("Failed to add oauth_client_id column")?;

        sqlx::query(
            "ALTER TABLE user_kiro_tokens ADD COLUMN IF NOT EXISTS oauth_client_secret TEXT",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to add oauth_client_secret column")?;

        sqlx::query("ALTER TABLE user_kiro_tokens ADD COLUMN IF NOT EXISTS oauth_sso_region TEXT")
            .execute(&mut *tx)
            .await
            .context("Failed to add oauth_sso_region column")?;

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(6_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 6")?;

        tx.commit().await.context("Failed to commit v6 migration")?;

        tracing::info!("Database migration to version 6 complete");
        Ok(())
    }

    /// Get a single config value by key.
    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        let result: Option<String> = sqlx::query_scalar("SELECT value FROM config WHERE key = $1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to query config value")?;

        Ok(result)
    }

    /// Upsert a config value and record the change in history.
    /// All operations (read old value, upsert, history insert, prune) run in a single transaction.
    pub async fn set(&self, key: &str, value: &str, source: &str) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin transaction for config set")?;

        // Fetch old value for history
        let old_value: Option<String> =
            sqlx::query_scalar("SELECT value FROM config WHERE key = $1")
                .bind(key)
                .fetch_optional(&mut *tx)
                .await
                .context("Failed to fetch old config value")?;

        sqlx::query(
            "INSERT INTO config (key, value, updated_at)
             VALUES ($1, $2, NOW())
             ON CONFLICT(key) DO UPDATE SET value = EXCLUDED.value, updated_at = EXCLUDED.updated_at",
        )
        .bind(key)
        .bind(value)
        .execute(&mut *tx)
        .await
        .with_context(|| format!("Failed to upsert config key '{}'", key))?;

        sqlx::query(
            "INSERT INTO config_history (key, old_value, new_value, source)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(key)
        .bind(&old_value)
        .bind(value)
        .bind(source)
        .execute(&mut *tx)
        .await
        .with_context(|| format!("Failed to record config history for '{}'", key))?;

        // Prune old history entries, keeping the most recent 1000
        sqlx::query(
            "DELETE FROM config_history WHERE id NOT IN (SELECT id FROM config_history ORDER BY id DESC LIMIT 1000)",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to prune config history")?;

        tx.commit()
            .await
            .context("Failed to commit config set transaction")?;

        Ok(())
    }

    /// Upsert an encrypted config value. Encrypts the plaintext and stores with `value_type='encrypted'`.
    #[allow(dead_code)]
    pub async fn set_encrypted(
        &self,
        key: &str,
        plaintext: &str,
        encryption_key: &aes_gcm::Key<aes_gcm::Aes256Gcm>,
        source: &str,
    ) -> Result<()> {
        let encrypted = crypto::encrypt_value(plaintext, encryption_key)
            .with_context(|| format!("Failed to encrypt config value for '{}'", key))?;

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin transaction for encrypted config set")?;

        let old_value: Option<String> =
            sqlx::query_scalar("SELECT value FROM config WHERE key = $1")
                .bind(key)
                .fetch_optional(&mut *tx)
                .await
                .context("Failed to fetch old config value")?;

        sqlx::query(
            "INSERT INTO config (key, value, value_type, updated_at)
             VALUES ($1, $2, 'encrypted', NOW())
             ON CONFLICT(key) DO UPDATE SET value = EXCLUDED.value, value_type = 'encrypted', updated_at = EXCLUDED.updated_at",
        )
        .bind(key)
        .bind(&encrypted)
        .execute(&mut *tx)
        .await
        .with_context(|| format!("Failed to upsert encrypted config key '{}'", key))?;

        sqlx::query(
            "INSERT INTO config_history (key, old_value, new_value, source)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(key)
        .bind(&old_value)
        .bind("[encrypted]")
        .bind(source)
        .execute(&mut *tx)
        .await
        .with_context(|| format!("Failed to record config history for '{}'", key))?;

        sqlx::query(
            "DELETE FROM config_history WHERE id NOT IN (SELECT id FROM config_history ORDER BY id DESC LIMIT 1000)",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to prune config history")?;

        tx.commit()
            .await
            .context("Failed to commit encrypted config set transaction")?;

        Ok(())
    }

    /// Read and decrypt a config value that was stored with `value_type='encrypted'`.
    #[allow(dead_code)]
    pub async fn get_decrypted(
        &self,
        key: &str,
        encryption_key: &aes_gcm::Key<aes_gcm::Aes256Gcm>,
    ) -> Result<Option<String>> {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT value, value_type FROM config WHERE key = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .context("Failed to query config value")?;

        match row {
            Some((value, vtype)) if vtype == "encrypted" => {
                let plaintext = crypto::decrypt_value(&value, encryption_key)
                    .with_context(|| format!("Failed to decrypt config key '{}'", key))?;
                Ok(Some(plaintext))
            }
            Some((value, _)) => Ok(Some(value)),
            None => Ok(None),
        }
    }

    /// Get all config key-value pairs.
    #[allow(dead_code)]
    pub async fn get_all(&self) -> Result<HashMap<String, String>> {
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM config")
            .fetch_all(&self.pool)
            .await
            .context("Failed to query all config")?;

        let mut map = HashMap::new();
        for (k, v) in rows {
            map.insert(k, v);
        }
        Ok(map)
    }

    /// Get recent config change history.
    pub async fn get_history(&self, limit: usize) -> Result<Vec<ConfigChange>> {
        let rows: Vec<(String, Option<String>, String, String, String)> = sqlx::query_as(
            "SELECT key, old_value, new_value, changed_at::TEXT, source
             FROM config_history
             ORDER BY id DESC
             LIMIT $1",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .context("Failed to query config history")?;

        let changes = rows
            .into_iter()
            .map(
                |(key, old_value, new_value, changed_at, source)| ConfigChange {
                    key,
                    old_value,
                    new_value,
                    changed_at,
                    source,
                },
            )
            .collect();

        Ok(changes)
    }

    /// Overlay persisted config values onto an existing Config struct.
    ///
    /// Numeric fields are validated against allowed ranges. Out-of-range or
    /// unparseable values are logged and silently skipped (the default is kept).
    pub async fn load_into_config(&self, config: &mut Config) -> Result<()> {
        /// Parse a numeric config value, validate it against an inclusive range,
        /// and log warnings on parse failure or out-of-range values.
        macro_rules! parse_ranged {
            ($key:expr, $value:expr, $field:expr, $ty:ty, $lo:expr, $hi:expr) => {
                match $value.parse::<$ty>() {
                    Ok(v) if ($lo..=$hi).contains(&v) => $field = v,
                    Ok(v) => {
                        tracing::warn!(
                            "Config '{}' value '{}' out of range ({}..={}), keeping default",
                            $key,
                            v,
                            $lo,
                            $hi
                        );
                    }
                    Err(_) => {
                        tracing::warn!(
                            "Failed to parse config '{}' value '{}', keeping default",
                            $key,
                            $value
                        );
                    }
                }
            };
        }

        let rows: Vec<(String, String, String)> =
            sqlx::query_as("SELECT key, value, value_type FROM config")
                .fetch_all(&self.pool)
                .await
                .context("Failed to query all config for load_into_config")?;

        let encryption_key = crypto::load_encryption_key().ok();

        for (key, raw_value, value_type) in &rows {
            let value = if value_type == "encrypted" {
                match &encryption_key {
                    Some(ek) => match crypto::decrypt_value(raw_value, ek) {
                        Ok(plaintext) => plaintext,
                        Err(e) => {
                            tracing::warn!(
                                key = %key,
                                "Failed to decrypt config value, skipping: {}",
                                e
                            );
                            continue;
                        }
                    },
                    None => {
                        tracing::warn!(
                            key = %key,
                            "Encrypted config value found but CONFIG_ENCRYPTION_KEY not set, skipping"
                        );
                        continue;
                    }
                }
            } else {
                raw_value.clone()
            };

            match key.as_str() {
                "kiro_region" => config.kiro_region = value.clone(),
                "log_level" => config.log_level = value.clone(),
                "debug_mode" => {
                    config.debug_mode = match value.to_lowercase().as_str() {
                        "errors" => DebugMode::Errors,
                        "all" => DebugMode::All,
                        _ => DebugMode::Off,
                    };
                }
                "fake_reasoning_enabled" => {
                    if let Ok(v) = value.parse() {
                        config.fake_reasoning_enabled = v;
                    } else {
                        tracing::warn!(
                            "Failed to parse config '{}' value '{}', keeping default",
                            key,
                            value
                        );
                    }
                }
                "fake_reasoning_max_tokens" => {
                    parse_ranged!(
                        key,
                        value,
                        config.fake_reasoning_max_tokens,
                        u32,
                        1,
                        1_000_000
                    );
                }
                "truncation_recovery" => {
                    if let Ok(v) = value.parse() {
                        config.truncation_recovery = v;
                    } else {
                        tracing::warn!(
                            "Failed to parse config '{}' value '{}', keeping default",
                            key,
                            value
                        );
                    }
                }
                "guardrails_enabled" => {
                    if let Ok(v) = value.parse() {
                        config.guardrails_enabled = v;
                    } else {
                        tracing::warn!(
                            "Failed to parse config '{}' value '{}', keeping default",
                            key,
                            value
                        );
                    }
                }
                "tool_description_max_length" => {
                    parse_ranged!(
                        key,
                        value,
                        config.tool_description_max_length,
                        usize,
                        1,
                        1_000_000
                    );
                }
                "first_token_timeout" => {
                    parse_ranged!(key, value, config.first_token_timeout, u64, 1, 86400);
                }
                "streaming_timeout" => {
                    parse_ranged!(key, value, config.streaming_timeout, u64, 1, 86400);
                }
                "token_refresh_threshold" => {
                    parse_ranged!(key, value, config.token_refresh_threshold, u64, 1, 86400);
                }
                "http_max_connections" => {
                    parse_ranged!(key, value, config.http_max_connections, usize, 1, 1000);
                }
                "http_connect_timeout" => {
                    parse_ranged!(key, value, config.http_connect_timeout, u64, 1, 86400);
                }
                "http_request_timeout" => {
                    parse_ranged!(key, value, config.http_request_timeout, u64, 1, 86400);
                }
                "http_max_retries" => {
                    parse_ranged!(key, value, config.http_max_retries, u32, 0, 10);
                }
                "anthropic_oauth_client_id" => config.anthropic_oauth_client_id = value.clone(),
                "openai_oauth_client_id" => config.openai_oauth_client_id = value.clone(),
                "google_client_id" => config.google_client_id = value.clone(),
                "google_client_secret" => config.google_client_secret = value.clone(),
                "google_callback_url" => config.google_callback_url = value.clone(),
                "auth_google_enabled" => {
                    config.auth_google_enabled = value == "true";
                }
                "auth_password_enabled" => {
                    config.auth_password_enabled = value == "true";
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Check if initial setup has been completed.
    ///
    /// Setup is complete when at least one admin user exists.
    pub async fn is_setup_complete(&self) -> bool {
        let result: Option<bool> =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE role = 'admin')")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(Some(false));

        result.unwrap_or(false)
    }

    /// Get the stored Kiro refresh token.
    pub async fn get_refresh_token(&self) -> Result<Option<String>> {
        self.get("kiro_refresh_token").await
    }

    /// Get OAuth client credentials from config.
    #[allow(dead_code)]
    pub async fn get_oauth_client(&self) -> Result<Option<(String, String, String)>> {
        let client_id = self.get("oauth_client_id").await?;
        let client_secret = self.get("oauth_client_secret").await?;
        let expires_at = self
            .get("oauth_client_secret_expires_at")
            .await?
            .unwrap_or_default();
        match (client_id, client_secret) {
            (Some(id), Some(secret)) => Ok(Some((id, secret, expires_at))),
            _ => Ok(None),
        }
    }

    // ── User CRUD ──────────────────────────────────────────────────

    /// Upsert a user from Google profile. First user gets admin role atomically.
    /// Uses SERIALIZABLE isolation to prevent race conditions in first-user-admin assignment.
    /// Returns (user_id, role).
    #[allow(dead_code)]
    pub async fn upsert_user(
        &self,
        email: &str,
        name: &str,
        picture_url: Option<&str>,
    ) -> Result<(Uuid, String)> {
        // Retry loop for SERIALIZABLE transaction serialization failures (error code 40001)
        for attempt in 0..3 {
            let mut tx = self
                .pool
                .begin()
                .await
                .context("Failed to begin transaction")?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
                .execute(&mut *tx)
                .await
                .context("Failed to set isolation level")?;

            let id = Uuid::new_v4();
            let result: std::result::Result<(Uuid, String), sqlx::Error> = sqlx::query_as(
                "INSERT INTO users (id, email, name, picture_url, role)
                 VALUES ($1, $2, $3, $4,
                   CASE WHEN (SELECT COUNT(*) FROM users) = 0 THEN 'admin' ELSE 'user' END)
                 ON CONFLICT (email) DO UPDATE SET last_login = NOW(), name = EXCLUDED.name, picture_url = EXCLUDED.picture_url
                 RETURNING id, role",
            )
            .bind(id)
            .bind(email)
            .bind(name)
            .bind(picture_url)
            .fetch_one(&mut *tx)
            .await;

            match result {
                Ok(row) => {
                    tx.commit()
                        .await
                        .context("Failed to commit upsert_user transaction")?;
                    return Ok(row);
                }
                Err(e) => {
                    // Check for serialization failure (PostgreSQL error code 40001)
                    let is_serialization_failure = e
                        .as_database_error()
                        .and_then(|db_err| db_err.code())
                        .map(|code| code == "40001")
                        .unwrap_or(false);

                    if is_serialization_failure && attempt < 2 {
                        tracing::warn!(attempt, "Serialization failure in upsert_user, retrying");
                        continue;
                    }
                    return Err(anyhow::anyhow!(e).context("Failed to upsert user"));
                }
            }
        }
        unreachable!()
    }

    /// Get a user by ID.
    #[allow(dead_code)]
    pub async fn get_user(&self, user_id: Uuid) -> Result<Option<UserRow>> {
        let row: Option<UserRow> = sqlx::query_as(
            "SELECT id, email, name, picture_url, role, created_at FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get user")?;

        Ok(row)
    }

    /// Get a user by email.
    #[allow(dead_code)]
    pub async fn get_user_by_email(
        &self,
        email: &str,
    ) -> Result<Option<(Uuid, String, String, Option<String>, String)>> {
        let row: Option<(Uuid, String, String, Option<String>, String)> =
            sqlx::query_as("SELECT id, email, name, picture_url, role FROM users WHERE email = $1")
                .bind(email)
                .fetch_optional(&self.pool)
                .await
                .context("Failed to get user by email")?;

        Ok(row)
    }

    // ── Password Auth + 2FA Methods ────────────────────────────────

    /// Create a password-authenticated user.
    /// Uses SERIALIZABLE isolation like upsert_user for first-user-admin logic.
    #[allow(dead_code)]
    pub async fn create_password_user(
        &self,
        email: &str,
        name: &str,
        password_hash: &str,
        role: &str,
        must_change_password: bool,
    ) -> Result<Uuid> {
        for attempt in 0..3 {
            let mut tx = self
                .pool
                .begin()
                .await
                .context("Failed to begin transaction")?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
                .execute(&mut *tx)
                .await
                .context("Failed to set isolation level")?;

            let id = Uuid::new_v4();
            let effective_role = if role == "admin" {
                "admin".to_string()
            } else {
                // First user becomes admin
                let count: Option<i64> = sqlx::query_scalar("SELECT COUNT(*) FROM users")
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap_or(Some(0));
                if count.unwrap_or(0) == 0 {
                    "admin".to_string()
                } else {
                    role.to_string()
                }
            };

            let result = sqlx::query_scalar::<_, Uuid>(
                "INSERT INTO users (id, email, name, role, auth_method, password_hash, must_change_password)
                 VALUES ($1, $2, $3, $4, 'password', $5, $6)
                 RETURNING id",
            )
            .bind(id)
            .bind(email)
            .bind(name)
            .bind(&effective_role)
            .bind(password_hash)
            .bind(must_change_password)
            .fetch_one(&mut *tx)
            .await;

            match result {
                Ok(user_id) => {
                    tx.commit()
                        .await
                        .context("Failed to commit create_password_user transaction")?;
                    return Ok(user_id);
                }
                Err(e) => {
                    let is_serialization_failure = e
                        .as_database_error()
                        .and_then(|db_err| db_err.code())
                        .map(|code| code == "40001")
                        .unwrap_or(false);

                    if is_serialization_failure && attempt < 2 {
                        tracing::warn!(
                            attempt,
                            "Serialization failure in create_password_user, retrying"
                        );
                        continue;
                    }
                    return Err(anyhow::anyhow!(e).context("Failed to create password user"));
                }
            }
        }
        unreachable!()
    }

    /// Get a user by email with full auth fields.
    /// Returns: (id, email, name, picture_url, role, password_hash, totp_enabled, auth_method, must_change_password)
    #[allow(dead_code)]
    pub async fn get_user_by_email_with_auth(
        &self,
        email: &str,
    ) -> Result<
        Option<(
            Uuid,
            String,
            String,
            Option<String>,
            String,
            Option<String>,
            bool,
            String,
            bool,
        )>,
    > {
        let row = sqlx::query_as(
            "SELECT id, email, name, picture_url, role, password_hash, totp_enabled, auth_method, must_change_password
             FROM users WHERE email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get user by email with auth")?;

        Ok(row)
    }

    /// Get a user by ID with full auth fields (for session middleware).
    /// Returns same tuple as get_user_by_email_with_auth.
    #[allow(dead_code)]
    pub async fn get_user_by_email_with_auth_by_id(
        &self,
        user_id: Uuid,
    ) -> Result<
        Option<(
            Uuid,
            String,
            String,
            Option<String>,
            String,
            Option<String>,
            bool,
            String,
            bool,
        )>,
    > {
        let row = sqlx::query_as(
            "SELECT id, email, name, picture_url, role, password_hash, totp_enabled, auth_method, must_change_password
             FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get user by id with auth")?;

        Ok(row)
    }

    /// Update a user's password and clear the must_change_password flag.
    #[allow(dead_code)]
    pub async fn update_password(&self, user_id: Uuid, password_hash: &str) -> Result<()> {
        sqlx::query(
            "UPDATE users SET password_hash = $1, must_change_password = false WHERE id = $2",
        )
        .bind(password_hash)
        .bind(user_id)
        .execute(&self.pool)
        .await
        .context("Failed to update password")?;

        Ok(())
    }

    /// Enable TOTP for a user with the given secret.
    #[allow(dead_code)]
    pub async fn enable_totp(&self, user_id: Uuid, totp_secret: &str) -> Result<()> {
        sqlx::query("UPDATE users SET totp_secret = $1, totp_enabled = true WHERE id = $2")
            .bind(totp_secret)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .context("Failed to enable TOTP")?;

        Ok(())
    }

    /// Disable TOTP for a user (clear secret and flag).
    #[allow(dead_code)]
    pub async fn disable_totp(&self, user_id: Uuid) -> Result<()> {
        sqlx::query("UPDATE users SET totp_secret = NULL, totp_enabled = false WHERE id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await
            .context("Failed to disable TOTP")?;

        Ok(())
    }

    /// Get the TOTP secret for a user.
    #[allow(dead_code)]
    pub async fn get_totp_secret(&self, user_id: Uuid) -> Result<Option<String>> {
        let row: Option<(Option<String>,)> =
            sqlx::query_as("SELECT totp_secret FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_optional(&self.pool)
                .await
                .context("Failed to get TOTP secret")?;

        Ok(row.and_then(|r| r.0))
    }

    /// Store TOTP recovery code hashes (replaces existing ones).
    #[allow(dead_code)]
    pub async fn store_recovery_codes(&self, user_id: Uuid, code_hashes: &[String]) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin transaction")?;

        // Delete old codes
        sqlx::query("DELETE FROM totp_recovery_codes WHERE user_id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .context("Failed to delete old recovery codes")?;

        // Insert new ones
        for hash in code_hashes {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO totp_recovery_codes (id, user_id, code_hash) VALUES ($1, $2, $3)",
            )
            .bind(id)
            .bind(user_id)
            .bind(hash)
            .execute(&mut *tx)
            .await
            .context("Failed to insert recovery code")?;
        }

        tx.commit()
            .await
            .context("Failed to commit recovery codes")?;

        Ok(())
    }

    /// Use a recovery code (marks it as used). Returns true if the code was valid and unused.
    #[allow(dead_code)]
    pub async fn use_recovery_code(&self, user_id: Uuid, code_hash: &str) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE totp_recovery_codes SET used = true
             WHERE user_id = $1 AND code_hash = $2 AND used = false",
        )
        .bind(user_id)
        .bind(code_hash)
        .execute(&self.pool)
        .await
        .context("Failed to use recovery code")?;

        Ok(result.rows_affected() > 0)
    }

    /// Set the google_linked flag for a user.
    #[allow(dead_code)]
    pub async fn set_google_linked(&self, user_id: Uuid, linked: bool) -> Result<()> {
        sqlx::query("UPDATE users SET google_linked = $1 WHERE id = $2")
            .bind(linked)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .context("Failed to set google_linked")?;

        Ok(())
    }

    /// Get the google_linked flag for a user.
    #[allow(dead_code)]
    pub async fn get_google_linked(&self, user_id: Uuid) -> Result<bool> {
        let row: Option<(bool,)> = sqlx::query_as("SELECT google_linked FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to get google_linked")?;

        Ok(row.map(|r| r.0).unwrap_or(false))
    }

    /// Update a user's password, set auth_method to 'password', and clear must_change_password.
    #[allow(dead_code)]
    pub async fn update_password_with_auth_method(
        &self,
        user_id: Uuid,
        password_hash: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE users SET password_hash = $1, auth_method = 'password', must_change_password = false WHERE id = $2",
        )
        .bind(password_hash)
        .bind(user_id)
        .execute(&self.pool)
        .await
        .context("Failed to update password with auth_method")?;

        Ok(())
    }

    /// Create a pending 2FA login with 5-minute expiry. Returns the token UUID.
    #[allow(dead_code)]
    pub async fn create_pending_2fa(&self, user_id: Uuid) -> Result<Uuid> {
        let token = Uuid::new_v4();
        let expires_at = Utc::now() + chrono::Duration::minutes(5);

        sqlx::query(
            "INSERT INTO pending_2fa_logins (token, user_id, expires_at) VALUES ($1, $2, $3)",
        )
        .bind(token)
        .bind(user_id)
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .context("Failed to create pending 2FA login")?;

        Ok(token)
    }

    /// Get a pending 2FA login (only if not expired).
    #[allow(dead_code)]
    pub async fn get_pending_2fa(
        &self,
        token: Uuid,
    ) -> Result<Option<(Uuid, Uuid, DateTime<Utc>)>> {
        let row: Option<(Uuid, Uuid, DateTime<Utc>)> = sqlx::query_as(
            "SELECT token, user_id, expires_at FROM pending_2fa_logins
             WHERE token = $1 AND expires_at > NOW()",
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get pending 2FA login")?;

        Ok(row)
    }

    /// Delete a pending 2FA login.
    #[allow(dead_code)]
    pub async fn delete_pending_2fa(&self, token: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM pending_2fa_logins WHERE token = $1")
            .bind(token)
            .execute(&self.pool)
            .await
            .context("Failed to delete pending 2FA login")?;

        Ok(())
    }

    /// Clean up all expired pending 2FA logins. Returns the number of rows deleted.
    #[allow(dead_code)]
    pub async fn cleanup_expired_2fa(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM pending_2fa_logins WHERE expires_at <= NOW()")
            .execute(&self.pool)
            .await
            .context("Failed to cleanup expired 2FA logins")?;

        Ok(result.rows_affected())
    }

    /// Reset a user's 2FA (disable TOTP + delete recovery codes).
    #[allow(dead_code)]
    pub async fn reset_user_2fa(&self, user_id: Uuid) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin transaction")?;

        sqlx::query("UPDATE users SET totp_secret = NULL, totp_enabled = false WHERE id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .context("Failed to disable TOTP")?;

        sqlx::query("DELETE FROM totp_recovery_codes WHERE user_id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .context("Failed to delete recovery codes")?;

        tx.commit().await.context("Failed to commit 2FA reset")?;

        Ok(())
    }

    /// List all users.
    #[allow(dead_code)]
    pub async fn list_users(
        &self,
    ) -> Result<
        Vec<(
            Uuid,
            String,
            String,
            Option<String>,
            String,
            DateTime<Utc>,
            Option<DateTime<Utc>>,
        )>,
    > {
        let rows = sqlx::query_as(
            "SELECT id, email, name, picture_url, role, created_at, last_login
             FROM users ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to list users")?;

        Ok(rows)
    }

    /// Update a user's role. Returns the number of rows affected.
    #[allow(dead_code)]
    pub async fn update_user_role(&self, user_id: Uuid, role: &str) -> Result<u64> {
        let result = sqlx::query("UPDATE users SET role = $1 WHERE id = $2")
            .bind(role)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .context("Failed to update user role")?;

        Ok(result.rows_affected())
    }

    /// Delete a user by ID. Returns the number of rows affected.
    #[allow(dead_code)]
    pub async fn delete_user(&self, user_id: Uuid) -> Result<u64> {
        let result = sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await
            .context("Failed to delete user")?;

        Ok(result.rows_affected())
    }

    /// Count admin users.
    #[allow(dead_code)]
    pub async fn count_admins(&self) -> Result<i64> {
        let count: Option<i64> =
            sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin'")
                .fetch_one(&self.pool)
                .await
                .context("Failed to count admins")?;

        Ok(count.unwrap_or(0))
    }

    // ── Session CRUD ───────────────────────────────────────────────

    /// Create a new session. Returns the session ID.
    #[allow(dead_code)]
    pub async fn create_session(&self, user_id: Uuid, expires_at: DateTime<Utc>) -> Result<Uuid> {
        let session_id = Uuid::new_v4();
        sqlx::query("INSERT INTO sessions (id, user_id, expires_at) VALUES ($1, $2, $3)")
            .bind(session_id)
            .bind(user_id)
            .bind(expires_at)
            .execute(&self.pool)
            .await
            .context("Failed to create session")?;

        Ok(session_id)
    }

    /// Get a session by ID (only if not expired).
    #[allow(dead_code)]
    pub async fn get_session(
        &self,
        session_id: Uuid,
    ) -> Result<Option<(Uuid, Uuid, DateTime<Utc>)>> {
        let row: Option<(Uuid, Uuid, DateTime<Utc>)> = sqlx::query_as(
            "SELECT id, user_id, expires_at FROM sessions
             WHERE id = $1 AND expires_at > NOW()",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get session")?;

        Ok(row)
    }

    /// Extend a session's expiry (sliding window).
    #[allow(dead_code)]
    pub async fn extend_session(
        &self,
        session_id: Uuid,
        new_expires_at: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query("UPDATE sessions SET expires_at = $1 WHERE id = $2")
            .bind(new_expires_at)
            .bind(session_id)
            .execute(&self.pool)
            .await
            .context("Failed to extend session")?;

        Ok(())
    }

    /// Delete a session.
    #[allow(dead_code)]
    pub async fn delete_session(&self, session_id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await
            .context("Failed to delete session")?;

        Ok(())
    }

    /// Delete all sessions for a user.
    #[allow(dead_code)]
    pub async fn delete_user_sessions(&self, user_id: Uuid) -> Result<u64> {
        let result = sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await
            .context("Failed to delete user sessions")?;

        Ok(result.rows_affected())
    }

    /// Delete expired sessions.
    #[allow(dead_code)]
    pub async fn cleanup_expired_sessions(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM sessions WHERE expires_at < NOW()")
            .execute(&self.pool)
            .await
            .context("Failed to cleanup expired sessions")?;

        Ok(result.rows_affected())
    }

    // ── API Key CRUD ───────────────────────────────────────────────

    /// Insert a new API key record (stores only the hash).
    #[allow(dead_code)]
    pub async fn insert_api_key(
        &self,
        user_id: Uuid,
        key_hash: &str,
        key_prefix: &str,
        label: &str,
    ) -> Result<Uuid> {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO api_keys (id, user_id, key_hash, key_prefix, label) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(user_id)
        .bind(key_hash)
        .bind(key_prefix)
        .bind(label)
        .execute(&self.pool)
        .await
        .context("Failed to insert API key")?;

        Ok(id)
    }

    /// Look up an API key by its hash. Returns (key_id, user_id).
    #[allow(dead_code)]
    pub async fn get_api_key_by_hash(&self, key_hash: &str) -> Result<Option<(Uuid, Uuid)>> {
        let row: Option<(Uuid, Uuid)> =
            sqlx::query_as("SELECT id, user_id FROM api_keys WHERE key_hash = $1")
                .bind(key_hash)
                .fetch_optional(&self.pool)
                .await
                .context("Failed to look up API key by hash")?;

        Ok(row)
    }

    /// List API keys for a user (excludes hash, returns metadata only).
    #[allow(dead_code)]
    pub async fn list_api_keys(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<(Uuid, String, String, Option<DateTime<Utc>>, DateTime<Utc>)>> {
        let rows = sqlx::query_as(
            "SELECT id, key_prefix, label, last_used, created_at
             FROM api_keys WHERE user_id = $1 ORDER BY created_at DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list API keys")?;

        Ok(rows)
    }

    /// Count API keys for a user.
    #[allow(dead_code)]
    pub async fn count_api_keys(&self, user_id: Uuid) -> Result<i64> {
        let count: Option<i64> =
            sqlx::query_scalar("SELECT COUNT(*) FROM api_keys WHERE user_id = $1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await
                .context("Failed to count API keys")?;

        Ok(count.unwrap_or(0))
    }

    /// Delete an API key (returns its hash for cache eviction).
    #[allow(dead_code)]
    pub async fn delete_api_key(&self, key_id: Uuid, user_id: Uuid) -> Result<Option<String>> {
        let hash: Option<String> = sqlx::query_scalar(
            "DELETE FROM api_keys WHERE id = $1 AND user_id = $2 RETURNING key_hash",
        )
        .bind(key_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to delete API key")?;

        Ok(hash)
    }

    /// Update last_used timestamp for an API key.
    #[allow(dead_code)]
    pub async fn touch_api_key(&self, key_id: Uuid) -> Result<()> {
        sqlx::query("UPDATE api_keys SET last_used = NOW() WHERE id = $1")
            .bind(key_id)
            .execute(&self.pool)
            .await
            .context("Failed to update API key last_used")?;

        Ok(())
    }

    // ── User Kiro Tokens ──────────────────────────────────────────

    /// Upsert a user's Kiro refresh token.
    #[allow(dead_code)]
    pub async fn upsert_kiro_token(
        &self,
        user_id: Uuid,
        refresh_token: &str,
        access_token: Option<&str>,
        token_expiry: Option<DateTime<Utc>>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO user_kiro_tokens (user_id, refresh_token, access_token, token_expiry, updated_at)
             VALUES ($1, $2, $3, $4, NOW())
             ON CONFLICT (user_id) DO UPDATE SET
               refresh_token = EXCLUDED.refresh_token,
               access_token = EXCLUDED.access_token,
               token_expiry = EXCLUDED.token_expiry,
               updated_at = NOW()",
        )
        .bind(user_id)
        .bind(refresh_token)
        .bind(access_token)
        .bind(token_expiry)
        .execute(&self.pool)
        .await
        .context("Failed to upsert Kiro token")?;

        Ok(())
    }

    /// Get a user's Kiro tokens.
    #[allow(dead_code)]
    pub async fn get_kiro_token(
        &self,
        user_id: Uuid,
    ) -> Result<Option<(String, Option<String>, Option<DateTime<Utc>>)>> {
        let row: Option<(String, Option<String>, Option<DateTime<Utc>>)> = sqlx::query_as(
            "SELECT refresh_token, access_token, token_expiry FROM user_kiro_tokens WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get Kiro token")?;

        Ok(row)
    }

    /// Delete a user's Kiro token.
    #[allow(dead_code)]
    pub async fn delete_kiro_token(&self, user_id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM user_kiro_tokens WHERE user_id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await
            .context("Failed to delete Kiro token")?;

        Ok(())
    }

    /// Get tokens that need refreshing (expiring within the next 5 minutes).
    #[allow(dead_code)]
    pub async fn get_expiring_kiro_tokens(&self) -> Result<Vec<(Uuid, String)>> {
        let rows: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT user_id, refresh_token FROM user_kiro_tokens
             WHERE token_expiry IS NOT NULL AND token_expiry < NOW() + INTERVAL '5 minutes'",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to get expiring Kiro tokens")?;

        Ok(rows)
    }

    /// Mark a Kiro token as expired (clear access_token, null expiry).
    #[allow(dead_code)]
    pub async fn mark_kiro_token_expired(&self, user_id: Uuid) -> Result<()> {
        sqlx::query(
            "UPDATE user_kiro_tokens SET access_token = NULL, token_expiry = NULL, updated_at = NOW()
             WHERE user_id = $1",
        )
        .bind(user_id)
        .execute(&self.pool)
        .await
        .context("Failed to mark Kiro token as expired")?;

        Ok(())
    }

    /// Store per-user OAuth client credentials.
    #[allow(dead_code)]
    pub async fn upsert_user_oauth_client(
        &self,
        user_id: Uuid,
        client_id: &str,
        client_secret: &str,
        sso_region: &str,
        start_url: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO user_kiro_tokens (user_id, refresh_token, oauth_client_id, oauth_client_secret, oauth_sso_region, oauth_start_url, updated_at)
             VALUES ($1, '', $2, $3, $4, $5, NOW())
             ON CONFLICT (user_id) DO UPDATE SET
               oauth_client_id = EXCLUDED.oauth_client_id,
               oauth_client_secret = EXCLUDED.oauth_client_secret,
               oauth_sso_region = EXCLUDED.oauth_sso_region,
               oauth_start_url = EXCLUDED.oauth_start_url,
               updated_at = NOW()",
        )
        .bind(user_id)
        .bind(client_id)
        .bind(client_secret)
        .bind(sso_region)
        .bind(start_url)
        .execute(&self.pool)
        .await
        .context("Failed to upsert user OAuth client")?;

        Ok(())
    }

    /// Get per-user OAuth client credentials.
    #[allow(dead_code)]
    pub async fn get_user_oauth_client(
        &self,
        user_id: Uuid,
    ) -> Result<Option<(String, String, String)>> {
        let row: Option<(Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT oauth_client_id, oauth_client_secret, oauth_sso_region
             FROM user_kiro_tokens WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get user OAuth client")?;

        Ok(
            row.and_then(|(id, secret, region)| match (id, secret, region) {
                (Some(id), Some(secret), Some(region)) => Some((id, secret, region)),
                _ => None,
            }),
        )
    }

    /// Clear per-user OAuth client credentials (before fresh registration).
    #[allow(dead_code)]
    pub async fn clear_user_oauth_client(&self, user_id: Uuid) -> Result<()> {
        sqlx::query(
            "UPDATE user_kiro_tokens SET
               oauth_client_id = NULL, oauth_client_secret = NULL, oauth_sso_region = NULL, oauth_start_url = NULL,
               updated_at = NOW()
             WHERE user_id = $1",
        )
        .bind(user_id)
        .execute(&self.pool)
        .await
        .context("Failed to clear user OAuth client")?;

        Ok(())
    }

    /// Get per-user SSO configuration (start URL and region).
    #[allow(dead_code)]
    pub async fn get_user_sso_config(&self, user_id: Uuid) -> Result<Option<(String, String)>> {
        let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT oauth_start_url, oauth_sso_region
             FROM user_kiro_tokens WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get user SSO config")?;

        Ok(row.and_then(|(url, region)| match (url, region) {
            (Some(url), Some(region)) => Some((url, region)),
            _ => None,
        }))
    }

    /// Get expiring tokens with per-user OAuth credentials for refresh.
    #[allow(dead_code)]
    pub async fn get_expiring_kiro_tokens_with_oauth(&self) -> Result<Vec<KiroTokenOAuthRow>> {
        let rows: Vec<KiroTokenOAuthRow> = sqlx::query_as(
            "SELECT user_id, refresh_token, oauth_client_id, oauth_client_secret, oauth_sso_region
                 FROM user_kiro_tokens
                 WHERE token_expiry IS NOT NULL AND token_expiry < NOW() + INTERVAL '5 minutes'",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to get expiring Kiro tokens with OAuth")?;

        Ok(rows)
    }

    // ── Domain Allowlist ──────────────────────────────────────────

    /// List all allowed domains.
    #[allow(dead_code)]
    pub async fn list_allowed_domains(&self) -> Result<Vec<(String, Option<Uuid>, DateTime<Utc>)>> {
        let rows: Vec<(String, Option<Uuid>, DateTime<Utc>)> = sqlx::query_as(
            "SELECT domain, added_by, created_at FROM allowed_domains ORDER BY domain",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to list allowed domains")?;

        Ok(rows)
    }

    /// Add an allowed domain. Stores lowercase.
    #[allow(dead_code)]
    pub async fn add_allowed_domain(&self, domain: &str, added_by: Uuid) -> Result<()> {
        let domain_lower = domain.to_lowercase();
        sqlx::query(
            "INSERT INTO allowed_domains (domain, added_by) VALUES ($1, $2)
             ON CONFLICT (domain) DO NOTHING",
        )
        .bind(&domain_lower)
        .bind(added_by)
        .execute(&self.pool)
        .await
        .context("Failed to add allowed domain")?;

        Ok(())
    }

    /// Remove an allowed domain.
    #[allow(dead_code)]
    pub async fn remove_allowed_domain(&self, domain: &str) -> Result<u64> {
        let domain_lower = domain.to_lowercase();
        let result = sqlx::query("DELETE FROM allowed_domains WHERE domain = $1")
            .bind(&domain_lower)
            .execute(&self.pool)
            .await
            .context("Failed to remove allowed domain")?;

        Ok(result.rows_affected())
    }

    /// Check if an email domain is allowed.
    /// Returns true if the allowlist is empty (bootstrap mode) or domain matches exactly.
    /// Uses a single query combining both checks.
    #[allow(dead_code)]
    pub async fn is_domain_allowed(&self, email: &str) -> Result<bool> {
        // Extract domain from email, lowercase
        let domain = email
            .rsplit_once('@')
            .map(|(_, d)| d.to_lowercase())
            .unwrap_or_default();

        // Single query: returns true if allowlist is empty OR domain is present
        let allowed: Option<bool> = sqlx::query_scalar(
            "SELECT CASE
               WHEN (SELECT COUNT(*) FROM allowed_domains) = 0 THEN true
               ELSE EXISTS(SELECT 1 FROM allowed_domains WHERE domain = $1)
             END",
        )
        .bind(&domain)
        .fetch_one(&self.pool)
        .await
        .context("Failed to check domain allowlist")?;

        Ok(allowed.unwrap_or(false))
    }

    /// Version 7 migration: per-user provider API keys and model routing overrides.
    async fn migrate_to_v7(&self) -> Result<()> {
        tracing::info!("Running database migration to version 7 (provider keys)...");

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v7 migration transaction")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS user_provider_keys (
                id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                provider_id TEXT NOT NULL CHECK (provider_id IN ('anthropic', 'openai', 'gemini')),
                api_key     TEXT NOT NULL,
                key_prefix  TEXT NOT NULL,
                label       TEXT NOT NULL DEFAULT '',
                created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE(user_id, provider_id)
            )",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create user_provider_keys table")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS model_routes (
                model_pattern TEXT PRIMARY KEY,
                provider_id   TEXT NOT NULL CHECK (provider_id IN ('kiro', 'anthropic', 'openai', 'gemini')),
                created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create model_routes table")?;

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(7_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 7")?;

        tx.commit().await.context("Failed to commit v7 migration")?;

        tracing::info!("Database migration to version 7 complete");
        Ok(())
    }

    /// Version 8 migration: provider OAuth tokens table.
    async fn migrate_to_v8(&self) -> Result<()> {
        tracing::info!("Running database migration to version 8 (provider OAuth tokens)...");

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v8 migration transaction")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS user_provider_tokens (
                id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                provider_id   TEXT NOT NULL CHECK (provider_id IN ('anthropic', 'gemini', 'openai')),
                access_token  TEXT NOT NULL,
                refresh_token TEXT NOT NULL DEFAULT '',
                expires_at    TIMESTAMPTZ NOT NULL,
                email         TEXT NOT NULL DEFAULT '',
                created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE(user_id, provider_id)
            )",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create user_provider_tokens table")?;

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(8_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 8")?;

        tx.commit().await.context("Failed to commit v8 migration")?;

        tracing::info!("Database migration to version 8 complete");
        Ok(())
    }

    /// Version 9 migration: Copilot tokens and provider priority tables.
    async fn migrate_to_v9(&self) -> Result<()> {
        tracing::info!(
            "Running database migration to version 9 (copilot tokens + provider priority)..."
        );

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v9 migration transaction")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS user_copilot_tokens (
                id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                github_token    TEXT NOT NULL,
                github_username TEXT,
                copilot_token   TEXT,
                copilot_plan    TEXT,
                base_url        TEXT,
                expires_at      TIMESTAMPTZ,
                refresh_in      BIGINT,
                created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE(user_id)
            )",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create user_copilot_tokens table")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS user_provider_priority (
                user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                provider_id TEXT NOT NULL,
                priority    INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (user_id, provider_id)
            )",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create user_provider_priority table")?;

        // Extend model_routes CHECK constraint to include 'copilot'
        sqlx::query(
            "ALTER TABLE model_routes
             DROP CONSTRAINT IF EXISTS model_routes_provider_id_check",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to drop old model_routes CHECK constraint")?;

        sqlx::query(
            "ALTER TABLE model_routes
             ADD CONSTRAINT model_routes_provider_id_check
             CHECK (provider_id IN ('kiro', 'anthropic', 'openai', 'gemini', 'copilot'))",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to add updated model_routes CHECK constraint")?;

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(9_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 9")?;

        tx.commit().await.context("Failed to commit v9 migration")?;

        tracing::info!("Database migration to version 9 complete");
        Ok(())
    }

    /// Version 10 migration: Add 'qwen' to user_provider_tokens CHECK constraint,
    /// add base_url column, and update model_routes CHECK.
    async fn migrate_to_v10(&self) -> Result<()> {
        tracing::info!("Running database migration to version 10 (qwen provider support)...");

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v10 migration transaction")?;

        // Drop old CHECK constraint on user_provider_tokens.provider_id
        sqlx::query(
            "ALTER TABLE user_provider_tokens
             DROP CONSTRAINT IF EXISTS user_provider_tokens_provider_id_check",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to drop old user_provider_tokens CHECK constraint")?;

        // Add updated CHECK constraint including 'qwen'
        sqlx::query(
            "ALTER TABLE user_provider_tokens
             ADD CONSTRAINT user_provider_tokens_provider_id_check
             CHECK (provider_id IN ('anthropic', 'gemini', 'openai', 'qwen'))",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to add updated user_provider_tokens CHECK constraint")?;

        // Add base_url column to user_provider_tokens (for Qwen resource_url)
        sqlx::query(
            "ALTER TABLE user_provider_tokens
             ADD COLUMN IF NOT EXISTS base_url TEXT",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to add base_url column to user_provider_tokens")?;

        // Update model_routes CHECK to include 'qwen'
        sqlx::query(
            "ALTER TABLE model_routes
             DROP CONSTRAINT IF EXISTS model_routes_provider_id_check",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to drop old model_routes CHECK constraint")?;

        sqlx::query(
            "ALTER TABLE model_routes
             ADD CONSTRAINT model_routes_provider_id_check
             CHECK (provider_id IN ('kiro', 'anthropic', 'openai', 'gemini', 'copilot', 'qwen'))",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to add updated model_routes CHECK constraint")?;

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(10_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 10")?;

        tx.commit()
            .await
            .context("Failed to commit v10 migration")?;

        tracing::info!("Database migration to version 10 complete");
        Ok(())
    }

    async fn migrate_to_v11(&self) -> Result<()> {
        tracing::info!(
            "Running database migration to version 11 (rename openai → openai_codex)..."
        );

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v11 migration transaction")?;

        // ── Drop CHECK constraints first (must happen before UPDATEs) ──

        sqlx::query(
            "ALTER TABLE user_provider_keys
             DROP CONSTRAINT IF EXISTS user_provider_keys_provider_id_check",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to drop user_provider_keys CHECK constraint")?;

        sqlx::query(
            "ALTER TABLE user_provider_tokens
             DROP CONSTRAINT IF EXISTS user_provider_tokens_provider_id_check",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to drop user_provider_tokens CHECK constraint")?;

        sqlx::query(
            "ALTER TABLE model_routes
             DROP CONSTRAINT IF EXISTS model_routes_provider_id_check",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to drop model_routes CHECK constraint")?;

        // ── Rename provider_id data in all tables ──────────────────

        sqlx::query("UPDATE user_provider_keys SET provider_id = 'openai_codex' WHERE provider_id = 'openai'")
            .execute(&mut *tx)
            .await
            .context("Failed to rename provider_id in user_provider_keys")?;

        sqlx::query("UPDATE user_provider_tokens SET provider_id = 'openai_codex' WHERE provider_id = 'openai'")
            .execute(&mut *tx)
            .await
            .context("Failed to rename provider_id in user_provider_tokens")?;

        sqlx::query("UPDATE user_provider_priority SET provider_id = 'openai_codex' WHERE provider_id = 'openai'")
            .execute(&mut *tx)
            .await
            .context("Failed to rename provider_id in user_provider_priority")?;

        sqlx::query(
            "UPDATE model_routes SET provider_id = 'openai_codex' WHERE provider_id = 'openai'",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to rename provider_id in model_routes")?;

        // ── Re-add CHECK constraints with 'openai_codex' ─────────

        sqlx::query(
            "ALTER TABLE user_provider_keys
             ADD CONSTRAINT user_provider_keys_provider_id_check
             CHECK (provider_id IN ('anthropic', 'openai_codex', 'gemini'))",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to add user_provider_keys CHECK constraint")?;

        sqlx::query(
            "ALTER TABLE user_provider_tokens
             ADD CONSTRAINT user_provider_tokens_provider_id_check
             CHECK (provider_id IN ('anthropic', 'gemini', 'openai_codex', 'qwen'))",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to add user_provider_tokens CHECK constraint")?;

        sqlx::query(
            "ALTER TABLE model_routes
             ADD CONSTRAINT model_routes_provider_id_check
             CHECK (provider_id IN ('kiro', 'anthropic', 'openai_codex', 'gemini', 'copilot', 'qwen'))",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to add model_routes CHECK constraint")?;

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(11_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 11")?;

        tx.commit()
            .await
            .context("Failed to commit v11 migration")?;

        tracing::info!("Database migration to version 11 complete");
        Ok(())
    }

    /// Version 12 migration: model registry table.
    async fn migrate_to_v12(&self) -> Result<()> {
        tracing::info!("Running database migration to version 12 (model registry)...");

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v12 migration transaction")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS model_registry (
                id                UUID PRIMARY KEY,
                provider_id       TEXT NOT NULL,
                model_id          TEXT NOT NULL,
                display_name      TEXT NOT NULL,
                prefixed_id       TEXT NOT NULL UNIQUE,
                context_length    INTEGER NOT NULL DEFAULT 0,
                max_output_tokens INTEGER NOT NULL DEFAULT 0,
                capabilities      JSONB NOT NULL DEFAULT '{}',
                enabled           BOOLEAN NOT NULL DEFAULT true,
                source            TEXT NOT NULL DEFAULT 'static',
                upstream_meta     JSONB,
                created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE (provider_id, model_id)
            )",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create model_registry table")?;

        // Partial index for fast lookups of enabled models
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_model_registry_enabled
             ON model_registry (provider_id, model_id)
             WHERE enabled = true",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create model_registry enabled index")?;

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(12_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 12")?;

        tx.commit()
            .await
            .context("Failed to commit v12 migration")?;

        tracing::info!("Database migration to version 12 complete");
        Ok(())
    }

    /// Version 13 migration: remove Gemini provider.
    async fn migrate_to_v13(&self) -> Result<()> {
        tracing::info!("Running database migration to version 13 (remove gemini provider)...");

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v13 migration transaction")?;

        // ── Delete Gemini rows from all provider tables ──────────

        sqlx::query("DELETE FROM user_provider_keys WHERE provider_id = 'gemini'")
            .execute(&mut *tx)
            .await
            .context("Failed to delete gemini from user_provider_keys")?;

        sqlx::query("DELETE FROM user_provider_tokens WHERE provider_id = 'gemini'")
            .execute(&mut *tx)
            .await
            .context("Failed to delete gemini from user_provider_tokens")?;

        sqlx::query("DELETE FROM model_routes WHERE provider_id = 'gemini'")
            .execute(&mut *tx)
            .await
            .context("Failed to delete gemini from model_routes")?;

        sqlx::query("DELETE FROM user_provider_priority WHERE provider_id = 'gemini'")
            .execute(&mut *tx)
            .await
            .context("Failed to delete gemini from user_provider_priority")?;

        sqlx::query("DELETE FROM model_registry WHERE provider_id = 'gemini'")
            .execute(&mut *tx)
            .await
            .context("Failed to delete gemini from model_registry")?;

        // ── Drop and re-add CHECK constraints without 'gemini' ──

        sqlx::query(
            "ALTER TABLE user_provider_keys
             DROP CONSTRAINT IF EXISTS user_provider_keys_provider_id_check",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to drop user_provider_keys CHECK constraint")?;

        sqlx::query(
            "ALTER TABLE user_provider_keys
             ADD CONSTRAINT user_provider_keys_provider_id_check
             CHECK (provider_id IN ('anthropic', 'openai_codex'))",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to add user_provider_keys CHECK constraint")?;

        sqlx::query(
            "ALTER TABLE user_provider_tokens
             DROP CONSTRAINT IF EXISTS user_provider_tokens_provider_id_check",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to drop user_provider_tokens CHECK constraint")?;

        sqlx::query(
            "ALTER TABLE user_provider_tokens
             ADD CONSTRAINT user_provider_tokens_provider_id_check
             CHECK (provider_id IN ('anthropic', 'openai_codex', 'qwen'))",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to add user_provider_tokens CHECK constraint")?;

        sqlx::query(
            "ALTER TABLE model_routes
             DROP CONSTRAINT IF EXISTS model_routes_provider_id_check",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to drop model_routes CHECK constraint")?;

        sqlx::query(
            "ALTER TABLE model_routes
             ADD CONSTRAINT model_routes_provider_id_check
             CHECK (provider_id IN ('kiro', 'anthropic', 'openai_codex', 'copilot', 'qwen'))",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to add model_routes CHECK constraint")?;

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(13_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 13")?;

        tx.commit()
            .await
            .context("Failed to commit v13 migration")?;

        tracing::info!("Database migration to version 13 complete");
        Ok(())
    }

    /// Version 14 migration: add per-user oauth_start_url column.
    async fn migrate_to_v14(&self) -> Result<()> {
        tracing::info!("Running database migration to version 14 (per-user SSO config)...");

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v14 migration transaction")?;

        // Add oauth_start_url column to user_kiro_tokens
        sqlx::query("ALTER TABLE user_kiro_tokens ADD COLUMN IF NOT EXISTS oauth_start_url TEXT")
            .execute(&mut *tx)
            .await
            .context("Failed to add oauth_start_url column")?;

        // Backfill from global config table
        sqlx::query(
            "UPDATE user_kiro_tokens SET oauth_start_url = (
                SELECT value FROM config WHERE key = 'oauth_start_url'
             ) WHERE oauth_start_url IS NULL AND oauth_sso_region IS NOT NULL",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to backfill oauth_start_url from global config")?;

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(14_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 14")?;

        tx.commit()
            .await
            .context("Failed to commit v14 migration")?;

        tracing::info!("Database migration to version 14 complete");
        Ok(())
    }

    /// Version 15 migration: password auth + 2FA tables.
    async fn migrate_to_v15(&self) -> Result<()> {
        tracing::info!("Running database migration to version 15 (password auth + 2FA)...");

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v15 migration transaction")?;

        // Add password/2FA columns to users table
        sqlx::query("ALTER TABLE users ADD COLUMN IF NOT EXISTS password_hash TEXT")
            .execute(&mut *tx)
            .await
            .context("Failed to add password_hash column")?;

        sqlx::query("ALTER TABLE users ADD COLUMN IF NOT EXISTS totp_secret TEXT")
            .execute(&mut *tx)
            .await
            .context("Failed to add totp_secret column")?;

        sqlx::query(
            "ALTER TABLE users ADD COLUMN IF NOT EXISTS totp_enabled BOOLEAN NOT NULL DEFAULT FALSE",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to add totp_enabled column")?;

        sqlx::query(
            "ALTER TABLE users ADD COLUMN IF NOT EXISTS auth_method TEXT NOT NULL DEFAULT 'google'",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to add auth_method column")?;

        sqlx::query(
            "ALTER TABLE users ADD COLUMN IF NOT EXISTS must_change_password BOOLEAN NOT NULL DEFAULT FALSE",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to add must_change_password column")?;

        // TOTP recovery codes table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS totp_recovery_codes (
                id UUID PRIMARY KEY,
                user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                code_hash TEXT NOT NULL,
                used BOOLEAN NOT NULL DEFAULT FALSE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create totp_recovery_codes table")?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_recovery_codes_user ON totp_recovery_codes(user_id)",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create recovery codes user index")?;

        // Pending 2FA logins table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS pending_2fa_logins (
                token UUID PRIMARY KEY,
                user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                expires_at TIMESTAMPTZ NOT NULL
            )",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create pending_2fa_logins table")?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_pending_2fa_user ON pending_2fa_logins(user_id)",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create pending 2FA user index")?;

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(15_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 15")?;

        tx.commit()
            .await
            .context("Failed to commit v15 migration")?;

        tracing::info!("Database migration to version 15 complete");
        Ok(())
    }

    /// Version 16 migration: drop MCP clients table and config keys.
    async fn migrate_to_v16(&self) -> Result<()> {
        tracing::info!("Running database migration to version 16 (drop MCP clients)...");

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v16 migration transaction")?;

        sqlx::query("DROP TABLE IF EXISTS mcp_clients")
            .execute(&mut *tx)
            .await
            .context("Failed to drop mcp_clients table")?;

        sqlx::query("DELETE FROM config WHERE key LIKE 'mcp_%'")
            .execute(&mut *tx)
            .await
            .context("Failed to remove MCP config keys")?;

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(16_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 16")?;

        tx.commit()
            .await
            .context("Failed to commit v16 migration")?;

        tracing::info!("Database migration to version 16 complete");
        Ok(())
    }

    /// Version 17 migration: add google_linked column to users table.
    async fn migrate_to_v17(&self) -> Result<()> {
        tracing::info!("Running database migration to version 17 (google_linked column)...");

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v17 migration transaction")?;

        sqlx::query("ALTER TABLE users ADD COLUMN IF NOT EXISTS google_linked BOOLEAN NOT NULL DEFAULT FALSE")
            .execute(&mut *tx)
            .await
            .context("Failed to add google_linked column")?;

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(17_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 17")?;

        tx.commit()
            .await
            .context("Failed to commit v17 migration")?;

        tracing::info!("Database migration to version 17 complete");
        Ok(())
    }

    /// Version 18 migration: seed provider OAuth client ID defaults into the
    /// config table for existing installations so they don't lose their working
    /// config after the hardcoded defaults were removed from `Config::with_defaults()`.
    async fn migrate_to_v18(&self) -> Result<()> {
        tracing::info!(
            "Running database migration to version 18 (seed provider OAuth client IDs)..."
        );

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v18 migration transaction")?;

        // INSERT ... ON CONFLICT DO NOTHING — only seed if the key is absent.
        let defaults = [
            ("qwen_oauth_client_id", "f0304373b74a44d2b584a3fb70ca9e56"),
            (
                "anthropic_oauth_client_id",
                "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
            ),
            ("openai_oauth_client_id", "app_EMoamEEZ73f0CkXaXp7hrann"),
        ];

        for (key, value) in &defaults {
            sqlx::query(
                "INSERT INTO config (key, value, value_type, description)
                 VALUES ($1, $2, 'string', $3)
                 ON CONFLICT (key) DO NOTHING",
            )
            .bind(key)
            .bind(value)
            .bind(format!("Default {} (seeded by v18 migration)", key))
            .execute(&mut *tx)
            .await
            .context(format!("Failed to seed config key '{}'", key))?;
        }

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(18_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 18")?;

        tx.commit()
            .await
            .context("Failed to commit v18 migration")?;

        tracing::info!("Database migration to version 18 complete");
        Ok(())
    }

    /// Version 19 migration: usage tracking table for token and cost analytics.
    async fn migrate_to_v19(&self) -> Result<()> {
        tracing::info!("Running database migration to version 19 (usage tracking)...");

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v19 migration transaction")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS usage_records (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                provider_id TEXT NOT NULL,
                model_id TEXT NOT NULL,
                input_tokens INTEGER NOT NULL CHECK (input_tokens >= 0),
                output_tokens INTEGER NOT NULL CHECK (output_tokens >= 0),
                cost DOUBLE PRECISION NOT NULL DEFAULT 0.0 CHECK (cost >= 0),
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create usage_records table")?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_usage_user_date ON usage_records(user_id, created_at)",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create idx_usage_user_date index")?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_usage_provider_model_date ON usage_records(provider_id, model_id, created_at)",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create idx_usage_provider_model_date index")?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_usage_date ON usage_records(created_at)")
            .execute(&mut *tx)
            .await
            .context("Failed to create idx_usage_date index")?;

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(19_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 19")?;

        tx.commit()
            .await
            .context("Failed to commit v19 migration")?;

        tracing::info!("Database migration to version 19 complete");
        Ok(())
    }

    /// Version 20 migration: multi-account support + admin provider pool.
    async fn migrate_to_v20(&self) -> Result<()> {
        tracing::info!("Running database migration to version 20 (multi-account + admin pool)...");

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v20 migration transaction")?;

        // 1. Drop existing unique constraint on user_provider_tokens(user_id, provider_id)
        sqlx::query(
            "ALTER TABLE user_provider_tokens
             DROP CONSTRAINT IF EXISTS user_provider_tokens_user_id_provider_id_key",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to drop user_provider_tokens unique constraint")?;

        // 2. Add account_label column
        sqlx::query(
            "ALTER TABLE user_provider_tokens
             ADD COLUMN IF NOT EXISTS account_label TEXT NOT NULL DEFAULT 'default'",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to add account_label column to user_provider_tokens")?;

        // 3. Add new unique constraint including account_label
        sqlx::query(
            "DO $$
            BEGIN
                IF NOT EXISTS (
                    SELECT 1 FROM pg_constraint
                    WHERE conname = 'user_provider_tokens_user_provider_label_key'
                ) THEN
                    ALTER TABLE user_provider_tokens
                    ADD CONSTRAINT user_provider_tokens_user_provider_label_key
                    UNIQUE (user_id, provider_id, account_label);
                END IF;
            END $$",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to add new unique constraint on user_provider_tokens")?;

        // 4. Create admin_provider_pool table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS admin_provider_pool (
                id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                provider_id   TEXT NOT NULL CHECK (provider_id IN ('kiro', 'anthropic', 'openai_codex', 'copilot', 'qwen')),
                account_label TEXT NOT NULL DEFAULT 'pool-1',
                api_key       TEXT NOT NULL,
                key_prefix    TEXT NOT NULL DEFAULT '',
                base_url      TEXT,
                enabled       BOOLEAN NOT NULL DEFAULT true,
                created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE (provider_id, account_label)
            )",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to create admin_provider_pool table")?;

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(20_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 20")?;

        tx.commit()
            .await
            .context("Failed to commit v20 migration")?;

        tracing::info!("Database migration to version 20 complete");
        Ok(())
    }

    // v21: DROP CHECK constraints on provider_id columns.
    // Validation moved to Rust via ProviderId::from_str() — the enum is the single
    // source of truth for valid providers. DB constraints were removed to avoid
    // maintaining parallel validation in both SQL and Rust.
    async fn migrate_to_v21(&self) -> Result<()> {
        tracing::info!(
            "Running database migration to version 21 (drop provider_id CHECK constraints)..."
        );

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v21 migration transaction")?;

        // Drop hardcoded CHECK constraints on provider_id columns.
        // Validation now happens in Rust via ProviderId::from_str().
        for table in &[
            "user_provider_tokens",
            "model_routes",
            "admin_provider_pool",
        ] {
            let drop_sql = format!(
                "DO $$
                DECLARE r RECORD;
                BEGIN
                    FOR r IN
                        SELECT con.conname
                        FROM pg_constraint con
                        JOIN pg_class rel ON rel.oid = con.conrelid
                        JOIN pg_attribute att ON att.attrelid = rel.oid
                            AND att.attnum = ANY(con.conkey)
                        WHERE rel.relname = '{table}'
                          AND att.attname = 'provider_id'
                          AND con.contype = 'c'
                    LOOP
                        EXECUTE 'ALTER TABLE {table} DROP CONSTRAINT ' || r.conname;
                    END LOOP;
                END $$"
            );
            sqlx::query(&drop_sql)
                .execute(&mut *tx)
                .await
                .context(format!("Failed to drop provider_id CHECK on {}", table))?;
        }

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(21_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 21")?;

        tx.commit()
            .await
            .context("Failed to commit v21 migration")?;

        tracing::info!("Database migration to version 21 complete");
        Ok(())
    }

    // v22: Remove all Qwen provider data.
    // The Qwen Coder provider is being fully removed from Harbangan.
    // This migration purges any remaining Qwen rows from provider tables.
    async fn migrate_to_v22(&self) -> Result<()> {
        tracing::info!("Running database migration to version 22 (remove qwen provider data)...");

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v22 migration transaction")?;

        // ── Delete Qwen rows from all provider tables ──────────

        sqlx::query("DELETE FROM user_provider_tokens WHERE provider_id = 'qwen'")
            .execute(&mut *tx)
            .await
            .context("Failed to delete qwen from user_provider_tokens")?;

        sqlx::query("DELETE FROM model_routes WHERE provider_id = 'qwen'")
            .execute(&mut *tx)
            .await
            .context("Failed to delete qwen from model_routes")?;

        sqlx::query("DELETE FROM model_registry WHERE provider_id = 'qwen'")
            .execute(&mut *tx)
            .await
            .context("Failed to delete qwen from model_registry")?;

        sqlx::query("DELETE FROM user_provider_priority WHERE provider_id = 'qwen'")
            .execute(&mut *tx)
            .await
            .context("Failed to delete qwen from user_provider_priority")?;

        sqlx::query("DELETE FROM admin_provider_pool WHERE provider_id = 'qwen'")
            .execute(&mut *tx)
            .await
            .context("Failed to delete qwen from admin_provider_pool")?;

        sqlx::query("DELETE FROM config WHERE key = 'qwen_oauth_client_id'")
            .execute(&mut *tx)
            .await
            .context("Failed to delete qwen_oauth_client_id from config")?;

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(22_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 22")?;

        tx.commit()
            .await
            .context("Failed to commit v22 migration")?;

        tracing::info!("Database migration to version 22 complete");
        Ok(())
    }

    async fn migrate_to_v23(&self) -> Result<()> {
        tracing::info!("Running database migration to version 23 (usage metric constraints)...");

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin v23 migration transaction")?;

        sqlx::query(
            "UPDATE usage_records
             SET input_tokens = GREATEST(input_tokens, 0),
                 output_tokens = GREATEST(output_tokens, 0),
                 cost = GREATEST(cost, 0.0)
             WHERE input_tokens < 0 OR output_tokens < 0 OR cost < 0.0",
        )
        .execute(&mut *tx)
        .await
        .context("Failed to normalize invalid usage metric rows")?;

        for (constraint_name, clause) in [
            (
                "usage_records_input_tokens_nonnegative",
                "CHECK (input_tokens >= 0)",
            ),
            (
                "usage_records_output_tokens_nonnegative",
                "CHECK (output_tokens >= 0)",
            ),
            ("usage_records_cost_nonnegative", "CHECK (cost >= 0)"),
        ] {
            let sql = format!(
                "DO $$ BEGIN
                     IF NOT EXISTS (
                         SELECT 1
                         FROM pg_constraint
                         WHERE conname = '{constraint_name}'
                     ) THEN
                         ALTER TABLE usage_records
                         ADD CONSTRAINT {constraint_name} {clause};
                     END IF;
                 END $$;"
            );
            sqlx::query(&sql)
                .execute(&mut *tx)
                .await
                .context(format!("Failed to add {constraint_name}"))?;
        }

        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(23_i32)
            .execute(&mut *tx)
            .await
            .context("Failed to record schema version 23")?;

        tx.commit()
            .await
            .context("Failed to commit v23 migration")?;

        tracing::info!("Database migration to version 23 complete");
        Ok(())
    }

    // ── Model Registry ───────────────────────────────────────────

    /// Get all models in the registry.
    #[allow(dead_code)]
    pub async fn get_all_registry_models(&self) -> Result<Vec<RegistryModel>> {
        let rows: Vec<RegistryModelRow> = sqlx::query_as(
            "SELECT id, provider_id, model_id, display_name, prefixed_id,
                    context_length, max_output_tokens, capabilities, enabled,
                    source, upstream_meta, created_at, updated_at
             FROM model_registry
             ORDER BY provider_id, display_name",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to get all registry models")?;

        Ok(rows.into_iter().map(Self::row_to_registry_model).collect())
    }

    /// Get only enabled models.
    #[allow(dead_code)]
    pub async fn get_enabled_registry_models(&self) -> Result<Vec<RegistryModel>> {
        let rows: Vec<RegistryModelRow> = sqlx::query_as(
            "SELECT id, provider_id, model_id, display_name, prefixed_id,
                    context_length, max_output_tokens, capabilities, enabled,
                    source, upstream_meta, created_at, updated_at
             FROM model_registry
             WHERE enabled = true
             ORDER BY provider_id, display_name",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to get enabled registry models")?;

        Ok(rows.into_iter().map(Self::row_to_registry_model).collect())
    }

    /// Upsert a single model into the registry.
    #[allow(dead_code)]
    pub async fn upsert_registry_model(&self, model: &RegistryModel) -> Result<()> {
        sqlx::query(
            "INSERT INTO model_registry
                (id, provider_id, model_id, display_name, prefixed_id,
                 context_length, max_output_tokens, capabilities, enabled,
                 source, upstream_meta, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
             ON CONFLICT (provider_id, model_id) DO UPDATE SET
               display_name      = EXCLUDED.display_name,
               prefixed_id       = EXCLUDED.prefixed_id,
               context_length    = EXCLUDED.context_length,
               max_output_tokens = EXCLUDED.max_output_tokens,
               capabilities      = EXCLUDED.capabilities,
               source            = EXCLUDED.source,
               upstream_meta     = EXCLUDED.upstream_meta,
               updated_at        = NOW()",
        )
        .bind(model.id)
        .bind(&model.provider_id)
        .bind(&model.model_id)
        .bind(&model.display_name)
        .bind(&model.prefixed_id)
        .bind(model.context_length)
        .bind(model.max_output_tokens)
        .bind(&model.capabilities)
        .bind(model.enabled)
        .bind(&model.source)
        .bind(&model.upstream_meta)
        .bind(model.created_at)
        .bind(model.updated_at)
        .execute(&self.pool)
        .await
        .context("Failed to upsert registry model")?;

        Ok(())
    }

    /// Bulk upsert models into the registry within a single transaction.
    #[allow(dead_code)]
    pub async fn bulk_upsert_registry_models(&self, models: &[RegistryModel]) -> Result<usize> {
        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin bulk upsert transaction")?;

        let mut count = 0usize;
        for model in models {
            sqlx::query(
                "INSERT INTO model_registry
                    (id, provider_id, model_id, display_name, prefixed_id,
                     context_length, max_output_tokens, capabilities, enabled,
                     source, upstream_meta, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
                 ON CONFLICT (provider_id, model_id) DO UPDATE SET
                   display_name      = EXCLUDED.display_name,
                   prefixed_id       = EXCLUDED.prefixed_id,
                   context_length    = EXCLUDED.context_length,
                   max_output_tokens = EXCLUDED.max_output_tokens,
                   capabilities      = EXCLUDED.capabilities,
                   source            = EXCLUDED.source,
                   upstream_meta     = EXCLUDED.upstream_meta,
                   updated_at        = NOW()",
            )
            .bind(model.id)
            .bind(&model.provider_id)
            .bind(&model.model_id)
            .bind(&model.display_name)
            .bind(&model.prefixed_id)
            .bind(model.context_length)
            .bind(model.max_output_tokens)
            .bind(&model.capabilities)
            .bind(model.enabled)
            .bind(&model.source)
            .bind(&model.upstream_meta)
            .bind(model.created_at)
            .bind(model.updated_at)
            .execute(&mut *tx)
            .await
            .context("Failed to upsert registry model in bulk")?;
            count += 1;
        }

        tx.commit().await.context("Failed to commit bulk upsert")?;

        Ok(count)
    }

    /// Toggle a model's enabled status.
    #[allow(dead_code)]
    pub async fn update_model_enabled(&self, id: Uuid, enabled: bool) -> Result<bool> {
        let result =
            sqlx::query("UPDATE model_registry SET enabled = $1, updated_at = NOW() WHERE id = $2")
                .bind(enabled)
                .bind(id)
                .execute(&self.pool)
                .await
                .context("Failed to update model enabled status")?;

        Ok(result.rows_affected() > 0)
    }

    /// Delete a model from the registry by ID.
    #[allow(dead_code)]
    pub async fn delete_registry_model(&self, id: Uuid) -> Result<bool> {
        let result = sqlx::query("DELETE FROM model_registry WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to delete registry model")?;

        Ok(result.rows_affected() > 0)
    }

    /// Update a model's display_name.
    #[allow(dead_code)]
    pub async fn update_model_display_name(&self, id: Uuid, display_name: &str) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE model_registry SET display_name = $1, updated_at = NOW() WHERE id = $2",
        )
        .bind(display_name)
        .bind(id)
        .execute(&self.pool)
        .await
        .context("Failed to update model display_name")?;

        Ok(result.rows_affected() > 0)
    }

    /// Remove all models for a given provider.
    #[allow(dead_code)]
    pub async fn clear_registry_by_provider(&self, provider_id: &str) -> Result<u64> {
        let result = sqlx::query("DELETE FROM model_registry WHERE provider_id = $1")
            .bind(provider_id)
            .execute(&self.pool)
            .await
            .context("Failed to clear registry by provider")?;

        Ok(result.rows_affected())
    }

    /// Convert a query row tuple into a `RegistryModel`.
    fn row_to_registry_model(row: RegistryModelRow) -> RegistryModel {
        RegistryModel {
            id: row.0,
            provider_id: row.1,
            model_id: row.2,
            display_name: row.3,
            prefixed_id: row.4,
            context_length: row.5,
            max_output_tokens: row.6,
            capabilities: row.7,
            enabled: row.8,
            source: row.9,
            upstream_meta: row.10,
            created_at: row.11,
            updated_at: row.12,
        }
    }

    // ── Copilot Tokens ────────────────────────────────────────────

    /// Upsert a user's Copilot tokens (GitHub OAuth + Copilot bearer).
    #[allow(dead_code)]
    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_copilot_tokens(
        &self,
        user_id: Uuid,
        github_token: &str,
        github_username: Option<&str>,
        copilot_token: Option<&str>,
        copilot_plan: Option<&str>,
        base_url: Option<&str>,
        expires_at: Option<DateTime<Utc>>,
        refresh_in: Option<i64>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO user_copilot_tokens
                (user_id, github_token, github_username, copilot_token, copilot_plan, base_url, expires_at, refresh_in, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())
             ON CONFLICT (user_id) DO UPDATE SET
               github_token    = EXCLUDED.github_token,
               github_username = EXCLUDED.github_username,
               copilot_token   = EXCLUDED.copilot_token,
               copilot_plan    = EXCLUDED.copilot_plan,
               base_url        = EXCLUDED.base_url,
               expires_at      = EXCLUDED.expires_at,
               refresh_in      = EXCLUDED.refresh_in,
               updated_at      = NOW()",
        )
        .bind(user_id)
        .bind(github_token)
        .bind(github_username)
        .bind(copilot_token)
        .bind(copilot_plan)
        .bind(base_url)
        .bind(expires_at)
        .bind(refresh_in)
        .execute(&self.pool)
        .await
        .context("Failed to upsert copilot tokens")?;

        Ok(())
    }

    /// Get a user's Copilot tokens.
    #[allow(dead_code)]
    pub async fn get_copilot_tokens(&self, user_id: Uuid) -> Result<Option<CopilotTokenRow>> {
        let row: Option<CopilotTokenQueryRow> = sqlx::query_as(
            "SELECT github_token, github_username, copilot_token, copilot_plan, base_url, expires_at, refresh_in
             FROM user_copilot_tokens
             WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get copilot tokens")?;

        Ok(row.map(
            |(
                github_token,
                github_username,
                copilot_token,
                copilot_plan,
                base_url,
                expires_at,
                refresh_in,
            )| {
                CopilotTokenRow {
                    user_id,
                    github_token,
                    github_username,
                    copilot_token,
                    copilot_plan,
                    base_url,
                    expires_at,
                    refresh_in,
                }
            },
        ))
    }

    /// Delete a user's Copilot tokens.
    #[allow(dead_code)]
    pub async fn delete_copilot_tokens(&self, user_id: Uuid) -> Result<u64> {
        let result = sqlx::query("DELETE FROM user_copilot_tokens WHERE user_id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await
            .context("Failed to delete copilot tokens")?;

        Ok(result.rows_affected())
    }

    /// Check if a user has Copilot tokens stored.
    #[allow(dead_code)]
    pub async fn has_copilot_token(&self, user_id: Uuid) -> Result<bool> {
        let count: Option<i64> =
            sqlx::query_scalar("SELECT COUNT(*) FROM user_copilot_tokens WHERE user_id = $1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await
                .context("Failed to check copilot token existence")?;

        Ok(count.unwrap_or(0) > 0)
    }

    /// Get all Copilot tokens expiring within 5 minutes (for background refresh).
    #[allow(dead_code)]
    pub async fn get_expiring_copilot_tokens(&self) -> Result<Vec<CopilotTokenRow>> {
        let rows: Vec<CopilotTokenExpiringRow> = sqlx::query_as(
            "SELECT user_id, github_token, github_username, copilot_token, copilot_plan, base_url, expires_at, refresh_in
             FROM user_copilot_tokens
             WHERE copilot_token IS NOT NULL
               AND expires_at IS NOT NULL
               AND expires_at < NOW() + INTERVAL '5 minutes'",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to get expiring copilot tokens")?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    user_id,
                    github_token,
                    github_username,
                    copilot_token,
                    copilot_plan,
                    base_url,
                    expires_at,
                    refresh_in,
                )| {
                    CopilotTokenRow {
                        user_id,
                        github_token,
                        github_username,
                        copilot_token,
                        copilot_plan,
                        base_url,
                        expires_at,
                        refresh_in,
                    }
                },
            )
            .collect())
    }

    // ── Provider Priority ─────────────────────────────────────────

    /// Get a user's provider priority list, ordered by priority (ascending).
    #[allow(dead_code)]
    pub async fn get_user_provider_priority(&self, user_id: Uuid) -> Result<Vec<(String, i32)>> {
        let rows: Vec<(String, i32)> = sqlx::query_as(
            "SELECT provider_id, priority
             FROM user_provider_priority
             WHERE user_id = $1
             ORDER BY priority ASC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to get user provider priority")?;

        Ok(rows)
    }

    /// Upsert a user's priority for a specific provider.
    #[allow(dead_code)]
    pub async fn upsert_user_provider_priority(
        &self,
        user_id: Uuid,
        provider_id: &str,
        priority: i32,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO user_provider_priority (user_id, provider_id, priority)
             VALUES ($1, $2, $3)
             ON CONFLICT (user_id, provider_id) DO UPDATE SET
               priority = EXCLUDED.priority",
        )
        .bind(user_id)
        .bind(provider_id)
        .bind(priority)
        .execute(&self.pool)
        .await
        .context("Failed to upsert user provider priority")?;

        Ok(())
    }

    // ── Provider Keys ─────────────────────────────────────────────

    /// Get a user's stored API key for a specific provider.
    /// Returns (api_key, key_prefix, label).
    #[allow(dead_code)]
    pub async fn get_user_provider_key(
        &self,
        user_id: Uuid,
        provider_id: &str,
    ) -> Result<Option<(String, String, String)>> {
        let row: Option<(String, String, String)> = sqlx::query_as(
            "SELECT api_key, key_prefix, label
             FROM user_provider_keys
             WHERE user_id = $1 AND provider_id = $2",
        )
        .bind(user_id)
        .bind(provider_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get user provider key")?;

        Ok(row)
    }

    /// Upsert a user's API key for a provider (one key per user per provider).
    #[allow(dead_code)]
    pub async fn upsert_user_provider_key(
        &self,
        user_id: Uuid,
        provider_id: &str,
        api_key: &str,
        key_prefix: &str,
        label: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO user_provider_keys (user_id, provider_id, api_key, key_prefix, label, updated_at)
             VALUES ($1, $2, $3, $4, $5, NOW())
             ON CONFLICT (user_id, provider_id) DO UPDATE SET
               api_key    = EXCLUDED.api_key,
               key_prefix = EXCLUDED.key_prefix,
               label      = EXCLUDED.label,
               updated_at = NOW()",
        )
        .bind(user_id)
        .bind(provider_id)
        .bind(api_key)
        .bind(key_prefix)
        .bind(label)
        .execute(&self.pool)
        .await
        .context("Failed to upsert user provider key")?;

        Ok(())
    }

    /// Delete a user's API key for a specific provider.
    #[allow(dead_code)]
    pub async fn delete_user_provider_key(&self, user_id: Uuid, provider_id: &str) -> Result<u64> {
        let result =
            sqlx::query("DELETE FROM user_provider_keys WHERE user_id = $1 AND provider_id = $2")
                .bind(user_id)
                .bind(provider_id)
                .execute(&self.pool)
                .await
                .context("Failed to delete user provider key")?;

        Ok(result.rows_affected())
    }

    /// Get all providers for which a user has configured a key.
    /// Returns a list of (provider_id, key_prefix, label).
    #[allow(dead_code)]
    pub async fn get_user_connected_providers(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<(String, String, String)>> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT provider_id, key_prefix, label
             FROM user_provider_keys
             WHERE user_id = $1
             ORDER BY provider_id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to get user connected providers")?;

        Ok(rows)
    }

    // ── Provider OAuth Tokens ──────────────────────────────────────

    /// Upsert a user's OAuth token for a provider (uses 'default' account label).
    /// Only overwrites `refresh_token` if the new value is non-empty (preserves existing on re-auth).
    #[allow(dead_code)]
    pub async fn upsert_user_provider_token(
        &self,
        user_id: Uuid,
        provider_id: &str,
        access_token: &str,
        refresh_token: &str,
        expires_at: DateTime<Utc>,
        email: &str,
    ) -> Result<()> {
        if refresh_token.is_empty() {
            // Preserve existing refresh_token
            sqlx::query(
                "INSERT INTO user_provider_tokens (user_id, provider_id, account_label, access_token, expires_at, email, updated_at)
                 VALUES ($1, $2, 'default', $3, $4, $5, NOW())
                 ON CONFLICT (user_id, provider_id, account_label) DO UPDATE SET
                   access_token  = EXCLUDED.access_token,
                   expires_at    = EXCLUDED.expires_at,
                   email         = CASE WHEN EXCLUDED.email = '' THEN user_provider_tokens.email ELSE EXCLUDED.email END,
                   updated_at    = NOW()",
            )
            .bind(user_id)
            .bind(provider_id)
            .bind(access_token)
            .bind(expires_at)
            .bind(email)
            .execute(&self.pool)
            .await
            .context("Failed to upsert user provider token")?;
        } else {
            sqlx::query(
                "INSERT INTO user_provider_tokens (user_id, provider_id, account_label, access_token, refresh_token, expires_at, email, updated_at)
                 VALUES ($1, $2, 'default', $3, $4, $5, $6, NOW())
                 ON CONFLICT (user_id, provider_id, account_label) DO UPDATE SET
                   access_token  = EXCLUDED.access_token,
                   refresh_token = EXCLUDED.refresh_token,
                   expires_at    = EXCLUDED.expires_at,
                   email         = CASE WHEN EXCLUDED.email = '' THEN user_provider_tokens.email ELSE EXCLUDED.email END,
                   updated_at    = NOW()",
            )
            .bind(user_id)
            .bind(provider_id)
            .bind(access_token)
            .bind(refresh_token)
            .bind(expires_at)
            .bind(email)
            .execute(&self.pool)
            .await
            .context("Failed to upsert user provider token")?;
        }

        Ok(())
    }

    /// Get a user's OAuth token for a specific provider.
    /// Returns (access_token, refresh_token, expires_at, email).
    #[allow(dead_code)]
    pub async fn get_user_provider_token(
        &self,
        user_id: Uuid,
        provider_id: &str,
    ) -> Result<Option<(String, String, DateTime<Utc>, String)>> {
        let row: Option<(String, String, DateTime<Utc>, String)> = sqlx::query_as(
            "SELECT access_token, refresh_token, expires_at, email
             FROM user_provider_tokens
             WHERE user_id = $1 AND provider_id = $2",
        )
        .bind(user_id)
        .bind(provider_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get user provider token")?;

        Ok(row)
    }

    /// Delete a user's OAuth token for a specific provider.
    #[allow(dead_code)]
    pub async fn delete_user_provider_token(
        &self,
        user_id: Uuid,
        provider_id: &str,
    ) -> Result<u64> {
        let result =
            sqlx::query("DELETE FROM user_provider_tokens WHERE user_id = $1 AND provider_id = $2")
                .bind(user_id)
                .bind(provider_id)
                .execute(&self.pool)
                .await
                .context("Failed to delete user provider token")?;

        Ok(result.rows_affected())
    }

    /// Set the base_url for a user's provider token.
    #[allow(dead_code)]
    pub async fn set_user_provider_base_url(
        &self,
        user_id: Uuid,
        provider_id: &str,
        base_url: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE user_provider_tokens
             SET base_url = $3, updated_at = NOW()
             WHERE user_id = $1 AND provider_id = $2",
        )
        .bind(user_id)
        .bind(provider_id)
        .bind(base_url)
        .execute(&self.pool)
        .await
        .context("Failed to set user provider base_url")?;
        Ok(())
    }

    /// Get the base_url for a user's provider token.
    #[allow(dead_code)]
    pub async fn get_user_provider_base_url(
        &self,
        user_id: Uuid,
        provider_id: &str,
    ) -> Result<Option<String>> {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT base_url FROM user_provider_tokens
             WHERE user_id = $1 AND provider_id = $2",
        )
        .bind(user_id)
        .bind(provider_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get user provider base_url")?;
        Ok(row.and_then(|(url,)| url))
    }

    /// Get all providers for which a user has OAuth tokens.
    /// Returns a list of (provider_id, email).
    #[allow(dead_code)]
    pub async fn get_user_connected_oauth_providers(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<(String, String)>> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT provider_id, email
             FROM user_provider_tokens
             WHERE user_id = $1
             ORDER BY provider_id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to get user connected OAuth providers")?;

        Ok(rows)
    }

    // ── Multi-Account Provider Tokens ─────────────────────────────

    /// Get all OAuth tokens for a user + provider (multi-account).
    #[allow(dead_code, clippy::type_complexity)]
    pub async fn get_all_user_provider_tokens(
        &self,
        user_id: Uuid,
        provider_id: &str,
    ) -> Result<Vec<UserProviderTokenRow>> {
        let rows: Vec<(
            Uuid,
            Uuid,
            String,
            String,
            String,
            String,
            DateTime<Utc>,
            String,
            Option<String>,
            DateTime<Utc>,
            DateTime<Utc>,
        )> = sqlx::query_as(
            "SELECT id, user_id, provider_id, account_label, access_token, refresh_token,
                    expires_at, email, base_url, created_at, updated_at
             FROM user_provider_tokens
             WHERE user_id = $1 AND provider_id = $2
             ORDER BY account_label",
        )
        .bind(user_id)
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to get all user provider tokens")?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    user_id,
                    provider_id,
                    account_label,
                    access_token,
                    refresh_token,
                    expires_at,
                    email,
                    base_url,
                    created_at,
                    updated_at,
                )| {
                    UserProviderTokenRow {
                        id,
                        user_id,
                        provider_id,
                        account_label,
                        access_token,
                        refresh_token,
                        expires_at,
                        email,
                        base_url,
                        created_at,
                        updated_at,
                    }
                },
            )
            .collect())
    }

    /// Upsert a user's OAuth token for a provider with a specific account label.
    #[allow(dead_code, clippy::too_many_arguments)]
    pub async fn upsert_user_provider_token_labeled(
        &self,
        user_id: Uuid,
        provider_id: &str,
        account_label: &str,
        access_token: &str,
        refresh_token: Option<&str>,
        expires_at: Option<DateTime<Utc>>,
        email: Option<&str>,
        base_url: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO user_provider_tokens
                (user_id, provider_id, account_label, access_token, refresh_token, expires_at, email, base_url, updated_at)
             VALUES ($1, $2, $3, $4, COALESCE($5, ''), COALESCE($6, NOW()), COALESCE($7, ''), $8, NOW())
             ON CONFLICT (user_id, provider_id, account_label) DO UPDATE SET
               access_token  = EXCLUDED.access_token,
               refresh_token = CASE WHEN EXCLUDED.refresh_token = '' THEN user_provider_tokens.refresh_token ELSE EXCLUDED.refresh_token END,
               expires_at    = EXCLUDED.expires_at,
               email         = CASE WHEN EXCLUDED.email = '' THEN user_provider_tokens.email ELSE EXCLUDED.email END,
               base_url      = COALESCE(EXCLUDED.base_url, user_provider_tokens.base_url),
               updated_at    = NOW()",
        )
        .bind(user_id)
        .bind(provider_id)
        .bind(account_label)
        .bind(access_token)
        .bind(refresh_token)
        .bind(expires_at)
        .bind(email)
        .bind(base_url)
        .execute(&self.pool)
        .await
        .context("Failed to upsert labeled user provider token")?;

        Ok(())
    }

    /// Delete a user's OAuth token for a specific provider + account label.
    #[allow(dead_code)]
    pub async fn delete_user_provider_token_labeled(
        &self,
        user_id: Uuid,
        provider_id: &str,
        account_label: &str,
    ) -> Result<()> {
        sqlx::query(
            "DELETE FROM user_provider_tokens
             WHERE user_id = $1 AND provider_id = $2 AND account_label = $3",
        )
        .bind(user_id)
        .bind(provider_id)
        .bind(account_label)
        .execute(&self.pool)
        .await
        .context("Failed to delete labeled user provider token")?;

        Ok(())
    }

    // ── Admin Provider Pool ───────────────────────────────────────

    /// Get all admin pool accounts for a specific provider.
    #[allow(dead_code, clippy::type_complexity)]
    pub async fn get_admin_pool_accounts(&self, provider_id: &str) -> Result<Vec<AdminPoolRow>> {
        let rows: Vec<(
            Uuid,
            String,
            String,
            String,
            String,
            Option<String>,
            bool,
            DateTime<Utc>,
            DateTime<Utc>,
        )> = sqlx::query_as(
            "SELECT id, provider_id, account_label, api_key, key_prefix, base_url,
                    enabled, created_at, updated_at
             FROM admin_provider_pool
             WHERE provider_id = $1 AND enabled = true
             ORDER BY account_label",
        )
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to get admin pool accounts")?;

        Ok(rows.into_iter().map(Self::row_to_admin_pool).collect())
    }

    /// Get all admin pool accounts across all providers.
    #[allow(dead_code, clippy::type_complexity)]
    pub async fn get_all_admin_pool_accounts(&self) -> Result<Vec<AdminPoolRow>> {
        let rows: Vec<(
            Uuid,
            String,
            String,
            String,
            String,
            Option<String>,
            bool,
            DateTime<Utc>,
            DateTime<Utc>,
        )> = sqlx::query_as(
            "SELECT id, provider_id, account_label, api_key, key_prefix, base_url,
                    enabled, created_at, updated_at
             FROM admin_provider_pool
             WHERE enabled = true
             ORDER BY provider_id, account_label",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to get all admin pool accounts")?;

        Ok(rows.into_iter().map(Self::row_to_admin_pool).collect())
    }

    /// Get all admin pool accounts including disabled (for admin UI management).
    #[allow(dead_code, clippy::type_complexity)]
    pub async fn get_all_admin_pool_accounts_include_disabled(&self) -> Result<Vec<AdminPoolRow>> {
        let rows: Vec<(
            Uuid,
            String,
            String,
            String,
            String,
            Option<String>,
            bool,
            DateTime<Utc>,
            DateTime<Utc>,
        )> = sqlx::query_as(
            "SELECT id, provider_id, account_label, api_key, key_prefix, base_url,
                    enabled, created_at, updated_at
             FROM admin_provider_pool
             ORDER BY provider_id, account_label",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to get all admin pool accounts")?;

        Ok(rows.into_iter().map(Self::row_to_admin_pool).collect())
    }

    /// Upsert an admin pool account for a provider.
    #[allow(dead_code)]
    pub async fn upsert_admin_pool_account(
        &self,
        provider_id: &str,
        account_label: &str,
        api_key: &str,
        key_prefix: &str,
        base_url: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO admin_provider_pool
                (provider_id, account_label, api_key, key_prefix, base_url, updated_at)
             VALUES ($1, $2, $3, $4, $5, NOW())
             ON CONFLICT (provider_id, account_label) DO UPDATE SET
               api_key    = EXCLUDED.api_key,
               key_prefix = EXCLUDED.key_prefix,
               base_url   = EXCLUDED.base_url,
               updated_at = NOW()",
        )
        .bind(provider_id)
        .bind(account_label)
        .bind(api_key)
        .bind(key_prefix)
        .bind(base_url)
        .execute(&self.pool)
        .await
        .context("Failed to upsert admin pool account")?;

        Ok(())
    }

    /// Delete an admin pool account by ID.
    #[allow(dead_code)]
    pub async fn delete_admin_pool_account(&self, id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM admin_provider_pool WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to delete admin pool account")?;

        Ok(())
    }

    /// Enable or disable an admin pool account.
    #[allow(dead_code)]
    pub async fn set_admin_pool_account_enabled(&self, id: Uuid, enabled: bool) -> Result<()> {
        sqlx::query(
            "UPDATE admin_provider_pool SET enabled = $2, updated_at = NOW() WHERE id = $1",
        )
        .bind(id)
        .bind(enabled)
        .execute(&self.pool)
        .await
        .context("Failed to set admin pool account enabled")?;

        Ok(())
    }

    /// Convert a raw admin_provider_pool row tuple to an AdminPoolRow.
    #[allow(clippy::type_complexity)]
    fn row_to_admin_pool(
        row: (
            Uuid,
            String,
            String,
            String,
            String,
            Option<String>,
            bool,
            DateTime<Utc>,
            DateTime<Utc>,
        ),
    ) -> AdminPoolRow {
        AdminPoolRow {
            id: row.0,
            provider_id: row.1,
            account_label: row.2,
            api_key: row.3,
            key_prefix: row.4,
            base_url: row.5,
            enabled: row.6,
            created_at: row.7,
            updated_at: row.8,
        }
    }

    /// Expose the connection pool for direct use in transactional operations.
    #[allow(dead_code)]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    // ── Usage Tracking ───────────────────────────────────────────

    /// Insert usage metrics for tracking token and cost analytics.
    pub async fn insert_usage_metric(
        &self,
        user_id: Uuid,
        provider_id: &str,
        model_id: &str,
        input_tokens: i32,
        output_tokens: i32,
        cost: f64,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO usage_records
             (user_id, provider_id, model_id, input_tokens, output_tokens, cost)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(user_id)
        .bind(provider_id)
        .bind(model_id)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(cost)
        .execute(&self.pool)
        .await
        .context("Failed to insert usage metric")?;

        Ok(())
    }

    /// Get usage summary grouped by a specified dimension.
    ///
    /// # Arguments
    /// * `user_id` - Optional user ID filter (None for all users)
    /// * `start` - Start date string (e.g., "2024-01-01")
    /// * `end` - End date string (e.g., "2024-12-31")
    /// * `group_by` - Grouping dimension: "day", "model", or "provider"
    pub async fn get_usage_summary(
        &self,
        user_id: Option<Uuid>,
        start: &str,
        end: &str,
        group_by: &str,
    ) -> Result<Vec<UsageSummary>> {
        // Validate group_by to prevent SQL injection
        let group_col = match group_by {
            "day" => "DATE(created_at)",
            "model" => "model_id",
            "provider" => "provider_id",
            _ => return Err(anyhow::anyhow!("Invalid group_by value: {}", group_by)),
        };

        let rows: Vec<(String, i64, i64, f64, i64)> = if let Some(uid) = user_id {
            sqlx::query_as(&format!(
                "SELECT {}::TEXT as group_key,
                        SUM(input_tokens) as total_input_tokens,
                        SUM(output_tokens) as total_output_tokens,
                        SUM(cost) as total_cost,
                        COUNT(*) as request_count
                 FROM usage_records
                 WHERE user_id = $1 AND DATE(created_at) BETWEEN $2::date AND $3::date
                 GROUP BY {}
                 ORDER BY group_key",
                group_col, group_col
            ))
            .bind(uid)
            .bind(start)
            .bind(end)
            .fetch_all(&self.pool)
            .await
            .context("Failed to get usage summary for user")?
        } else {
            sqlx::query_as(&format!(
                "SELECT {}::TEXT as group_key,
                        SUM(input_tokens) as total_input_tokens,
                        SUM(output_tokens) as total_output_tokens,
                        SUM(cost) as total_cost,
                        COUNT(*) as request_count
                 FROM usage_records
                 WHERE DATE(created_at) BETWEEN $1::date AND $2::date
                 GROUP BY {}
                 ORDER BY group_key",
                group_col, group_col
            ))
            .bind(start)
            .bind(end)
            .fetch_all(&self.pool)
            .await
            .context("Failed to get usage summary")?
        };

        Ok(rows
            .into_iter()
            .map(
                |(
                    group_key,
                    total_input_tokens,
                    total_output_tokens,
                    total_cost,
                    request_count,
                )| {
                    UsageSummary {
                        group_key,
                        total_input_tokens,
                        total_output_tokens,
                        total_cost,
                        request_count,
                    }
                },
            )
            .collect())
    }

    /// Get usage summary grouped by user (admin only).
    ///
    /// # Arguments
    /// * `start` - Start date string (e.g., "2024-01-01")
    /// * `end` - End date string (e.g., "2024-12-31")
    pub async fn get_usage_by_users(
        &self,
        start: &str,
        end: &str,
    ) -> Result<Vec<UserUsageSummary>> {
        let rows: Vec<(Uuid, String, i64, i64, f64, i64)> = sqlx::query_as(
            "SELECT u.id as user_id,
                    u.email,
                    COALESCE(SUM(ur.input_tokens), 0) as total_input_tokens,
                    COALESCE(SUM(ur.output_tokens), 0) as total_output_tokens,
                    COALESCE(SUM(ur.cost), 0.0) as total_cost,
                    COUNT(ur.id) as request_count
             FROM users u
             LEFT JOIN usage_records ur ON u.id = ur.user_id
                 AND DATE(ur.created_at) BETWEEN $1::date AND $2::date
             GROUP BY u.id, u.email
             ORDER BY total_cost DESC",
        )
        .bind(start)
        .bind(end)
        .fetch_all(&self.pool)
        .await
        .context("Failed to get usage by users")?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    user_id,
                    email,
                    total_input_tokens,
                    total_output_tokens,
                    total_cost,
                    request_count,
                )| {
                    UserUsageSummary {
                        user_id,
                        email,
                        total_input_tokens,
                        total_output_tokens,
                        total_cost,
                        request_count,
                    }
                },
            )
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Connect to a test PostgreSQL database. Returns None if DATABASE_URL is not set.
    async fn setup_test_db() -> Option<ConfigDb> {
        let url = std::env::var("DATABASE_URL").ok()?;
        let db = ConfigDb::connect(&url).await.ok()?;
        // Clean tables for a fresh test (order matters for FK constraints)
        sqlx::query("DELETE FROM api_keys")
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM user_kiro_tokens")
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM admin_provider_pool")
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM user_provider_tokens")
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM user_copilot_tokens")
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM user_provider_priority")
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM user_provider_keys")
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM sessions")
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM allowed_domains")
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM users")
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM config_history")
            .execute(&db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM config")
            .execute(&db.pool)
            .await
            .ok();
        Some(db)
    }

    fn create_test_config() -> Config {
        Config::with_defaults()
    }

    #[tokio::test]
    async fn test_connect_creates_tables() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        let count: Option<i64> = sqlx::query_scalar("SELECT COUNT(*) FROM schema_version")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(count, Some(1));
    }

    #[tokio::test]
    async fn test_insert_usage_metric_rejects_negative_values() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };

        let (user_id, _) = db
            .upsert_user("usage-negative@example.com", "Usage Negative", None)
            .await
            .unwrap();

        let result = db
            .insert_usage_metric(user_id, "anthropic", "claude-sonnet-4", -1, 10, 0.1)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_set_and_get() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        db.set("log_level", "debug", "test").await.unwrap();
        let val = db.get("log_level").await.unwrap();
        assert_eq!(val, Some("debug".to_string()));
    }

    #[tokio::test]
    async fn test_get_missing_key() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        let val = db.get("nonexistent").await.unwrap();
        assert_eq!(val, None);
    }

    #[tokio::test]
    async fn test_set_upsert() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        db.set("log_level", "info", "test").await.unwrap();
        db.set("log_level", "debug", "test").await.unwrap();
        let val = db.get("log_level").await.unwrap();
        assert_eq!(val, Some("debug".to_string()));
    }

    #[tokio::test]
    async fn test_get_all() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        db.set("key1", "val1", "test").await.unwrap();
        db.set("key2", "val2", "test").await.unwrap();
        let all = db.get_all().await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all.get("key1").unwrap(), "val1");
        assert_eq!(all.get("key2").unwrap(), "val2");
    }

    #[tokio::test]
    async fn test_set_encrypted_get_decrypted_roundtrip() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        let key = *aes_gcm::Key::<aes_gcm::Aes256Gcm>::from_slice(&[0x42u8; 32]);
        db.set_encrypted("secret_key", "my-secret-value", &key, "test")
            .await
            .unwrap();

        let decrypted = db.get_decrypted("secret_key", &key).await.unwrap();
        assert_eq!(decrypted, Some("my-secret-value".to_string()));
    }

    #[tokio::test]
    async fn test_get_decrypted_plain_value() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        let key = *aes_gcm::Key::<aes_gcm::Aes256Gcm>::from_slice(&[0x42u8; 32]);
        db.set("plain_key", "plain-value", "test").await.unwrap();

        let result = db.get_decrypted("plain_key", &key).await.unwrap();
        assert_eq!(result, Some("plain-value".to_string()));
    }

    #[tokio::test]
    async fn test_get_decrypted_nonexistent() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        let key = *aes_gcm::Key::<aes_gcm::Aes256Gcm>::from_slice(&[0x42u8; 32]);
        let result = db.get_decrypted("no_such_key", &key).await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_set_encrypted_wrong_key_fails_decrypt() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        let key1 = *aes_gcm::Key::<aes_gcm::Aes256Gcm>::from_slice(&[0x42u8; 32]);
        let key2 = *aes_gcm::Key::<aes_gcm::Aes256Gcm>::from_slice(&[0x99u8; 32]);
        db.set_encrypted("wrong_key_test", "secret", &key1, "test")
            .await
            .unwrap();

        let result = db.get_decrypted("wrong_key_test", &key2).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_set_encrypted_overwrites_plain() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        let key = *aes_gcm::Key::<aes_gcm::Aes256Gcm>::from_slice(&[0x42u8; 32]);
        db.set("upgrade_key", "old-plain", "test").await.unwrap();
        db.set_encrypted("upgrade_key", "new-secret", &key, "test")
            .await
            .unwrap();

        let decrypted = db.get_decrypted("upgrade_key", &key).await.unwrap();
        assert_eq!(decrypted, Some("new-secret".to_string()));
    }

    #[tokio::test]
    async fn test_get_history() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        db.set("log_level", "info", "init").await.unwrap();
        db.set("log_level", "debug", "web_ui").await.unwrap();
        let history = db.get_history(10).await.unwrap();
        assert_eq!(history.len(), 2);

        // Most recent first
        assert_eq!(history[0].key, "log_level");
        assert_eq!(history[0].new_value, "debug");
        assert_eq!(history[0].old_value, Some("info".to_string()));
        assert_eq!(history[0].source, "web_ui");

        assert_eq!(history[1].key, "log_level");
        assert_eq!(history[1].new_value, "info");
        assert_eq!(history[1].old_value, None);
        assert_eq!(history[1].source, "init");
    }

    #[tokio::test]
    async fn test_get_history_limit() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        for i in 0..5 {
            db.set("key", &format!("val{}", i), "test").await.unwrap();
        }
        let history = db.get_history(2).await.unwrap();
        assert_eq!(history.len(), 2);
    }

    #[tokio::test]
    async fn test_set_and_load_config() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        db.set("log_level", "info", "test").await.unwrap();
        db.set("fake_reasoning_enabled", "true", "test")
            .await
            .unwrap();
        db.set("truncation_recovery", "true", "test").await.unwrap();

        let mut loaded = create_test_config();
        loaded.log_level = "changed".to_string();

        db.load_into_config(&mut loaded).await.unwrap();

        assert_eq!(loaded.log_level, "info");
        assert!(loaded.fake_reasoning_enabled);
        assert!(loaded.truncation_recovery);
    }

    #[tokio::test]
    async fn test_load_into_config_debug_mode() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        db.set("debug_mode", "errors", "test").await.unwrap();
        let mut config = create_test_config();
        db.load_into_config(&mut config).await.unwrap();
        assert_eq!(config.debug_mode, DebugMode::Errors);
    }

    #[tokio::test]
    async fn test_is_setup_complete_false_when_no_users() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        assert!(!db.is_setup_complete().await);
    }

    #[tokio::test]
    async fn test_is_setup_complete_false_with_only_regular_user() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        // Insert a regular user directly (not via upsert which auto-promotes first user)
        let user_id = uuid::Uuid::new_v4();
        sqlx::query("INSERT INTO users (id, email, name, role) VALUES ($1, $2, $3, 'user')")
            .bind(user_id)
            .bind("user@example.com")
            .bind("Test User")
            .execute(&db.pool)
            .await
            .unwrap();
        assert!(!db.is_setup_complete().await);
    }

    #[tokio::test]
    async fn test_is_setup_complete_true_with_admin() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        // First user via upsert gets admin role
        let (_, role) = db
            .upsert_user("admin@example.com", "Admin", None)
            .await
            .unwrap();
        assert_eq!(role, "admin");
        assert!(db.is_setup_complete().await);
    }

    #[tokio::test]
    async fn test_get_refresh_token() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        assert_eq!(db.get_refresh_token().await.unwrap(), None);
        db.set("kiro_refresh_token", "my-token", "test")
            .await
            .unwrap();
        assert_eq!(
            db.get_refresh_token().await.unwrap(),
            Some("my-token".to_string())
        );
    }

    #[tokio::test]
    async fn test_load_into_config_numeric_fields_valid() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        db.set("streaming_timeout", "120", "test").await.unwrap();
        db.set("token_refresh_threshold", "600", "test")
            .await
            .unwrap();
        db.set("first_token_timeout", "30", "test").await.unwrap();
        db.set("http_max_connections", "500", "test").await.unwrap();
        db.set("http_connect_timeout", "10", "test").await.unwrap();
        db.set("http_request_timeout", "60", "test").await.unwrap();
        db.set("http_max_retries", "5", "test").await.unwrap();
        db.set("fake_reasoning_max_tokens", "8192", "test")
            .await
            .unwrap();
        db.set("tool_description_max_length", "4096", "test")
            .await
            .unwrap();

        let mut config = create_test_config();
        db.load_into_config(&mut config).await.unwrap();

        assert_eq!(config.streaming_timeout, 120);
        assert_eq!(config.token_refresh_threshold, 600);
        assert_eq!(config.first_token_timeout, 30);
        assert_eq!(config.http_max_connections, 500);
        assert_eq!(config.http_connect_timeout, 10);
        assert_eq!(config.http_request_timeout, 60);
        assert_eq!(config.http_max_retries, 5);
        assert_eq!(config.fake_reasoning_max_tokens, 8192);
        assert_eq!(config.tool_description_max_length, 4096);
    }

    #[tokio::test]
    async fn test_load_into_config_out_of_range_keeps_defaults() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        // All out-of-range: too high or zero
        db.set("streaming_timeout", "0", "test").await.unwrap();
        db.set("token_refresh_threshold", "100000", "test")
            .await
            .unwrap();
        db.set("first_token_timeout", "100000", "test")
            .await
            .unwrap();
        db.set("http_max_connections", "9999", "test")
            .await
            .unwrap();
        db.set("http_connect_timeout", "0", "test").await.unwrap();
        db.set("http_request_timeout", "100000", "test")
            .await
            .unwrap();
        db.set("http_max_retries", "99", "test").await.unwrap();
        db.set("fake_reasoning_max_tokens", "0", "test")
            .await
            .unwrap();
        db.set("tool_description_max_length", "0", "test")
            .await
            .unwrap();

        let mut config = create_test_config();
        let defaults = create_test_config();
        db.load_into_config(&mut config).await.unwrap();

        // All should remain at defaults
        assert_eq!(config.streaming_timeout, defaults.streaming_timeout);
        assert_eq!(
            config.token_refresh_threshold,
            defaults.token_refresh_threshold
        );
        assert_eq!(config.first_token_timeout, defaults.first_token_timeout);
        assert_eq!(config.http_max_connections, defaults.http_max_connections);
        assert_eq!(config.http_connect_timeout, defaults.http_connect_timeout);
        assert_eq!(config.http_request_timeout, defaults.http_request_timeout);
        assert_eq!(config.http_max_retries, defaults.http_max_retries);
        assert_eq!(
            config.fake_reasoning_max_tokens,
            defaults.fake_reasoning_max_tokens
        );
        assert_eq!(
            config.tool_description_max_length,
            defaults.tool_description_max_length
        );
    }

    #[tokio::test]
    async fn test_load_into_config_unparseable_keeps_defaults() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        db.set("http_max_retries", "abc", "test").await.unwrap();
        db.set("fake_reasoning_enabled", "not_bool", "test")
            .await
            .unwrap();

        let mut config = create_test_config();
        let defaults = create_test_config();
        db.load_into_config(&mut config).await.unwrap();

        assert_eq!(config.http_max_retries, defaults.http_max_retries);
        assert_eq!(
            config.fake_reasoning_enabled,
            defaults.fake_reasoning_enabled
        );
    }

    #[tokio::test]
    async fn test_load_into_config_boundary_values() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        // Test exact boundary values (min and max)
        db.set("http_max_retries", "0", "test").await.unwrap();
        db.set("http_max_connections", "1000", "test")
            .await
            .unwrap();
        db.set("fake_reasoning_max_tokens", "1000000", "test")
            .await
            .unwrap();

        let mut config = create_test_config();
        db.load_into_config(&mut config).await.unwrap();

        assert_eq!(config.http_max_retries, 0);
        assert_eq!(config.http_max_connections, 1000);
        assert_eq!(config.fake_reasoning_max_tokens, 1_000_000);
    }

    #[tokio::test]
    async fn test_load_into_config_ignores_unknown_keys() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        db.set("unknown_key", "whatever", "test").await.unwrap();
        let mut config = create_test_config();
        // Should not panic
        db.load_into_config(&mut config).await.unwrap();
    }

    // ── Domain validation tests ──────────────────────────────────────

    #[tokio::test]
    async fn test_domain_allowed_empty_allowlist() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        // No domains in allowlist → any domain should pass (bootstrap mode)
        let allowed = db.is_domain_allowed("user@anything.com").await.unwrap();
        assert!(allowed, "Empty allowlist should allow any domain");
    }

    #[tokio::test]
    async fn test_domain_allowed_exact_match() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        sqlx::query("INSERT INTO allowed_domains (domain) VALUES ($1)")
            .bind("example.com")
            .execute(&db.pool)
            .await
            .unwrap();

        let allowed = db.is_domain_allowed("user@example.com").await.unwrap();
        assert!(allowed, "Exact domain match should be allowed");
    }

    #[tokio::test]
    async fn test_domain_rejected_subdomain() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        sqlx::query("INSERT INTO allowed_domains (domain) VALUES ($1)")
            .bind("example.com")
            .execute(&db.pool)
            .await
            .unwrap();

        let allowed = db.is_domain_allowed("user@sub.example.com").await.unwrap();
        assert!(!allowed, "Subdomain should NOT match parent domain");
    }

    #[tokio::test]
    async fn test_domain_case_insensitive() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        sqlx::query("INSERT INTO allowed_domains (domain) VALUES ($1)")
            .bind("example.com")
            .execute(&db.pool)
            .await
            .unwrap();

        let allowed = db.is_domain_allowed("user@EXAMPLE.COM").await.unwrap();
        assert!(allowed, "Domain check should be case insensitive");
    }

    // ── First-user-admin concurrency test ────────────────────────────

    #[tokio::test]
    async fn test_upsert_user_concurrent_first_admin() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        let db = std::sync::Arc::new(db);

        // Spawn 10 concurrent upsert_user calls with different emails
        let mut handles = Vec::new();
        for i in 0..10 {
            let db = std::sync::Arc::clone(&db);
            handles.push(tokio::spawn(async move {
                db.upsert_user(
                    &format!("user{}@example.com", i),
                    &format!("User {}", i),
                    None,
                )
                .await
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await.unwrap());
        }

        // All should succeed
        let successes: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
        assert_eq!(successes.len(), 10, "All upserts should succeed");

        // Exactly one admin
        let admin_count = db.count_admins().await.unwrap();
        assert_eq!(
            admin_count, 1,
            "Exactly one user should be admin after concurrent inserts"
        );
    }

    // ── Provider OAuth Token Tests ───────────────────────────────────

    /// Helper: create a user and return their UUID.
    async fn create_test_user(db: &ConfigDb, email: &str) -> Uuid {
        let (user_id, _) = db.upsert_user(email, "Test User", None).await.unwrap();
        user_id
    }

    #[tokio::test]
    async fn test_upsert_and_get_provider_token() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        let user_id = create_test_user(&db, "token@example.com").await;
        let expires = Utc::now() + chrono::Duration::hours(1);

        db.upsert_user_provider_token(
            user_id,
            "anthropic",
            "access_123",
            "refresh_456",
            expires,
            "user@anthropic.com",
        )
        .await
        .unwrap();

        let row = db
            .get_user_provider_token(user_id, "anthropic")
            .await
            .unwrap();
        assert!(row.is_some());
        let (access, refresh, exp, email) = row.unwrap();
        assert_eq!(access, "access_123");
        assert_eq!(refresh, "refresh_456");
        assert_eq!(email, "user@anthropic.com");
        // Timestamps lose sub-microsecond precision in PG, just check it's close
        assert!((exp - expires).num_seconds().abs() < 2);
    }

    #[tokio::test]
    async fn test_upsert_provider_token_preserves_refresh_when_empty() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        let user_id = create_test_user(&db, "refresh@example.com").await;
        let expires = Utc::now() + chrono::Duration::hours(1);

        // First insert with a refresh token
        db.upsert_user_provider_token(
            user_id,
            "anthropic",
            "access_1",
            "refresh_original",
            expires,
            "user@anthropic.com",
        )
        .await
        .unwrap();

        // Second upsert with empty refresh_token — should preserve the original
        let expires2 = Utc::now() + chrono::Duration::hours(2);
        db.upsert_user_provider_token(
            user_id,
            "anthropic",
            "access_2",
            "",
            expires2,
            "user@anthropic.com",
        )
        .await
        .unwrap();

        let (access, refresh, _, _) = db
            .get_user_provider_token(user_id, "anthropic")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(access, "access_2");
        assert_eq!(refresh, "refresh_original");
    }

    #[tokio::test]
    async fn test_upsert_provider_token_overwrites_refresh_when_nonempty() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        let user_id = create_test_user(&db, "overwrite@example.com").await;
        let expires = Utc::now() + chrono::Duration::hours(1);

        db.upsert_user_provider_token(
            user_id,
            "openai_codex",
            "access_1",
            "refresh_old",
            expires,
            "user@openai.com",
        )
        .await
        .unwrap();

        db.upsert_user_provider_token(
            user_id,
            "openai_codex",
            "access_2",
            "refresh_new",
            expires,
            "user@openai.com",
        )
        .await
        .unwrap();

        let (_, refresh, _, _) = db
            .get_user_provider_token(user_id, "openai_codex")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(refresh, "refresh_new");
    }

    #[tokio::test]
    async fn test_get_provider_token_not_found() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        let user_id = create_test_user(&db, "empty@example.com").await;

        let row = db
            .get_user_provider_token(user_id, "anthropic")
            .await
            .unwrap();
        assert!(row.is_none());
    }

    #[tokio::test]
    async fn test_delete_provider_token() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        let user_id = create_test_user(&db, "delete@example.com").await;
        let expires = Utc::now() + chrono::Duration::hours(1);

        db.upsert_user_provider_token(
            user_id,
            "anthropic",
            "access",
            "refresh",
            expires,
            "a@b.com",
        )
        .await
        .unwrap();

        let deleted = db
            .delete_user_provider_token(user_id, "anthropic")
            .await
            .unwrap();
        assert_eq!(deleted, 1);

        let row = db
            .get_user_provider_token(user_id, "anthropic")
            .await
            .unwrap();
        assert!(row.is_none());
    }

    #[tokio::test]
    async fn test_delete_provider_token_not_found() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        let user_id = create_test_user(&db, "nodel@example.com").await;

        let deleted = db
            .delete_user_provider_token(user_id, "openai_codex")
            .await
            .unwrap();
        assert_eq!(deleted, 0);
    }

    #[tokio::test]
    async fn test_get_connected_oauth_providers() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        let user_id = create_test_user(&db, "multi@example.com").await;
        let expires = Utc::now() + chrono::Duration::hours(1);

        db.upsert_user_provider_token(
            user_id,
            "anthropic",
            "a1",
            "r1",
            expires,
            "me@anthropic.com",
        )
        .await
        .unwrap();
        db.upsert_user_provider_token(
            user_id,
            "openai_codex",
            "a2",
            "r2",
            expires,
            "me@openai.com",
        )
        .await
        .unwrap();

        let providers = db
            .get_user_connected_oauth_providers(user_id)
            .await
            .unwrap();
        assert_eq!(providers.len(), 2);
        // Ordered by provider_id alphabetically
        assert_eq!(
            providers[0],
            ("anthropic".to_string(), "me@anthropic.com".to_string())
        );
        assert_eq!(
            providers[1],
            ("openai_codex".to_string(), "me@openai.com".to_string())
        );
    }

    #[tokio::test]
    async fn test_get_connected_oauth_providers_empty() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        let user_id = create_test_user(&db, "none@example.com").await;

        let providers = db
            .get_user_connected_oauth_providers(user_id)
            .await
            .unwrap();
        assert!(providers.is_empty());
    }

    #[tokio::test]
    async fn test_provider_token_unique_per_user_provider() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        let user1 = create_test_user(&db, "user1@example.com").await;
        let user2 = create_test_user(&db, "user2@example.com").await;
        let expires = Utc::now() + chrono::Duration::hours(1);

        // Both users can have anthropic tokens
        db.upsert_user_provider_token(user1, "anthropic", "a1", "r1", expires, "u1@a.com")
            .await
            .unwrap();
        db.upsert_user_provider_token(user2, "anthropic", "a2", "r2", expires, "u2@a.com")
            .await
            .unwrap();

        let t1 = db
            .get_user_provider_token(user1, "anthropic")
            .await
            .unwrap()
            .unwrap();
        let t2 = db
            .get_user_provider_token(user2, "anthropic")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t1.0, "a1");
        assert_eq!(t2.0, "a2");
    }

    #[tokio::test]
    async fn test_migration_v20_adds_account_label() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };

        // Verify schema_version includes v20
        let max_version: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert!(max_version.unwrap_or(0) >= 20);

        // Verify account_label column exists on user_provider_tokens
        let col_exists: Option<i64> = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.columns
             WHERE table_name = 'user_provider_tokens' AND column_name = 'account_label'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(col_exists, Some(1));

        // Verify admin_provider_pool table exists
        let table_exists: Option<i64> = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.tables
             WHERE table_name = 'admin_provider_pool'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(table_exists, Some(1));

        // Verify new unique constraint exists
        let constraint_exists: Option<i64> = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pg_constraint
             WHERE conname = 'user_provider_tokens_user_provider_label_key'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(constraint_exists, Some(1));
    }

    #[tokio::test]
    async fn test_multi_account_token_crud() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        let user_id = create_test_user(&db, "multi@example.com").await;
        let expires = Utc::now() + chrono::Duration::hours(1);

        // Insert two accounts for the same provider
        db.upsert_user_provider_token_labeled(
            user_id,
            "anthropic",
            "work",
            "access_work",
            Some("refresh_work"),
            Some(expires),
            Some("work@anthropic.com"),
            None,
        )
        .await
        .unwrap();

        db.upsert_user_provider_token_labeled(
            user_id,
            "anthropic",
            "personal",
            "access_personal",
            Some("refresh_personal"),
            Some(expires),
            Some("personal@anthropic.com"),
            None,
        )
        .await
        .unwrap();

        // Retrieve all tokens for the provider
        let tokens = db
            .get_all_user_provider_tokens(user_id, "anthropic")
            .await
            .unwrap();
        assert_eq!(tokens.len(), 2);

        // Ordered by account_label: "personal" before "work"
        assert_eq!(tokens[0].account_label, "personal");
        assert_eq!(tokens[0].access_token, "access_personal");
        assert_eq!(tokens[1].account_label, "work");
        assert_eq!(tokens[1].access_token, "access_work");

        // Upsert updates existing labeled token
        db.upsert_user_provider_token_labeled(
            user_id,
            "anthropic",
            "work",
            "access_work_v2",
            None,
            Some(expires),
            None,
            None,
        )
        .await
        .unwrap();

        let tokens = db
            .get_all_user_provider_tokens(user_id, "anthropic")
            .await
            .unwrap();
        assert_eq!(tokens.len(), 2);
        let work_token = tokens.iter().find(|t| t.account_label == "work").unwrap();
        assert_eq!(work_token.access_token, "access_work_v2");
        // refresh_token preserved when None passed
        assert_eq!(work_token.refresh_token, "refresh_work");

        // Delete one account
        db.delete_user_provider_token_labeled(user_id, "anthropic", "personal")
            .await
            .unwrap();
        let tokens = db
            .get_all_user_provider_tokens(user_id, "anthropic")
            .await
            .unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].account_label, "work");

        // Legacy method still works (uses 'default' label)
        db.upsert_user_provider_token(
            user_id,
            "anthropic",
            "access_default",
            "refresh_default",
            expires,
            "default@anthropic.com",
        )
        .await
        .unwrap();
        let tokens = db
            .get_all_user_provider_tokens(user_id, "anthropic")
            .await
            .unwrap();
        assert_eq!(tokens.len(), 2); // "default" + "work"
    }

    #[tokio::test]
    async fn test_admin_pool_crud() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };

        // Insert pool accounts
        db.upsert_admin_pool_account("anthropic", "pool-1", "sk-ant-key1", "sk-ant-", None)
            .await
            .unwrap();
        db.upsert_admin_pool_account(
            "anthropic",
            "pool-2",
            "sk-ant-key2",
            "sk-ant-",
            Some("https://api.anthropic.com"),
        )
        .await
        .unwrap();
        db.upsert_admin_pool_account("openai_codex", "pool-1", "sk-oai-key1", "sk-oai-", None)
            .await
            .unwrap();

        // Get by provider
        let anthropic_accounts = db.get_admin_pool_accounts("anthropic").await.unwrap();
        assert_eq!(anthropic_accounts.len(), 2);
        assert_eq!(anthropic_accounts[0].account_label, "pool-1");
        assert_eq!(anthropic_accounts[1].account_label, "pool-2");
        assert_eq!(
            anthropic_accounts[1].base_url,
            Some("https://api.anthropic.com".to_string())
        );

        // Get all
        let all = db.get_all_admin_pool_accounts().await.unwrap();
        assert_eq!(all.len(), 3);

        // Upsert updates existing
        db.upsert_admin_pool_account("anthropic", "pool-1", "sk-ant-key1-v2", "sk-ant-", None)
            .await
            .unwrap();
        let accounts = db.get_admin_pool_accounts("anthropic").await.unwrap();
        assert_eq!(accounts[0].api_key, "sk-ant-key1-v2");

        // Disable an account
        let pool1_id = accounts[0].id;
        db.set_admin_pool_account_enabled(pool1_id, false)
            .await
            .unwrap();
        let accounts = db.get_admin_pool_accounts("anthropic").await.unwrap();
        assert!(!accounts[0].enabled);

        // Re-enable
        db.set_admin_pool_account_enabled(pool1_id, true)
            .await
            .unwrap();
        let accounts = db.get_admin_pool_accounts("anthropic").await.unwrap();
        assert!(accounts[0].enabled);

        // Delete by ID
        db.delete_admin_pool_account(pool1_id).await.unwrap();
        let accounts = db.get_admin_pool_accounts("anthropic").await.unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].account_label, "pool-2");
    }
}
