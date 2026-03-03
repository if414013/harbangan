use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::{Config, DebugMode};

/// Tuple representing a user row: (id, email, name, picture_url, role, created_at).
#[allow(dead_code)]
pub type UserRow = (Uuid, String, String, Option<String>, String, DateTime<Utc>);

/// A record of a configuration change.
#[derive(Debug, Clone)]
pub struct ConfigChange {
    pub key: String,
    pub old_value: Option<String>,
    pub new_value: String,
    pub changed_at: String,
    pub source: String,
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

    /// Get all config key-value pairs.
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

        let all = self.get_all().await?;

        for (key, value) in &all {
            match key.as_str() {
                "server_host" => config.server_host = value.clone(),
                "server_port" => {
                    parse_ranged!(key, value, config.server_port, u16, 1, 65535);
                }
                "proxy_api_key" => { /* removed — no longer in Config */ }
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
                "tls_cert_path" => {
                    config.tls_cert_path = Some(std::path::PathBuf::from(value));
                }
                "tls_key_path" => {
                    config.tls_key_path = Some(std::path::PathBuf::from(value));
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

    /// Save initial setup configuration (proxy key, refresh token, region).
    /// All writes are wrapped in a single transaction for atomicity.
    #[allow(dead_code)]
    pub async fn save_initial_setup(
        &self,
        proxy_key: &str,
        refresh_token: &str,
        region: &str,
    ) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin transaction for initial setup")?;

        let keys_values: &[(&str, &str)] = &[
            ("proxy_api_key", proxy_key),
            ("kiro_refresh_token", refresh_token),
            ("kiro_region", region),
            ("setup_complete", "true"),
        ];

        for &(key, value) in keys_values {
            let old_value: Option<String> =
                sqlx::query_scalar("SELECT value FROM config WHERE key = $1")
                    .bind(key)
                    .fetch_optional(&mut *tx)
                    .await
                    .context("Failed to fetch old config value during setup")?;

            sqlx::query(
                "INSERT INTO config (key, value, updated_at)
                 VALUES ($1, $2, NOW())
                 ON CONFLICT(key) DO UPDATE SET value = EXCLUDED.value, updated_at = EXCLUDED.updated_at",
            )
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await
            .with_context(|| format!("Failed to upsert config key '{}' during setup", key))?;

            sqlx::query(
                "INSERT INTO config_history (key, old_value, new_value, source)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(key)
            .bind(&old_value)
            .bind(value)
            .bind("setup")
            .execute(&mut *tx)
            .await
            .with_context(|| {
                format!("Failed to record config history for '{}' during setup", key)
            })?;
        }

        tx.commit()
            .await
            .context("Failed to commit initial setup transaction")?;

        Ok(())
    }

    /// Get the stored Kiro refresh token.
    pub async fn get_refresh_token(&self) -> Result<Option<String>> {
        self.get("kiro_refresh_token").await
    }

    /// Save OAuth-based setup (all fields in one transaction).
    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)]
    pub async fn save_oauth_setup(
        &self,
        proxy_key: &str,
        refresh_token: &str,
        region: &str,
        client_id: &str,
        client_secret: &str,
        client_secret_expires_at: &str,
        start_url: &str,
    ) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin transaction for OAuth setup")?;

        let keys_values: &[(&str, &str)] = &[
            ("proxy_api_key", proxy_key),
            ("kiro_refresh_token", refresh_token),
            ("kiro_region", "us-east-1"),
            ("oauth_sso_region", region),
            ("oauth_client_id", client_id),
            ("oauth_client_secret", client_secret),
            ("oauth_client_secret_expires_at", client_secret_expires_at),
            ("oauth_start_url", start_url),
            ("setup_complete", "true"),
        ];

        for &(key, value) in keys_values {
            let old_value: Option<String> =
                sqlx::query_scalar("SELECT value FROM config WHERE key = $1")
                    .bind(key)
                    .fetch_optional(&mut *tx)
                    .await
                    .context("Failed to fetch old config value during OAuth setup")?;

            sqlx::query(
                "INSERT INTO config (key, value, updated_at)
                 VALUES ($1, $2, NOW())
                 ON CONFLICT(key) DO UPDATE SET value = EXCLUDED.value, updated_at = EXCLUDED.updated_at",
            )
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await
            .with_context(|| format!("Failed to upsert config key '{}' during OAuth setup", key))?;

            sqlx::query(
                "INSERT INTO config_history (key, old_value, new_value, source)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(key)
            .bind(&old_value)
            .bind(value)
            .bind("oauth_setup")
            .execute(&mut *tx)
            .await
            .with_context(|| {
                format!(
                    "Failed to record config history for '{}' during OAuth setup",
                    key
                )
            })?;
        }

        tx.commit()
            .await
            .context("Failed to commit OAuth setup transaction")?;

        Ok(())
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

    /// Expose the connection pool for direct use in transactional operations.
    #[allow(dead_code)]
    pub fn pool(&self) -> &PgPool {
        &self.pool
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
        db.set("server_port", "8000", "test").await.unwrap();
        db.set("fake_reasoning_enabled", "true", "test")
            .await
            .unwrap();
        db.set("truncation_recovery", "true", "test").await.unwrap();

        let mut loaded = create_test_config();
        loaded.log_level = "changed".to_string();
        loaded.server_port = 9999;

        db.load_into_config(&mut loaded).await.unwrap();

        assert_eq!(loaded.log_level, "info");
        assert_eq!(loaded.server_port, 8000);
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
    async fn test_save_initial_setup() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        db.save_initial_setup("my-key", "my-token", "us-west-2")
            .await
            .unwrap();

        assert_eq!(
            db.get("proxy_api_key").await.unwrap(),
            Some("my-key".to_string())
        );
        assert_eq!(
            db.get("kiro_refresh_token").await.unwrap(),
            Some("my-token".to_string())
        );
        assert_eq!(
            db.get("kiro_region").await.unwrap(),
            Some("us-west-2".to_string())
        );
        assert_eq!(
            db.get("setup_complete").await.unwrap(),
            Some("true".to_string())
        );
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
        db.set("server_port", "0", "test").await.unwrap();
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
        assert_eq!(config.server_port, defaults.server_port);
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
        db.set("server_port", "not_a_number", "test").await.unwrap();
        db.set("http_max_retries", "abc", "test").await.unwrap();
        db.set("fake_reasoning_enabled", "not_bool", "test")
            .await
            .unwrap();

        let mut config = create_test_config();
        let defaults = create_test_config();
        db.load_into_config(&mut config).await.unwrap();

        assert_eq!(config.server_port, defaults.server_port);
        assert_eq!(config.http_max_retries, defaults.http_max_retries);
        assert_eq!(
            config.fake_reasoning_enabled,
            defaults.fake_reasoning_enabled
        );
    }

    #[tokio::test]
    async fn test_load_into_config_tls_paths() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        db.set("tls_cert_path", "/etc/ssl/cert.pem", "test")
            .await
            .unwrap();
        db.set("tls_key_path", "/etc/ssl/key.pem", "test")
            .await
            .unwrap();

        let mut config = create_test_config();
        assert!(config.tls_cert_path.is_none());
        assert!(config.tls_key_path.is_none());

        db.load_into_config(&mut config).await.unwrap();

        assert_eq!(
            config.tls_cert_path,
            Some(std::path::PathBuf::from("/etc/ssl/cert.pem"))
        );
        assert_eq!(
            config.tls_key_path,
            Some(std::path::PathBuf::from("/etc/ssl/key.pem"))
        );
    }

    #[tokio::test]
    async fn test_load_into_config_boundary_values() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping: DATABASE_URL not set");
            return;
        };
        // Test exact boundary values (min and max)
        db.set("server_port", "1", "test").await.unwrap();
        db.set("http_max_retries", "0", "test").await.unwrap();
        db.set("http_max_connections", "1000", "test")
            .await
            .unwrap();
        db.set("fake_reasoning_max_tokens", "1000000", "test")
            .await
            .unwrap();

        let mut config = create_test_config();
        db.load_into_config(&mut config).await.unwrap();

        assert_eq!(config.server_port, 1);
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
}
