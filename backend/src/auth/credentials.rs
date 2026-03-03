use anyhow::{Context, Result};

use super::types::Credentials;
use crate::web_ui::config_db::ConfigDb;

/// Load credentials from the gateway's config database.
///
/// Reads refresh token, region, and OAuth client credentials from the config DB.
/// Returns an error if `client_id` or `client_secret` is missing (device code
/// OAuth setup must be completed first).
pub async fn load_from_config_db(config_db: &ConfigDb) -> Result<Credentials> {
    let refresh_token = config_db
        .get_refresh_token()
        .await?
        .context("No kiro_refresh_token found in config database")?;

    let region = config_db
        .get("kiro_region")
        .await?
        .unwrap_or_else(|| "us-east-1".to_string());

    // Load OAuth client credentials
    let client_id = config_db.get("oauth_client_id").await?;
    let client_secret = config_db.get("oauth_client_secret").await?;
    let sso_region = config_db.get("oauth_sso_region").await?;

    if client_id.is_none() || client_secret.is_none() {
        anyhow::bail!(
            "OAuth client credentials (client_id/client_secret) not found in config database. \
             Please complete the device code login via the web UI."
        );
    }

    tracing::info!("Loaded credentials from config DB (AWS SSO OIDC auth)");

    Ok(Credentials {
        refresh_token,
        access_token: None,
        expires_at: None,
        profile_arn: None,
        region,
        client_id,
        client_secret,
        sso_region,
        scopes: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};

    /// Parse datetime from various ISO 8601 formats
    fn parse_datetime(s: &str) -> Result<DateTime<Utc>> {
        // Handle Z suffix
        let normalized = if s.ends_with('Z') {
            s.replace('Z', "+00:00")
        } else {
            s.to_string()
        };

        DateTime::parse_from_rfc3339(&normalized)
            .map(|dt| dt.with_timezone(&Utc))
            .with_context(|| format!("Failed to parse datetime: {}", s))
    }

    #[test]
    fn test_parse_datetime() {
        // Test with Z suffix
        let dt = parse_datetime("2025-01-12T10:30:00Z").unwrap();
        assert_eq!(dt.to_rfc3339(), "2025-01-12T10:30:00+00:00");

        // Test with timezone
        let dt = parse_datetime("2025-01-12T10:30:00+00:00").unwrap();
        assert_eq!(dt.to_rfc3339(), "2025-01-12T10:30:00+00:00");
    }

    /// Helper to connect to the test database using DATABASE_URL.
    /// Returns None if DATABASE_URL is not set (skips database-dependent tests).
    async fn setup_test_db() -> Option<ConfigDb> {
        let url = std::env::var("DATABASE_URL").ok()?;
        ConfigDb::connect(&url).await.ok()
    }

    #[tokio::test]
    async fn test_load_from_config_db_with_oauth_creds() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping test_load_from_config_db_with_oauth_creds: DATABASE_URL not set");
            return;
        };
        db.set("kiro_refresh_token", "my-refresh-token", "test")
            .await
            .unwrap();
        db.set("kiro_region", "us-west-2", "test").await.unwrap();
        db.set("oauth_client_id", "my-client-id", "test")
            .await
            .unwrap();
        db.set("oauth_client_secret", "my-client-secret", "test")
            .await
            .unwrap();

        let creds = load_from_config_db(&db).await.unwrap();
        assert_eq!(creds.refresh_token, "my-refresh-token");
        assert_eq!(creds.region, "us-west-2");
        assert_eq!(creds.client_id.as_deref(), Some("my-client-id"));
        assert_eq!(creds.client_secret.as_deref(), Some("my-client-secret"));
    }

    #[tokio::test]
    async fn test_load_from_config_db_missing_oauth_creds() {
        let Some(db) = setup_test_db().await else {
            eprintln!(
                "Skipping test_load_from_config_db_missing_oauth_creds: DATABASE_URL not set"
            );
            return;
        };
        db.set("kiro_refresh_token", "my-refresh-token", "test")
            .await
            .unwrap();

        let result = load_from_config_db(&db).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("OAuth client credentials"));
    }

    #[tokio::test]
    async fn test_load_from_config_db_default_region() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping test_load_from_config_db_default_region: DATABASE_URL not set");
            return;
        };
        db.set("kiro_refresh_token", "token", "test").await.unwrap();
        db.set("oauth_client_id", "cid", "test").await.unwrap();
        db.set("oauth_client_secret", "csec", "test").await.unwrap();

        let creds = load_from_config_db(&db).await.unwrap();
        assert_eq!(creds.region, "us-east-1");
    }

    #[tokio::test]
    async fn test_load_from_config_db_missing_token() {
        let Some(db) = setup_test_db().await else {
            eprintln!("Skipping test_load_from_config_db_missing_token: DATABASE_URL not set");
            return;
        };

        let result = load_from_config_db(&db).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("kiro_refresh_token"));
    }
}
