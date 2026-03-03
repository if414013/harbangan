use std::sync::Arc;

use chrono::Utc;
use dashmap::DashMap;
use uuid::Uuid;

use crate::routes::SessionInfo;
use crate::web_ui::config_db::ConfigDb;

/// Session management utilities.
/// Handlers call config_db directly (pattern established by google_auth.rs).
/// This module retains the background cleanup task and cookie attribute helpers.
pub struct SessionService;

impl SessionService {
    /// Spawn a background task that cleans up expired sessions every hour.
    pub fn spawn_cleanup_task(db: Arc<ConfigDb>, cache: Arc<DashMap<Uuid, SessionInfo>>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
            loop {
                interval.tick().await;

                // Clean cache
                let now = Utc::now();
                cache.retain(|_, info| info.expires_at > now);

                // Clean DB
                match db.cleanup_expired_sessions().await {
                    Ok(count) => {
                        if count > 0 {
                            tracing::info!(count, "Cleaned up expired sessions");
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = ?e, "Failed to cleanup expired sessions");
                    }
                }
            }
        });
    }

    /// Name of the session cookie.
    #[cfg(test)]
    pub const COOKIE_NAME: &'static str = "kgw_session";

    /// Build cookie attributes for the session cookie.
    /// `secure` is set based on the callback URL scheme.
    #[cfg(test)]
    pub fn cookie_attributes(callback_url: &str) -> String {
        let secure = if callback_url.starts_with("http://localhost")
            || callback_url.starts_with("http://127.0.0.1")
        {
            ""
        } else {
            "; Secure"
        };

        format!(
            "HttpOnly; SameSite=Strict; Path=/_ui; Max-Age=86400{}",
            secure
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cookie_attributes_production() {
        let attrs = SessionService::cookie_attributes("https://gateway.example.com/callback");
        assert!(attrs.contains("HttpOnly"));
        assert!(attrs.contains("Secure"));
        assert!(attrs.contains("SameSite=Strict"));
        assert!(attrs.contains("Path=/_ui"));
        assert!(attrs.contains("Max-Age=86400"));
    }

    #[test]
    fn test_cookie_attributes_localhost() {
        let attrs = SessionService::cookie_attributes("http://localhost:8000/callback");
        assert!(attrs.contains("HttpOnly"));
        assert!(!attrs.contains("Secure"));
        assert!(attrs.contains("SameSite=Strict"));
    }

    #[test]
    fn test_cookie_attributes_loopback() {
        let attrs = SessionService::cookie_attributes("http://127.0.0.1:8000/callback");
        assert!(!attrs.contains("Secure"));
    }

    #[test]
    fn test_cookie_name() {
        assert_eq!(SessionService::COOKIE_NAME, "kgw_session");
    }
}
