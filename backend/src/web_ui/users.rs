use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// A user in the system (populated from Google profile on first login).
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub picture_url: Option<String>,
    pub role: String,
    pub created_at: DateTime<Utc>,
    pub last_login: Option<DateTime<Utc>>,
}

/// Utility functions for user operations.
/// The service wrapper methods are not used — handlers call config_db directly.
/// This module retains domain extraction and the User struct.
#[allow(dead_code)]
pub struct UserService;

#[allow(dead_code)]
impl UserService {
    /// Extract the domain part of an email, lowercased.
    pub fn extract_domain(email: &str) -> String {
        email
            .rsplit_once('@')
            .map(|(_, d)| d.to_lowercase())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_domain() {
        assert_eq!(
            UserService::extract_domain("user@example.com"),
            "example.com"
        );
        assert_eq!(
            UserService::extract_domain("USER@EXAMPLE.COM"),
            "example.com"
        );
        assert_eq!(
            UserService::extract_domain("user@sub.example.com"),
            "sub.example.com"
        );
        assert_eq!(UserService::extract_domain("invalid-email"), "");
        assert_eq!(UserService::extract_domain(""), "");
    }

    #[test]
    fn test_extract_domain_with_plus() {
        assert_eq!(
            UserService::extract_domain("user+tag@example.com"),
            "example.com"
        );
    }
}
