//! AES-256-GCM encryption for sensitive config values (provider API keys, tokens).
//!
//! Encrypted format: base64(nonce[12] + ciphertext + tag[16])
//! Key source: `CONFIG_ENCRYPTION_KEY` env var (base64-encoded 32-byte key)

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};
use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

/// Load the 32-byte encryption key from `CONFIG_ENCRYPTION_KEY` env var.
pub fn load_encryption_key() -> Result<Key<Aes256Gcm>> {
    let encoded = std::env::var("CONFIG_ENCRYPTION_KEY")
        .context("CONFIG_ENCRYPTION_KEY env var is required for encrypted config values")?;
    let bytes = BASE64
        .decode(encoded.trim())
        .context("CONFIG_ENCRYPTION_KEY must be valid base64")?;
    if bytes.len() != 32 {
        anyhow::bail!(
            "CONFIG_ENCRYPTION_KEY must decode to exactly 32 bytes, got {}",
            bytes.len()
        );
    }
    Ok(*Key::<Aes256Gcm>::from_slice(&bytes))
}

/// Encrypt a plaintext value. Returns base64(nonce + ciphertext + tag).
pub fn encrypt_value(plaintext: &str, key: &Key<Aes256Gcm>) -> Result<String> {
    let cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce);
    combined.extend_from_slice(&ciphertext);

    Ok(BASE64.encode(&combined))
}

/// Decrypt a base64(nonce + ciphertext + tag) value back to plaintext.
pub fn decrypt_value(encrypted_base64: &str, key: &Key<Aes256Gcm>) -> Result<String> {
    let combined = BASE64
        .decode(encrypted_base64.trim())
        .context("Encrypted value is not valid base64")?;
    if combined.len() < 12 + 16 {
        anyhow::bail!(
            "Encrypted value too short (need at least 28 bytes, got {})",
            combined.len()
        );
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher = Aes256Gcm::new(key);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| anyhow::anyhow!("Decryption failed — wrong key or corrupted data"))?;

    String::from_utf8(plaintext).context("Decrypted value is not valid UTF-8")
}

/// Mask a sensitive value for display: "xxxx...last4"
#[allow(dead_code)]
pub fn mask_value(value: &str) -> String {
    if value.len() <= 4 {
        "****".to_string()
    } else {
        format!("xxxx...{}", &value[value.len() - 4..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> Key<Aes256Gcm> {
        // Fixed 32-byte key for deterministic tests
        *Key::<Aes256Gcm>::from_slice(&[0x42u8; 32])
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = test_key();
        let plaintext = "sk-ant-api03-secret-key-value";
        let encrypted = encrypt_value(plaintext, &key).unwrap();
        let decrypted = decrypt_value(&encrypted, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_produces_different_ciphertexts() {
        let key = test_key();
        let plaintext = "same-value";
        let enc1 = encrypt_value(plaintext, &key).unwrap();
        let enc2 = encrypt_value(plaintext, &key).unwrap();
        // Different nonces → different ciphertexts
        assert_ne!(enc1, enc2);
        // Both decrypt to the same value
        assert_eq!(decrypt_value(&enc1, &key).unwrap(), plaintext);
        assert_eq!(decrypt_value(&enc2, &key).unwrap(), plaintext);
    }

    #[test]
    fn test_decrypt_wrong_key_fails() {
        let key1 = test_key();
        let key2 = *Key::<Aes256Gcm>::from_slice(&[0x99u8; 32]);
        let encrypted = encrypt_value("secret", &key1).unwrap();
        let result = decrypt_value(&encrypted, &key2);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Decryption failed"));
    }

    #[test]
    fn test_decrypt_corrupted_data_fails() {
        let key = test_key();
        let encrypted = encrypt_value("secret", &key).unwrap();
        let mut bytes = BASE64.decode(&encrypted).unwrap();
        // Flip a byte in the ciphertext
        if let Some(b) = bytes.last_mut() {
            *b ^= 0xFF;
        }
        let corrupted = BASE64.encode(&bytes);
        assert!(decrypt_value(&corrupted, &key).is_err());
    }

    #[test]
    fn test_decrypt_too_short_fails() {
        let key = test_key();
        let short = BASE64.encode([0u8; 10]);
        let result = decrypt_value(&short, &key);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too short"));
    }

    #[test]
    fn test_decrypt_invalid_base64_fails() {
        let key = test_key();
        let result = decrypt_value("not-valid-base64!!!", &key);
        assert!(result.is_err());
    }

    #[test]
    fn test_encrypt_empty_string() {
        let key = test_key();
        let encrypted = encrypt_value("", &key).unwrap();
        let decrypted = decrypt_value(&encrypted, &key).unwrap();
        assert_eq!(decrypted, "");
    }

    #[test]
    fn test_encrypt_unicode() {
        let key = test_key();
        let plaintext = "日本語テスト 🔑";
        let encrypted = encrypt_value(plaintext, &key).unwrap();
        let decrypted = decrypt_value(&encrypted, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_mask_value_long() {
        assert_eq!(mask_value("sk-ant-api03-abcdef1234"), "xxxx...1234");
    }

    #[test]
    fn test_mask_value_short() {
        assert_eq!(mask_value("abc"), "****");
        assert_eq!(mask_value("abcd"), "****");
    }

    #[test]
    fn test_mask_value_exactly_five() {
        assert_eq!(mask_value("abcde"), "xxxx...bcde");
    }

    #[test]
    fn test_mask_value_empty() {
        assert_eq!(mask_value(""), "****");
    }

    #[test]
    fn test_load_encryption_key_wrong_length() {
        std::env::set_var("CONFIG_ENCRYPTION_KEY", BASE64.encode([0u8; 16]));
        let result = load_encryption_key();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("32 bytes"));
        std::env::remove_var("CONFIG_ENCRYPTION_KEY");
    }

    #[test]
    fn test_load_encryption_key_valid() {
        let key_bytes = [0xABu8; 32];
        std::env::set_var("CONFIG_ENCRYPTION_KEY", BASE64.encode(key_bytes));
        let key = load_encryption_key().unwrap();
        assert_eq!(key.as_slice(), &key_bytes);
        std::env::remove_var("CONFIG_ENCRYPTION_KEY");
    }
}
