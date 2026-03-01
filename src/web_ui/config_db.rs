use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::params;

use crate::config::{Config, DebugMode};

/// A record of a configuration change.
#[derive(Debug, Clone)]
pub struct ConfigChange {
    pub key: String,
    pub old_value: Option<String>,
    pub new_value: String,
    pub changed_at: String,
    pub source: String,
}

/// SQLite-backed configuration persistence.
pub struct ConfigDb {
    conn: Mutex<rusqlite::Connection>,
}

impl ConfigDb {
    /// Open (or create) the config database at `path` and run migrations.
    /// On Unix systems, sets restrictive permissions (0o700 on parent dir, 0o600 on DB file).
    pub fn open(path: &Path) -> Result<Self> {
        let conn = rusqlite::Connection::open(path)
            .with_context(|| format!("Failed to open config database: {}", path.display()))?;

        // Set restrictive file permissions on Unix (DB contains secrets)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(parent) = path.parent() {
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).ok();
            }
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).ok();
        }

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.run_migrations()?;
        Ok(db)
    }

    /// Create tables if they don't already exist.
    fn run_migrations(&self) -> Result<()> {
        let conn = self.conn.lock().expect("config db mutex poisoned");

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version    INTEGER NOT NULL,
                applied_at TEXT    NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS config (
                key         TEXT PRIMARY KEY NOT NULL,
                value       TEXT NOT NULL,
                value_type  TEXT NOT NULL DEFAULT 'string',
                updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
                description TEXT
            );

            CREATE TABLE IF NOT EXISTS config_history (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                key        TEXT NOT NULL,
                old_value  TEXT,
                new_value  TEXT NOT NULL,
                changed_at TEXT NOT NULL DEFAULT (datetime('now')),
                source     TEXT NOT NULL DEFAULT 'web_ui'
            );",
        )
        .context("Failed to run config database migrations")?;

        // Record schema version 1 if not present
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
            .unwrap_or(0);

        if count == 0 {
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?)",
                params![1],
            )
            .context("Failed to insert schema version")?;
        }

        Ok(())
    }

    /// Get a single config value by key.
    pub fn get(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().expect("config db mutex poisoned");
        let mut stmt = conn
            .prepare("SELECT value FROM config WHERE key = ?")
            .context("Failed to prepare get query")?;

        let result = stmt.query_row(params![key], |row| row.get(0)).ok();

        Ok(result)
    }

    /// Upsert a config value and record the change in history.
    /// All operations (read old value, upsert, history insert, prune) run in a single transaction.
    pub fn set(&self, key: &str, value: &str, source: &str) -> Result<()> {
        let conn = self.conn.lock().expect("config db mutex poisoned");
        let tx = conn
            .unchecked_transaction()
            .context("Failed to begin transaction for config set")?;

        // Fetch old value for history
        let old_value: Option<String> = tx
            .query_row(
                "SELECT value FROM config WHERE key = ?",
                params![key],
                |row| row.get(0),
            )
            .ok();

        tx.execute(
            "INSERT INTO config (key, value, updated_at)
             VALUES (?, ?, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, value],
        )
        .with_context(|| format!("Failed to upsert config key '{}'", key))?;

        tx.execute(
            "INSERT INTO config_history (key, old_value, new_value, source)
             VALUES (?, ?, ?, ?)",
            params![key, old_value, value, source],
        )
        .with_context(|| format!("Failed to record config history for '{}'", key))?;

        // Prune old history entries, keeping the most recent 1000
        tx.execute(
            "DELETE FROM config_history WHERE id NOT IN (SELECT id FROM config_history ORDER BY id DESC LIMIT 1000)",
            [],
        )
        .context("Failed to prune config history")?;

        tx.commit()
            .context("Failed to commit config set transaction")?;

        Ok(())
    }

    /// Get all config key-value pairs.
    pub fn get_all(&self) -> Result<HashMap<String, String>> {
        let conn = self.conn.lock().expect("config db mutex poisoned");
        let mut stmt = conn
            .prepare("SELECT key, value FROM config")
            .context("Failed to prepare get_all query")?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .context("Failed to query all config")?;

        let mut map = HashMap::new();
        for row in rows {
            let (k, v) = row.context("Failed to read config row")?;
            map.insert(k, v);
        }
        Ok(map)
    }

    /// Get recent config change history.
    pub fn get_history(&self, limit: usize) -> Result<Vec<ConfigChange>> {
        let conn = self.conn.lock().expect("config db mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT key, old_value, new_value, changed_at, source
                 FROM config_history
                 ORDER BY id DESC
                 LIMIT ?",
            )
            .context("Failed to prepare history query")?;

        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(ConfigChange {
                    key: row.get(0)?,
                    old_value: row.get(1)?,
                    new_value: row.get(2)?,
                    changed_at: row.get(3)?,
                    source: row.get(4)?,
                })
            })
            .context("Failed to query config history")?;

        let mut changes = Vec::new();
        for row in rows {
            changes.push(row.context("Failed to read history row")?);
        }
        Ok(changes)
    }

    /// Overlay persisted config values onto an existing Config struct.
    pub fn load_into_config(&self, config: &mut Config) -> Result<()> {
        let all = self.get_all()?;

        for (key, value) in &all {
            match key.as_str() {
                "server_host" => config.server_host = value.clone(),
                "server_port" => {
                    if let Ok(v) = value.parse() {
                        config.server_port = v;
                    }
                }
                "proxy_api_key" => config.proxy_api_key = value.clone(),
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
                    }
                }
                "fake_reasoning_max_tokens" => {
                    if let Ok(v) = value.parse() {
                        config.fake_reasoning_max_tokens = v;
                    }
                }
                "truncation_recovery" => {
                    if let Ok(v) = value.parse() {
                        config.truncation_recovery = v;
                    }
                }
                "tool_description_max_length" => {
                    if let Ok(v) = value.parse() {
                        config.tool_description_max_length = v;
                    }
                }
                "first_token_timeout" => {
                    if let Ok(v) = value.parse() {
                        config.first_token_timeout = v;
                    }
                }
                "tls_enabled" => {
                    if let Ok(v) = value.parse() {
                        config.tls_enabled = v;
                    }
                }
                "tls_cert_path" => {
                    config.tls_cert_path = Some(std::path::PathBuf::from(value));
                }
                "tls_key_path" => {
                    config.tls_key_path = Some(std::path::PathBuf::from(value));
                }
                "kiro_cli_db_file" => {
                    config.kiro_cli_db_file = std::path::PathBuf::from(value);
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Check if initial setup has been completed (proxy_api_key and kiro_refresh_token both exist).
    pub fn is_setup_complete(&self) -> bool {
        let has_key = self.get("proxy_api_key").ok().flatten().is_some();
        let has_token = self.get("kiro_refresh_token").ok().flatten().is_some();
        has_key && has_token
    }

    /// Save initial setup configuration (proxy key, refresh token, region).
    /// All four writes are wrapped in a single transaction for atomicity.
    pub fn save_initial_setup(
        &self,
        proxy_key: &str,
        refresh_token: &str,
        region: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("config db mutex poisoned");
        let tx = conn
            .unchecked_transaction()
            .context("Failed to begin transaction for initial setup")?;

        let keys_values: &[(&str, &str)] = &[
            ("proxy_api_key", proxy_key),
            ("kiro_refresh_token", refresh_token),
            ("kiro_region", region),
            ("setup_complete", "true"),
        ];

        for &(key, value) in keys_values {
            let old_value: Option<String> = tx
                .query_row(
                    "SELECT value FROM config WHERE key = ?",
                    params![key],
                    |row| row.get(0),
                )
                .ok();

            tx.execute(
                "INSERT INTO config (key, value, updated_at)
                 VALUES (?, ?, datetime('now'))
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                params![key, value],
            )
            .with_context(|| format!("Failed to upsert config key '{}' during setup", key))?;

            tx.execute(
                "INSERT INTO config_history (key, old_value, new_value, source)
                 VALUES (?, ?, ?, ?)",
                params![key, old_value, value, "setup"],
            )
            .with_context(|| {
                format!("Failed to record config history for '{}' during setup", key)
            })?;
        }

        tx.commit()
            .context("Failed to commit initial setup transaction")?;

        Ok(())
    }

    /// Get the stored Kiro refresh token.
    pub fn get_refresh_token(&self) -> Result<Option<String>> {
        self.get("kiro_refresh_token")
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn create_test_db() -> (ConfigDb, PathBuf) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let path = std::env::temp_dir().join(format!(
            "test_config_db_{}_{}.sqlite",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let db = ConfigDb::open(&path).unwrap();
        (db, path)
    }

    fn create_test_config() -> Config {
        Config {
            proxy_api_key: "test-key".to_string(),
            ..Config::with_defaults()
        }
    }

    #[test]
    fn test_open_creates_tables() {
        let (db, _tmp) = create_test_db();
        let conn = db.conn.lock().unwrap();
        // Verify tables exist by querying them
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_set_and_get() {
        let (db, _tmp) = create_test_db();

        db.set("log_level", "debug", "test").unwrap();
        let val = db.get("log_level").unwrap();
        assert_eq!(val, Some("debug".to_string()));
    }

    #[test]
    fn test_get_missing_key() {
        let (db, _tmp) = create_test_db();
        let val = db.get("nonexistent").unwrap();
        assert_eq!(val, None);
    }

    #[test]
    fn test_set_upsert() {
        let (db, _tmp) = create_test_db();

        db.set("log_level", "info", "test").unwrap();
        db.set("log_level", "debug", "test").unwrap();

        let val = db.get("log_level").unwrap();
        assert_eq!(val, Some("debug".to_string()));
    }

    #[test]
    fn test_get_all() {
        let (db, _tmp) = create_test_db();

        db.set("key1", "val1", "test").unwrap();
        db.set("key2", "val2", "test").unwrap();

        let all = db.get_all().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all.get("key1").unwrap(), "val1");
        assert_eq!(all.get("key2").unwrap(), "val2");
    }

    #[test]
    fn test_get_history() {
        let (db, _tmp) = create_test_db();

        db.set("log_level", "info", "init").unwrap();
        db.set("log_level", "debug", "web_ui").unwrap();

        let history = db.get_history(10).unwrap();
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

    #[test]
    fn test_get_history_limit() {
        let (db, _tmp) = create_test_db();

        for i in 0..5 {
            db.set("key", &format!("val{}", i), "test").unwrap();
        }

        let history = db.get_history(2).unwrap();
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn test_set_and_load_config() {
        let (db, _tmp) = create_test_db();

        db.set("log_level", "info", "test").unwrap();
        db.set("server_port", "8000", "test").unwrap();
        db.set("fake_reasoning_enabled", "true", "test").unwrap();
        db.set("truncation_recovery", "true", "test").unwrap();

        let mut loaded = create_test_config();
        loaded.log_level = "changed".to_string();
        loaded.server_port = 9999;

        db.load_into_config(&mut loaded).unwrap();

        assert_eq!(loaded.log_level, "info");
        assert_eq!(loaded.server_port, 8000);
        assert_eq!(loaded.fake_reasoning_enabled, true);
        assert_eq!(loaded.truncation_recovery, true);
    }

    #[test]
    fn test_load_into_config_debug_mode() {
        let (db, _tmp) = create_test_db();

        db.set("debug_mode", "errors", "test").unwrap();

        let mut config = create_test_config();
        db.load_into_config(&mut config).unwrap();

        assert_eq!(config.debug_mode, DebugMode::Errors);
    }

    #[test]
    fn test_is_setup_complete_false_when_empty() {
        let (db, _tmp) = create_test_db();
        assert!(!db.is_setup_complete());
    }

    #[test]
    fn test_is_setup_complete_false_with_only_key() {
        let (db, _tmp) = create_test_db();
        db.set("proxy_api_key", "test-key", "test").unwrap();
        assert!(!db.is_setup_complete());
    }

    #[test]
    fn test_is_setup_complete_true() {
        let (db, _tmp) = create_test_db();
        db.set("proxy_api_key", "test-key", "test").unwrap();
        db.set("kiro_refresh_token", "test-token", "test").unwrap();
        assert!(db.is_setup_complete());
    }

    #[test]
    fn test_save_initial_setup() {
        let (db, _tmp) = create_test_db();
        db.save_initial_setup("my-key", "my-token", "us-west-2")
            .unwrap();

        assert_eq!(db.get("proxy_api_key").unwrap(), Some("my-key".to_string()));
        assert_eq!(
            db.get("kiro_refresh_token").unwrap(),
            Some("my-token".to_string())
        );
        assert_eq!(
            db.get("kiro_region").unwrap(),
            Some("us-west-2".to_string())
        );
        assert_eq!(db.get("setup_complete").unwrap(), Some("true".to_string()));
        assert!(db.is_setup_complete());
    }

    #[test]
    fn test_get_refresh_token() {
        let (db, _tmp) = create_test_db();

        assert_eq!(db.get_refresh_token().unwrap(), None);

        db.set("kiro_refresh_token", "my-token", "test").unwrap();
        assert_eq!(
            db.get_refresh_token().unwrap(),
            Some("my-token".to_string())
        );
    }

    #[test]
    fn test_load_into_config_ignores_unknown_keys() {
        let (db, _tmp) = create_test_db();

        db.set("unknown_key", "whatever", "test").unwrap();

        let mut config = create_test_config();
        // Should not panic
        db.load_into_config(&mut config).unwrap();
    }
}
