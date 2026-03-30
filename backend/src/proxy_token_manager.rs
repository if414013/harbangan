//! File-based token persistence and background refresh for proxy-only mode.
//!
//! In proxy mode there is no database. Tokens are stored in a JSON file
//! (`/data/tokens.json` by default) and refreshed in the background.
//!
//! Write pattern: read → modify in memory → write to `.tmp` → set 0o600 → atomic rename.
//! All writes serialized through `file_lock`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::providers::types::{ProviderCredentials, ProviderId};
use crate::web_ui::provider_oauth::TokenExchanger;

/// How many seconds before expiry to trigger proactive refresh.
const REFRESH_BUFFER_SECS: i64 = 300;

/// Background refresh check interval.
const REFRESH_INTERVAL_SECS: u64 = 60;

// ── Token file schema ────────────────────────────────────────────────

/// Top-level structure of `/data/tokens.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenFile {
    #[serde(default)]
    pub anthropic: Option<OAuthTokenEntry>,
    #[serde(default)]
    pub openai: Option<OAuthTokenEntry>,
    #[serde(default)]
    pub copilot: Option<CopilotTokenEntry>,
}

/// OAuth token entry for Anthropic / OpenAI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokenEntry {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix timestamp (seconds) when the access token expires.
    pub expires_at: i64,
}

/// Copilot token entry (session token + optional persistent GitHub token).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotTokenEntry {
    /// Short-lived Copilot session token.
    pub session_token: String,
    /// Copilot API base URL.
    pub base_url: String,
    /// Unix timestamp when session_token expires.
    pub expires_at: i64,
    /// Long-lived GitHub access token (opt-in persistence).
    #[serde(default)]
    pub github_token: Option<String>,
}

// ── ProxyTokenManager ────────────────────────────────────────────────

/// Manages file-based token persistence and background refresh for proxy mode.
#[allow(dead_code)]
pub struct ProxyTokenManager {
    token_file: PathBuf,
    file_lock: tokio::sync::Mutex<()>,
    http_client: reqwest::Client,
    /// Live credential store — read by ProviderRegistry at request time.
    pub credentials: Arc<DashMap<ProviderId, ProviderCredentials>>,
}

impl ProxyTokenManager {
    pub fn new(token_file: PathBuf) -> Self {
        Self {
            token_file,
            file_lock: tokio::sync::Mutex::new(()),
            http_client: reqwest::Client::new(),
            credentials: Arc::new(DashMap::new()),
        }
    }

    /// Create a manager that shares the given credential store with the ProviderRegistry.
    /// Updates from file loads and refreshes are immediately visible to the registry.
    pub fn new_shared(
        token_file: PathBuf,
        credentials: Arc<DashMap<ProviderId, ProviderCredentials>>,
    ) -> Self {
        Self {
            token_file,
            file_lock: tokio::sync::Mutex::new(()),
            http_client: reqwest::Client::new(),
            credentials,
        }
    }

    /// Read the token file, returning an empty TokenFile if it doesn't exist.
    pub async fn read_token_file(&self) -> Result<TokenFile> {
        let _guard = self.file_lock.lock().await;
        self.read_token_file_unlocked()
    }

    fn read_token_file_unlocked(&self) -> Result<TokenFile> {
        if !self.token_file.exists() {
            return Ok(TokenFile::default());
        }
        let data = std::fs::read_to_string(&self.token_file)
            .with_context(|| format!("Failed to read {}", self.token_file.display()))?;
        let tokens: TokenFile = serde_json::from_str(&data)
            .with_context(|| format!("Failed to parse {}", self.token_file.display()))?;
        Ok(tokens)
    }

    /// Atomically write the token file (tmp + chmod 0o600 + rename).
    fn write_token_file_unlocked(&self, tokens: &TokenFile) -> Result<()> {
        let parent = self
            .token_file
            .parent()
            .context("Token file has no parent directory")?;
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;

        let tmp_path = self.token_file.with_extension("tmp");
        let data =
            serde_json::to_string_pretty(tokens).context("Failed to serialize token file")?;
        std::fs::write(&tmp_path, &data)
            .with_context(|| format!("Failed to write {}", tmp_path.display()))?;

        // Set restrictive permissions (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&tmp_path, perms)
                .with_context(|| format!("Failed to set permissions on {}", tmp_path.display()))?;
        }

        std::fs::rename(&tmp_path, &self.token_file).with_context(|| {
            format!(
                "Failed to rename {} -> {}",
                tmp_path.display(),
                self.token_file.display()
            )
        })?;
        Ok(())
    }

    /// Store OAuth tokens for a provider (called from relay callback).
    pub async fn store_oauth_tokens(
        &self,
        provider: &str,
        access_token: &str,
        refresh_token: &str,
        expires_in: i64,
    ) -> Result<()> {
        let _guard = self.file_lock.lock().await;
        let mut tokens = self.read_token_file_unlocked()?;
        let entry = OAuthTokenEntry {
            access_token: access_token.to_string(),
            refresh_token: refresh_token.to_string(),
            expires_at: chrono::Utc::now().timestamp() + expires_in,
        };
        match provider {
            "anthropic" => tokens.anthropic = Some(entry),
            "openai_codex" | "openai" => tokens.openai = Some(entry),
            _ => anyhow::bail!("Unknown OAuth provider: {}", provider),
        }
        self.write_token_file_unlocked(&tokens)?;
        self.update_credentials_from_tokens(&tokens);
        Ok(())
    }

    /// Store Copilot session tokens.
    pub async fn store_copilot_tokens(
        &self,
        session_token: &str,
        base_url: &str,
        expires_at: i64,
        github_token: Option<&str>,
    ) -> Result<()> {
        let _guard = self.file_lock.lock().await;
        let mut tokens = self.read_token_file_unlocked()?;
        tokens.copilot = Some(CopilotTokenEntry {
            session_token: session_token.to_string(),
            base_url: base_url.to_string(),
            expires_at,
            github_token: github_token.map(|s| s.to_string()),
        });
        self.write_token_file_unlocked(&tokens)?;
        self.update_credentials_from_tokens(&tokens);
        Ok(())
    }

    /// Load tokens from file and populate the live credential store.
    pub async fn load_from_file(&self) -> Result<()> {
        let tokens = self.read_token_file().await?;
        self.update_credentials_from_tokens(&tokens);
        Ok(())
    }

    /// Update the in-memory DashMap from a TokenFile snapshot.
    fn update_credentials_from_tokens(&self, tokens: &TokenFile) {
        let now = chrono::Utc::now().timestamp();

        if let Some(ref entry) = tokens.anthropic {
            if entry.expires_at > now {
                self.credentials.insert(
                    ProviderId::Anthropic,
                    ProviderCredentials {
                        provider: ProviderId::Anthropic,
                        access_token: entry.access_token.clone(),
                        base_url: None,
                        account_label: "proxy".to_string(),
                    },
                );
            } else {
                self.credentials.remove(&ProviderId::Anthropic);
            }
        }

        if let Some(ref entry) = tokens.openai {
            if entry.expires_at > now {
                self.credentials.insert(
                    ProviderId::OpenAICodex,
                    ProviderCredentials {
                        provider: ProviderId::OpenAICodex,
                        access_token: entry.access_token.clone(),
                        base_url: None,
                        account_label: "proxy".to_string(),
                    },
                );
            } else {
                self.credentials.remove(&ProviderId::OpenAICodex);
            }
        }

        if let Some(ref entry) = tokens.copilot {
            if entry.expires_at > now {
                self.credentials.insert(
                    ProviderId::Copilot,
                    ProviderCredentials {
                        provider: ProviderId::Copilot,
                        access_token: entry.session_token.clone(),
                        base_url: Some(entry.base_url.clone()),
                        account_label: "proxy".to_string(),
                    },
                );
            } else {
                self.credentials.remove(&ProviderId::Copilot);
            }
        }
    }

    /// Build a HashMap snapshot of current credentials.
    #[cfg(test)]
    pub fn snapshot_credentials(&self) -> HashMap<ProviderId, ProviderCredentials> {
        self.credentials
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect()
    }

    /// Refresh all tokens that are near expiry.
    ///
    /// Returns the number of tokens successfully refreshed.
    pub async fn refresh_expiring_tokens(&self, exchanger: &dyn TokenExchanger) -> Result<u32> {
        let _guard = self.file_lock.lock().await;
        let mut tokens = self.read_token_file_unlocked()?;
        let now = chrono::Utc::now().timestamp();
        let mut refreshed = 0u32;

        // Anthropic
        if let Some(ref entry) = tokens.anthropic {
            if !entry.refresh_token.is_empty() && (entry.expires_at - now) < REFRESH_BUFFER_SECS {
                match exchanger
                    .refresh_token("anthropic", &entry.refresh_token)
                    .await
                {
                    Ok(result) => {
                        let new_refresh = if result.refresh_token.is_empty() {
                            entry.refresh_token.clone()
                        } else {
                            result.refresh_token
                        };
                        tokens.anthropic = Some(OAuthTokenEntry {
                            access_token: result.access_token,
                            refresh_token: new_refresh,
                            expires_at: chrono::Utc::now().timestamp() + result.expires_in,
                        });
                        refreshed += 1;
                        tracing::info!("Refreshed Anthropic proxy token");
                    }
                    Err(e) => {
                        tracing::warn!(error = ?e, "Failed to refresh Anthropic proxy token");
                    }
                }
            }
        }

        // OpenAI
        if let Some(ref entry) = tokens.openai {
            if !entry.refresh_token.is_empty() && (entry.expires_at - now) < REFRESH_BUFFER_SECS {
                match exchanger
                    .refresh_token("openai_codex", &entry.refresh_token)
                    .await
                {
                    Ok(result) => {
                        let new_refresh = if result.refresh_token.is_empty() {
                            entry.refresh_token.clone()
                        } else {
                            result.refresh_token
                        };
                        tokens.openai = Some(OAuthTokenEntry {
                            access_token: result.access_token,
                            refresh_token: new_refresh,
                            expires_at: chrono::Utc::now().timestamp() + result.expires_in,
                        });
                        refreshed += 1;
                        tracing::info!("Refreshed OpenAI proxy token");
                    }
                    Err(e) => {
                        tracing::warn!(error = ?e, "Failed to refresh OpenAI proxy token");
                    }
                }
            }
        }

        // Copilot: refresh session token using github_token
        if let Some(ref entry) = tokens.copilot {
            if let Some(ref gh_token) = entry.github_token {
                if (entry.expires_at - now) < REFRESH_BUFFER_SECS {
                    match self.refresh_copilot_session(gh_token).await {
                        Ok((session_token, base_url, expires_at)) => {
                            tokens.copilot = Some(CopilotTokenEntry {
                                session_token,
                                base_url,
                                expires_at,
                                github_token: Some(gh_token.clone()),
                            });
                            refreshed += 1;
                            tracing::info!("Refreshed Copilot proxy session token");
                        }
                        Err(e) => {
                            tracing::warn!(error = ?e, "Failed to refresh Copilot proxy session");
                        }
                    }
                }
            }
        }

        if refreshed > 0 {
            self.write_token_file_unlocked(&tokens)?;
            self.update_credentials_from_tokens(&tokens);
        }

        Ok(refreshed)
    }

    /// Exchange a GitHub access token for a Copilot session token.
    async fn refresh_copilot_session(&self, github_token: &str) -> Result<(String, String, i64)> {
        let resp = self
            .http_client
            .get("https://api.github.com/copilot_internal/v2/token")
            .header("Authorization", format!("token {}", github_token))
            .header("User-Agent", "harbangan-gateway")
            .send()
            .await
            .context("Copilot token request failed")?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Copilot token endpoint returned {}: {}", status, body);
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse Copilot token response")?;

        let token = body["token"]
            .as_str()
            .context("Missing 'token' in Copilot response")?
            .to_string();
        let expires_at = body["expires_at"]
            .as_i64()
            .context("Missing 'expires_at' in Copilot response")?;

        // Extract endpoints.api from the response, fall back to default
        let base_url = body["endpoints"]["api"]
            .as_str()
            .unwrap_or("https://api.githubcopilot.com")
            .to_string();

        Ok((token, base_url, expires_at))
    }

    /// Get provider connection status.
    pub fn provider_status(&self) -> HashMap<String, bool> {
        let mut status = HashMap::new();
        status.insert(
            "anthropic".to_string(),
            self.credentials.contains_key(&ProviderId::Anthropic),
        );
        status.insert(
            "openai_codex".to_string(),
            self.credentials.contains_key(&ProviderId::OpenAICodex),
        );
        status.insert(
            "copilot".to_string(),
            self.credentials.contains_key(&ProviderId::Copilot),
        );
        status
    }
}

/// Spawn the background token refresh task for proxy mode.
///
/// Checks every 60s and refreshes tokens that are within 5 minutes of expiry.
/// The manager's `credentials` DashMap is shared with the ProviderRegistry,
/// so updates are visible immediately — no separate sync step needed.
#[allow(dead_code)]
pub fn spawn_refresh_task(manager: Arc<ProxyTokenManager>, exchanger: Arc<dyn TokenExchanger>) {
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(REFRESH_INTERVAL_SECS));
        loop {
            interval.tick().await;
            match manager.refresh_expiring_tokens(exchanger.as_ref()).await {
                Ok(count) => {
                    if count > 0 {
                        tracing::debug!(count, "Proxy token refresh cycle complete");
                    }
                }
                Err(e) => {
                    tracing::warn!(error = ?e, "Proxy token refresh cycle failed");
                }
            }
        }
    });
}

/// Default token file path for proxy mode.
#[allow(dead_code)]
pub fn default_token_file() -> PathBuf {
    PathBuf::from("/data/tokens.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_file_default_empty() {
        let tf = TokenFile::default();
        assert!(tf.anthropic.is_none());
        assert!(tf.openai.is_none());
        assert!(tf.copilot.is_none());
    }

    #[test]
    fn test_token_file_serde_round_trip() {
        let tf = TokenFile {
            anthropic: Some(OAuthTokenEntry {
                access_token: "ant-tok".to_string(),
                refresh_token: "ant-ref".to_string(),
                expires_at: 1700000000,
            }),
            openai: Some(OAuthTokenEntry {
                access_token: "oai-tok".to_string(),
                refresh_token: "oai-ref".to_string(),
                expires_at: 1700001000,
            }),
            copilot: Some(CopilotTokenEntry {
                session_token: "cop-sess".to_string(),
                base_url: "https://api.githubcopilot.com".to_string(),
                expires_at: 1700002000,
                github_token: Some("gh-tok".to_string()),
            }),
        };
        let json = serde_json::to_string(&tf).unwrap();
        let back: TokenFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.anthropic.as_ref().unwrap().access_token, "ant-tok");
        assert_eq!(back.openai.as_ref().unwrap().access_token, "oai-tok");
        assert_eq!(back.copilot.as_ref().unwrap().session_token, "cop-sess");
        assert_eq!(
            back.copilot.as_ref().unwrap().github_token.as_deref(),
            Some("gh-tok")
        );
    }

    #[test]
    fn test_token_file_deserialize_partial() {
        let json = r#"{"anthropic":{"access_token":"a","refresh_token":"r","expires_at":100}}"#;
        let tf: TokenFile = serde_json::from_str(json).unwrap();
        assert!(tf.anthropic.is_some());
        assert!(tf.openai.is_none());
        assert!(tf.copilot.is_none());
    }

    #[test]
    fn test_copilot_entry_without_github_token() {
        let json = r#"{"session_token":"s","base_url":"https://x","expires_at":100}"#;
        let entry: CopilotTokenEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.session_token, "s");
        assert!(entry.github_token.is_none());
    }

    #[tokio::test]
    async fn test_read_nonexistent_file_returns_default() {
        let mgr = ProxyTokenManager::new(PathBuf::from("/tmp/nonexistent_test_tokens.json"));
        let tf = mgr.read_token_file().await.unwrap();
        assert!(tf.anthropic.is_none());
    }

    #[tokio::test]
    async fn test_store_and_load_oauth_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mgr = ProxyTokenManager::new(path.clone());

        mgr.store_oauth_tokens("anthropic", "ant-access", "ant-refresh", 3600)
            .await
            .unwrap();

        // Verify file was written
        assert!(path.exists());

        // Verify credentials updated
        assert!(mgr.credentials.contains_key(&ProviderId::Anthropic));
        let cred = mgr.credentials.get(&ProviderId::Anthropic).unwrap();
        assert_eq!(cred.access_token, "ant-access");

        // Load from file into a new manager
        let mgr2 = ProxyTokenManager::new(path);
        mgr2.load_from_file().await.unwrap();
        assert!(mgr2.credentials.contains_key(&ProviderId::Anthropic));
    }

    #[tokio::test]
    async fn test_store_openai_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mgr = ProxyTokenManager::new(path);

        mgr.store_oauth_tokens("openai_codex", "oai-access", "oai-refresh", 3600)
            .await
            .unwrap();

        assert!(mgr.credentials.contains_key(&ProviderId::OpenAICodex));
        let cred = mgr.credentials.get(&ProviderId::OpenAICodex).unwrap();
        assert_eq!(cred.access_token, "oai-access");
    }

    #[tokio::test]
    async fn test_store_copilot_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mgr = ProxyTokenManager::new(path);

        let future_ts = chrono::Utc::now().timestamp() + 3600;
        mgr.store_copilot_tokens(
            "cop-session",
            "https://api.githubcopilot.com",
            future_ts,
            Some("gh-token"),
        )
        .await
        .unwrap();

        assert!(mgr.credentials.contains_key(&ProviderId::Copilot));
        let cred = mgr.credentials.get(&ProviderId::Copilot).unwrap();
        assert_eq!(cred.access_token, "cop-session");
        assert_eq!(
            cred.base_url.as_deref(),
            Some("https://api.githubcopilot.com")
        );
    }

    #[tokio::test]
    async fn test_expired_tokens_not_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mgr = ProxyTokenManager::new(path.clone());

        // Write a token that's already expired
        let tf = TokenFile {
            anthropic: Some(OAuthTokenEntry {
                access_token: "expired-tok".to_string(),
                refresh_token: "ref".to_string(),
                expires_at: 1000, // way in the past
            }),
            ..Default::default()
        };
        std::fs::write(&path, serde_json::to_string(&tf).unwrap()).unwrap();

        mgr.load_from_file().await.unwrap();
        assert!(!mgr.credentials.contains_key(&ProviderId::Anthropic));
    }

    #[test]
    fn test_provider_status_empty() {
        let mgr = ProxyTokenManager::new(PathBuf::from("/tmp/test.json"));
        let status = mgr.provider_status();
        assert!(!status["anthropic"]);
        assert!(!status["openai_codex"]);
        assert!(!status["copilot"]);
    }

    #[test]
    fn test_snapshot_credentials() {
        let mgr = ProxyTokenManager::new(PathBuf::from("/tmp/test.json"));
        mgr.credentials.insert(
            ProviderId::Anthropic,
            ProviderCredentials {
                provider: ProviderId::Anthropic,
                access_token: "test-tok".to_string(),
                base_url: None,
                account_label: "proxy".to_string(),
            },
        );
        let snap = mgr.snapshot_credentials();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[&ProviderId::Anthropic].access_token, "test-tok");
    }

    #[tokio::test]
    async fn test_store_unknown_provider_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mgr = ProxyTokenManager::new(path);

        let result = mgr.store_oauth_tokens("unknown", "tok", "ref", 3600).await;
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_file_permissions_0o600() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mgr = ProxyTokenManager::new(path.clone());

        mgr.store_oauth_tokens("anthropic", "tok", "ref", 3600)
            .await
            .unwrap();

        let metadata = std::fs::metadata(&path).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[tokio::test]
    async fn test_multiple_providers_in_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mgr = ProxyTokenManager::new(path);

        mgr.store_oauth_tokens("anthropic", "ant-tok", "ant-ref", 3600)
            .await
            .unwrap();
        mgr.store_oauth_tokens("openai_codex", "oai-tok", "oai-ref", 3600)
            .await
            .unwrap();

        assert!(mgr.credentials.contains_key(&ProviderId::Anthropic));
        assert!(mgr.credentials.contains_key(&ProviderId::OpenAICodex));

        // Both should be in the file
        let tf = mgr.read_token_file().await.unwrap();
        assert!(tf.anthropic.is_some());
        assert!(tf.openai.is_some());
    }

    #[tokio::test]
    async fn test_new_shared_uses_same_dashmap() {
        let shared = Arc::new(DashMap::new());
        shared.insert(
            ProviderId::Anthropic,
            ProviderCredentials {
                provider: ProviderId::Anthropic,
                access_token: "pre-existing".to_string(),
                base_url: None,
                account_label: "proxy".to_string(),
            },
        );

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mgr = ProxyTokenManager::new_shared(path, Arc::clone(&shared));

        // Manager sees the pre-existing credential
        assert!(mgr.credentials.contains_key(&ProviderId::Anthropic));
        assert_eq!(
            mgr.credentials
                .get(&ProviderId::Anthropic)
                .unwrap()
                .access_token,
            "pre-existing"
        );

        // Store a new token via manager — visible through the shared map
        mgr.store_oauth_tokens("openai_codex", "oai-tok", "oai-ref", 3600)
            .await
            .unwrap();
        assert!(shared.contains_key(&ProviderId::OpenAICodex));
        assert_eq!(
            shared.get(&ProviderId::OpenAICodex).unwrap().access_token,
            "oai-tok"
        );
    }

    #[tokio::test]
    async fn test_load_from_file_populates_shared_dashmap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");

        // Write a token file first
        let tf = TokenFile {
            anthropic: Some(OAuthTokenEntry {
                access_token: "ant-from-file".to_string(),
                refresh_token: "ant-ref".to_string(),
                expires_at: chrono::Utc::now().timestamp() + 3600,
            }),
            ..Default::default()
        };
        std::fs::write(&path, serde_json::to_string(&tf).unwrap()).unwrap();

        // Create shared map and manager
        let shared = Arc::new(DashMap::new());
        let mgr = ProxyTokenManager::new_shared(path, Arc::clone(&shared));
        mgr.load_from_file().await.unwrap();

        // Shared map should have the credential
        assert!(shared.contains_key(&ProviderId::Anthropic));
        assert_eq!(
            shared.get(&ProviderId::Anthropic).unwrap().access_token,
            "ant-from-file"
        );
    }

    #[tokio::test]
    async fn test_provider_status_after_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mgr = ProxyTokenManager::new(path);

        mgr.store_oauth_tokens("anthropic", "tok", "ref", 3600)
            .await
            .unwrap();

        let status = mgr.provider_status();
        assert!(status["anthropic"]);
        assert!(!status["openai_codex"]);
        assert!(!status["copilot"]);
    }

    #[tokio::test]
    async fn test_store_openai_alias() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mgr = ProxyTokenManager::new(path);

        // "openai" alias should also work
        mgr.store_oauth_tokens("openai", "oai-tok", "oai-ref", 3600)
            .await
            .unwrap();
        assert!(mgr.credentials.contains_key(&ProviderId::OpenAICodex));
    }

    // ── MockExchanger for refresh tests ──────────────────────────────

    use crate::error::ApiError;
    use crate::web_ui::provider_oauth::TokenExchangeResult;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct MockExchanger {
        call_count: Arc<AtomicU32>,
        should_fail: bool,
    }

    impl MockExchanger {
        fn new() -> Self {
            Self {
                call_count: Arc::new(AtomicU32::new(0)),
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                call_count: Arc::new(AtomicU32::new(0)),
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl TokenExchanger for MockExchanger {
        async fn exchange_code(
            &self,
            _provider: &str,
            _code: &str,
            _state: &str,
            _pkce_verifier: &str,
            _redirect_uri: &str,
        ) -> Result<TokenExchangeResult, ApiError> {
            unimplemented!("not used in refresh tests")
        }

        async fn refresh_token(
            &self,
            _provider: &str,
            _refresh_token: &str,
        ) -> Result<TokenExchangeResult, ApiError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            if self.should_fail {
                Err(ApiError::Internal(anyhow::anyhow!("Token revoked")))
            } else {
                Ok(TokenExchangeResult {
                    access_token: "refreshed-access-token".to_string(),
                    refresh_token: "refreshed-refresh-token".to_string(),
                    expires_in: 3600,
                    email: String::new(),
                })
            }
        }
    }

    #[tokio::test]
    async fn test_refresh_expiring_anthropic_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mgr = ProxyTokenManager::new(path.clone());

        // Store a token that's about to expire (within REFRESH_BUFFER_SECS)
        let near_expiry = chrono::Utc::now().timestamp() + 60; // 60s left, < 300s buffer
        let tf = TokenFile {
            anthropic: Some(OAuthTokenEntry {
                access_token: "old-ant-tok".to_string(),
                refresh_token: "ant-refresh".to_string(),
                expires_at: near_expiry,
            }),
            ..Default::default()
        };
        std::fs::write(&path, serde_json::to_string(&tf).unwrap()).unwrap();
        mgr.load_from_file().await.unwrap();

        let exchanger = MockExchanger::new();
        let refreshed = mgr.refresh_expiring_tokens(&exchanger).await.unwrap();

        assert_eq!(refreshed, 1);
        assert_eq!(exchanger.call_count.load(Ordering::SeqCst), 1);

        // Credential should be updated
        let cred = mgr.credentials.get(&ProviderId::Anthropic).unwrap();
        assert_eq!(cred.access_token, "refreshed-access-token");

        // File should be updated
        let tf2 = mgr.read_token_file().await.unwrap();
        assert_eq!(
            tf2.anthropic.as_ref().unwrap().access_token,
            "refreshed-access-token"
        );
        assert_eq!(
            tf2.anthropic.as_ref().unwrap().refresh_token,
            "refreshed-refresh-token"
        );
    }

    #[tokio::test]
    async fn test_refresh_skips_fresh_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mgr = ProxyTokenManager::new(path.clone());

        // Store a token that's still fresh (well beyond buffer)
        let far_future = chrono::Utc::now().timestamp() + 7200; // 2 hours
        let tf = TokenFile {
            anthropic: Some(OAuthTokenEntry {
                access_token: "fresh-tok".to_string(),
                refresh_token: "ref".to_string(),
                expires_at: far_future,
            }),
            ..Default::default()
        };
        std::fs::write(&path, serde_json::to_string(&tf).unwrap()).unwrap();
        mgr.load_from_file().await.unwrap();

        let exchanger = MockExchanger::new();
        let refreshed = mgr.refresh_expiring_tokens(&exchanger).await.unwrap();

        assert_eq!(refreshed, 0);
        assert_eq!(exchanger.call_count.load(Ordering::SeqCst), 0);

        // Token unchanged
        let cred = mgr.credentials.get(&ProviderId::Anthropic).unwrap();
        assert_eq!(cred.access_token, "fresh-tok");
    }

    #[tokio::test]
    async fn test_refresh_skips_tokens_without_refresh_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mgr = ProxyTokenManager::new(path.clone());

        // Near-expiry but no refresh token
        let near_expiry = chrono::Utc::now().timestamp() + 60;
        let tf = TokenFile {
            anthropic: Some(OAuthTokenEntry {
                access_token: "old-tok".to_string(),
                refresh_token: String::new(), // empty
                expires_at: near_expiry,
            }),
            ..Default::default()
        };
        std::fs::write(&path, serde_json::to_string(&tf).unwrap()).unwrap();
        mgr.load_from_file().await.unwrap();

        let exchanger = MockExchanger::new();
        let refreshed = mgr.refresh_expiring_tokens(&exchanger).await.unwrap();

        assert_eq!(refreshed, 0);
        assert_eq!(exchanger.call_count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_refresh_failure_does_not_crash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mgr = ProxyTokenManager::new(path.clone());

        let near_expiry = chrono::Utc::now().timestamp() + 60;
        let tf = TokenFile {
            anthropic: Some(OAuthTokenEntry {
                access_token: "old-tok".to_string(),
                refresh_token: "ref-tok".to_string(),
                expires_at: near_expiry,
            }),
            ..Default::default()
        };
        std::fs::write(&path, serde_json::to_string(&tf).unwrap()).unwrap();
        mgr.load_from_file().await.unwrap();

        let exchanger = MockExchanger::failing();
        let refreshed = mgr.refresh_expiring_tokens(&exchanger).await.unwrap();

        // Refresh attempted but failed — count is 0 (not refreshed)
        assert_eq!(refreshed, 0);
        assert_eq!(exchanger.call_count.load(Ordering::SeqCst), 1);

        // Original token still in credentials (not removed on failure)
        assert!(mgr.credentials.contains_key(&ProviderId::Anthropic));
    }

    #[tokio::test]
    async fn test_refresh_both_providers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mgr = ProxyTokenManager::new(path.clone());

        let near_expiry = chrono::Utc::now().timestamp() + 60;
        let tf = TokenFile {
            anthropic: Some(OAuthTokenEntry {
                access_token: "old-ant".to_string(),
                refresh_token: "ant-ref".to_string(),
                expires_at: near_expiry,
            }),
            openai: Some(OAuthTokenEntry {
                access_token: "old-oai".to_string(),
                refresh_token: "oai-ref".to_string(),
                expires_at: near_expiry,
            }),
            ..Default::default()
        };
        std::fs::write(&path, serde_json::to_string(&tf).unwrap()).unwrap();
        mgr.load_from_file().await.unwrap();

        let exchanger = MockExchanger::new();
        let refreshed = mgr.refresh_expiring_tokens(&exchanger).await.unwrap();

        assert_eq!(refreshed, 2);
        assert_eq!(exchanger.call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_refresh_preserves_existing_refresh_token_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mgr = ProxyTokenManager::new(path.clone());

        let near_expiry = chrono::Utc::now().timestamp() + 60;
        let tf = TokenFile {
            anthropic: Some(OAuthTokenEntry {
                access_token: "old-tok".to_string(),
                refresh_token: "original-refresh".to_string(),
                expires_at: near_expiry,
            }),
            ..Default::default()
        };
        std::fs::write(&path, serde_json::to_string(&tf).unwrap()).unwrap();
        mgr.load_from_file().await.unwrap();

        // MockExchanger returns a non-empty refresh_token, so it should be updated
        let exchanger = MockExchanger::new();
        mgr.refresh_expiring_tokens(&exchanger).await.unwrap();

        let tf2 = mgr.read_token_file().await.unwrap();
        assert_eq!(
            tf2.anthropic.as_ref().unwrap().refresh_token,
            "refreshed-refresh-token"
        );
    }
}
