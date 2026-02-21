//! Session management for Cloud Code API.
//!
//! Provides stable session IDs per account email for prompt caching continuity.
//! Session IDs are derived deterministically from the email address using SHA-256.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

// === Session Manager ===

/// Manages stable session IDs per account email.
///
/// Session IDs are derived from the email via SHA-256 so the same email
/// always maps to the same session, enabling prompt caching continuity
/// across requests. The cache is cleared on restart.
#[derive(Debug, Default)]
pub struct SessionManager {
    /// email → session_id
    sessions: HashMap<String, String>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the session ID for the given email, creating one if needed.
    ///
    /// The session ID is deterministic: the same email always produces
    /// the same ID (hex-encoded SHA-256 prefix).
    pub fn get_or_create(&mut self, email: &str) -> &str {
        self.sessions
            .entry(email.to_string())
            .or_insert_with(|| derive_session_id(email))
    }

    /// Clears all cached sessions.
    pub fn clear(&mut self) {
        self.sessions.clear();
    }

    /// Returns the number of active sessions.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Returns true if there are no active sessions.
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

/// Derives a stable session ID from an email address.
///
/// Uses SHA-256 of the email, truncated to 32 hex chars, to produce
/// a deterministic but opaque identifier.
pub fn derive_session_id(email: &str) -> String {
    let hash = Sha256::digest(email.as_bytes());
    // Take first 16 bytes (32 hex chars) for a compact but unique ID
    hex::encode(&hash[..16])
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_session_id_deterministic() {
        let id1 = derive_session_id("user@example.com");
        let id2 = derive_session_id("user@example.com");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_derive_session_id_different_emails() {
        let id1 = derive_session_id("alice@example.com");
        let id2 = derive_session_id("bob@example.com");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_derive_session_id_length() {
        let id = derive_session_id("test@test.com");
        assert_eq!(id.len(), 32); // 16 bytes = 32 hex chars
    }

    #[test]
    fn test_derive_session_id_hex_chars() {
        let id = derive_session_id("test@test.com");
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_session_manager_get_or_create() {
        let mut mgr = SessionManager::new();
        let id1 = mgr.get_or_create("user@example.com").to_string();
        let id2 = mgr.get_or_create("user@example.com").to_string();
        assert_eq!(id1, id2);
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn test_session_manager_multiple_emails() {
        let mut mgr = SessionManager::new();
        mgr.get_or_create("alice@example.com");
        mgr.get_or_create("bob@example.com");
        assert_eq!(mgr.len(), 2);
    }

    #[test]
    fn test_session_manager_clear() {
        let mut mgr = SessionManager::new();
        mgr.get_or_create("user@example.com");
        assert!(!mgr.is_empty());
        mgr.clear();
        assert!(mgr.is_empty());
        assert_eq!(mgr.len(), 0);
    }

    #[test]
    fn test_session_manager_stable_after_clear() {
        let mut mgr = SessionManager::new();
        let id_before = mgr.get_or_create("user@example.com").to_string();
        mgr.clear();
        let id_after = mgr.get_or_create("user@example.com").to_string();
        // Deterministic derivation means same ID even after clear
        assert_eq!(id_before, id_after);
    }
}
