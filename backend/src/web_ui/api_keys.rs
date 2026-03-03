use axum::extract::{Path, State};
use axum::routing::{delete, get};
use axum::{Extension, Json, Router};
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::error::ApiError;
use crate::routes::{AppState, SessionInfo};

/// Maximum number of API keys per user.
const MAX_KEYS_PER_USER: i64 = 10;

/// Prefix for generated API keys.
const KEY_PREFIX_DISPLAY: &str = "sk-";

/// Number of hex characters to use as the display prefix (after "sk-").
const KEY_PREFIX_LEN: usize = 8;

// ── Types ────────────────────────────────────────────────────────────

/// A single API key in list responses.
#[derive(Serialize)]
struct ApiKeyInfo {
    id: Uuid,
    key_prefix: String,
    label: String,
    last_used: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

/// Response for listing API keys.
#[derive(Serialize)]
struct ApiKeyListResponse {
    keys: Vec<ApiKeyInfo>,
    count: usize,
}

/// Request to create a new API key.
#[derive(Deserialize)]
struct CreateApiKeyRequest {
    #[serde(default)]
    label: String,
}

/// Response for creating a new API key (plaintext returned ONCE).
#[derive(Serialize)]
struct CreateApiKeyResponse {
    id: Uuid,
    key: String,
    key_prefix: String,
    label: String,
}

/// Response for deleting an API key.
#[derive(Serialize)]
struct DeleteApiKeyResponse {
    ok: bool,
}

// ── Key generation helpers ───────────────────────────────────────────

/// Generate a cryptographically random API key (256-bit entropy).
/// Format: "sk-" + 64 hex chars (32 bytes).
fn generate_api_key() -> String {
    let mut key_bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut key_bytes);
    format!("{}{}", KEY_PREFIX_DISPLAY, hex::encode(key_bytes))
}

/// Compute the SHA-256 hash of a key (hex-encoded).
fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

/// Constant-time comparison of two hex-encoded hashes.
fn verify_key_hash(computed_hash: &str, stored_hash: &str) -> bool {
    computed_hash
        .as_bytes()
        .ct_eq(stored_hash.as_bytes())
        .into()
}

/// Extract the display prefix from a generated key.
/// Returns "sk-XXXXXXXX" (the first 8 hex chars after the prefix).
fn extract_prefix(key: &str) -> String {
    let after_prefix = key.strip_prefix(KEY_PREFIX_DISPLAY).unwrap_or(key);
    let prefix_chars: String = after_prefix.chars().take(KEY_PREFIX_LEN).collect();
    format!("{}{}", KEY_PREFIX_DISPLAY, prefix_chars)
}

// ── Handlers ─────────────────────────────────────────────────────────

/// GET /_ui/api/keys — list own API keys (metadata only, no hashes)
async fn list_api_keys(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
) -> Result<Json<ApiKeyListResponse>, ApiError> {
    let user_id = session.user_id;
    let config_db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Database not configured")))?;

    let rows = config_db
        .list_api_keys(user_id)
        .await
        .map_err(ApiError::Internal)?;

    let keys: Vec<ApiKeyInfo> = rows
        .into_iter()
        .map(|(id, prefix, label, last_used, created_at)| ApiKeyInfo {
            id,
            key_prefix: prefix,
            label,
            last_used,
            created_at,
        })
        .collect();

    let count = keys.len();
    Ok(Json(ApiKeyListResponse { keys, count }))
}

/// POST /_ui/api/keys — generate a new API key (returns plaintext ONCE)
async fn create_api_key(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
    Json(body): Json<CreateApiKeyRequest>,
) -> Result<Json<CreateApiKeyResponse>, ApiError> {
    let user_id = session.user_id;
    let config_db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Database not configured")))?;

    // Enforce max 10 keys per user
    let current_count = config_db
        .count_api_keys(user_id)
        .await
        .map_err(ApiError::Internal)?;

    if current_count >= MAX_KEYS_PER_USER {
        return Err(ApiError::ValidationError(format!(
            "Maximum {} API keys per user reached. Delete an existing key first.",
            MAX_KEYS_PER_USER
        )));
    }

    // Generate key
    let plaintext_key = generate_api_key();
    let key_hash = hash_api_key(&plaintext_key);
    let key_prefix = extract_prefix(&plaintext_key);

    // Store in database
    let key_id = config_db
        .insert_api_key(user_id, &key_hash, &key_prefix, &body.label)
        .await
        .map_err(ApiError::Internal)?;

    tracing::info!(
        user_id = %user_id,
        key_prefix = %key_prefix,
        "api_key_created"
    );

    Ok(Json(CreateApiKeyResponse {
        id: key_id,
        key: plaintext_key,
        key_prefix,
        label: body.label,
    }))
}

/// DELETE /_ui/api/keys/:id — revoke an API key + evict from cache
async fn delete_api_key(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
    Path(key_id): Path<Uuid>,
) -> Result<Json<DeleteApiKeyResponse>, ApiError> {
    let user_id = session.user_id;
    let config_db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Database not configured")))?;

    // Delete from DB (returns the hash for cache eviction)
    let deleted_hash = config_db
        .delete_api_key(key_id, user_id)
        .await
        .map_err(ApiError::Internal)?;

    match deleted_hash {
        Some(hash) => {
            // Evict from api_key_cache immediately
            state.api_key_cache.remove(&hash);

            tracing::info!(
                user_id = %user_id,
                key_id = %key_id,
                "api_key_revoked"
            );

            Ok(Json(DeleteApiKeyResponse { ok: true }))
        }
        None => Err(ApiError::ValidationError(
            "API key not found or does not belong to you".to_string(),
        )),
    }
}

// ── Router ───────────────────────────────────────────────────────────

/// Build the API key management router.
pub fn api_key_routes() -> Router<AppState> {
    Router::new()
        .route("/keys", get(list_api_keys).post(create_api_key))
        .route("/keys/{id}", delete(delete_api_key))
}

// ── Public utilities for auth middleware ──────────────────────────────

/// Constant-time verify a hash (used by auth middleware).
pub fn constant_time_verify(computed: &str, stored: &str) -> bool {
    verify_key_hash(computed, stored)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_api_key_format() {
        let key = generate_api_key();
        assert!(
            key.starts_with("sk-"),
            "Key should start with 'sk-': {}",
            key
        );
        // sk- + 64 hex chars = 67 chars total
        assert_eq!(key.len(), 67, "Key should be 67 chars: {}", key);
    }

    #[test]
    fn test_generate_api_key_unique() {
        let key1 = generate_api_key();
        let key2 = generate_api_key();
        assert_ne!(key1, key2, "Keys should be unique");
    }

    #[test]
    fn test_hash_api_key_deterministic() {
        let key = "sk-test123";
        let hash1 = hash_api_key(key);
        let hash2 = hash_api_key(key);
        assert_eq!(hash1, hash2, "Same key should produce same hash");
    }

    #[test]
    fn test_hash_api_key_different_keys_different_hashes() {
        let hash1 = hash_api_key("sk-key1");
        let hash2 = hash_api_key("sk-key2");
        assert_ne!(hash1, hash2, "Different keys should have different hashes");
    }

    #[test]
    fn test_hash_api_key_hex_format() {
        let hash = hash_api_key("sk-test");
        // SHA-256 produces 32 bytes = 64 hex chars
        assert_eq!(hash.len(), 64, "SHA-256 hash should be 64 hex chars");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "Hash should be hex: {}",
            hash
        );
    }

    #[test]
    fn test_verify_key_hash_matching() {
        let key = generate_api_key();
        let hash = hash_api_key(&key);
        assert!(
            verify_key_hash(&hash, &hash),
            "Matching hashes should verify"
        );
    }

    #[test]
    fn test_verify_key_hash_not_matching() {
        let hash1 = hash_api_key("sk-key1");
        let hash2 = hash_api_key("sk-key2");
        assert!(
            !verify_key_hash(&hash1, &hash2),
            "Different hashes should not verify"
        );
    }

    #[test]
    fn test_constant_time_comparison() {
        // Verify that our constant-time comparison works correctly
        let a = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        let b = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        let c = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";

        assert!(verify_key_hash(a, b));
        assert!(!verify_key_hash(a, c));
    }

    #[test]
    fn test_extract_prefix() {
        let key = "sk-abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        let prefix = extract_prefix(&key);
        assert_eq!(prefix, "sk-abcdef12");
    }

    #[test]
    fn test_extract_prefix_generated_key() {
        let key = generate_api_key();
        let prefix = extract_prefix(&key);
        assert!(prefix.starts_with("sk-"));
        // sk- (3) + 8 hex chars = 11
        assert_eq!(prefix.len(), 11);
    }

    #[test]
    fn test_generate_and_hash_roundtrip() {
        let key = generate_api_key();
        let hash = hash_api_key(&key);

        // Verify we can reproduce the hash from the same key
        let hash2 = hash_api_key(&key);
        assert!(verify_key_hash(&hash, &hash2));
    }

    #[test]
    fn test_create_api_key_request_default_label() {
        let json = serde_json::json!({});
        let req: CreateApiKeyRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.label, "");
    }

    #[test]
    fn test_create_api_key_request_with_label() {
        let json = serde_json::json!({ "label": "my-key" });
        let req: CreateApiKeyRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.label, "my-key");
    }

    #[test]
    fn test_api_key_info_serialization() {
        let info = ApiKeyInfo {
            id: Uuid::new_v4(),
            key_prefix: "sk-abcdef12".to_string(),
            label: "test".to_string(),
            last_used: None,
            created_at: Utc::now(),
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["key_prefix"], "sk-abcdef12");
        assert_eq!(json["label"], "test");
        assert!(json["last_used"].is_null());
    }
}
