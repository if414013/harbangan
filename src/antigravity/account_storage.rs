//! JSON file persistence for antigravity accounts.
//!
//! Stores account credentials in a JSON file so they survive restarts.
//! Default location: ~/.config/kiro-gateway/accounts.json

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Serialisable account record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAccount {
    pub email: String,
    pub composite_refresh_token: String,
    pub added_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used: Option<chrono::DateTime<chrono::Utc>>,
}

/// Returns the default storage path: ~/.config/kiro-gateway/accounts.json
pub fn default_storage_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".config/kiro-gateway/accounts.json"))
}

/// Loads accounts from a JSON file. Returns empty vec if file doesn't exist.
pub fn load_accounts(path: &Path) -> Result<Vec<StoredAccount>> {
    if !path.exists() {
        return Ok(vec![]);
    }

    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read accounts file: {}", path.display()))?;

    if contents.trim().is_empty() {
        return Ok(vec![]);
    }

    let accounts: Vec<StoredAccount> = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse accounts file: {}", path.display()))?;

    Ok(accounts)
}

/// Saves accounts to a JSON file, creating parent directories if needed.
pub fn save_accounts(path: &Path, accounts: &[StoredAccount]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    let json = serde_json::to_string_pretty(accounts).context("Failed to serialise accounts")?;

    std::fs::write(path, json)
        .with_context(|| format!("Failed to write accounts file: {}", path.display()))?;

    Ok(())
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_storage_path() {
        let path = default_storage_path();
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.to_string_lossy().contains("accounts.json"));
    }

    #[test]
    fn test_load_nonexistent_file() {
        let path = Path::new("/tmp/nonexistent_kiro_test_accounts.json");
        let accounts = load_accounts(path).unwrap();
        assert!(accounts.is_empty());
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("kiro_test_{}", std::process::id()));
        let path = dir.join("accounts.json");

        let accounts = vec![
            StoredAccount {
                email: "a@test.com".into(),
                composite_refresh_token: "token_a|proj1|managed1".into(),
                added_at: chrono::Utc::now(),
                last_used: None,
            },
            StoredAccount {
                email: "b@test.com".into(),
                composite_refresh_token: "token_b".into(),
                added_at: chrono::Utc::now(),
                last_used: Some(chrono::Utc::now()),
            },
        ];

        save_accounts(&path, &accounts).unwrap();
        let loaded = load_accounts(&path).unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].email, "a@test.com");
        assert_eq!(loaded[1].email, "b@test.com");
        assert_eq!(loaded[0].composite_refresh_token, "token_a|proj1|managed1");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_empty_file() {
        let dir = std::env::temp_dir().join(format!("kiro_test_empty_{}", std::process::id()));
        let path = dir.join("accounts.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "").unwrap();

        let accounts = load_accounts(&path).unwrap();
        assert!(accounts.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_stored_account_serialization() {
        let account = StoredAccount {
            email: "test@test.com".into(),
            composite_refresh_token: "token".into(),
            added_at: chrono::Utc::now(),
            last_used: None,
        };

        let json = serde_json::to_string(&account).unwrap();
        assert!(json.contains("test@test.com"));
        // last_used should be skipped when None
        assert!(!json.contains("last_used"));
    }
}
