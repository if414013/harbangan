use anyhow::{Context, Result};
use axum_server::tls_rustls::RustlsConfig;
use rcgen::{CertificateParams, KeyPair, SanType};
use std::net::IpAddr;
use std::path::{Path, PathBuf};

/// Default directory name for storing auto-generated certificates
const TLS_DIR_NAME: &str = "tls";
/// Default certificate file name
const CERT_FILENAME: &str = "cert.pem";
/// Default private key file name
const KEY_FILENAME: &str = "key.pem";
/// Validity period for self-signed certificates (in days)
const SELF_SIGNED_VALIDITY_DAYS: u32 = 365;
/// Regenerate self-signed certificates this many days before they expire,
/// so clients never hit a hard expiry wall.
const RENEWAL_BUFFER_DAYS: u32 = 30;

/// TLS configuration for the gateway server.
#[derive(Clone, Debug)]
pub struct TlsConfig {
    /// Path to the TLS certificate file (PEM format)
    pub cert_path: Option<PathBuf>,
    /// Path to the TLS private key file (PEM format)
    pub key_path: Option<PathBuf>,
}

impl TlsConfig {
    /// Build an `axum_server::tls_rustls::RustlsConfig` from this TLS configuration.
    ///
    /// If `cert_path` and `key_path` are provided, loads certificates from those files.
    /// Otherwise, auto-generates a self-signed certificate (reusing a previously generated
    /// one if it exists in the default storage directory and has not expired).
    pub async fn build_rustls_config(&self) -> Result<RustlsConfig> {
        match (&self.cert_path, &self.key_path) {
            (Some(cert), Some(key)) => load_from_files(cert, key).await,
            (Some(_), None) => {
                anyhow::bail!(
                    "TLS certificate path provided without a key path. \
                     Please also set --tls-key."
                );
            }
            (None, Some(_)) => {
                anyhow::bail!(
                    "TLS key path provided without a certificate path. \
                     Please also set --tls-cert."
                );
            }
            (None, None) => load_or_generate_self_signed().await,
        }
    }
}

/// Load TLS configuration from existing PEM files.
///
/// File-existence checks are intentionally omitted here because they are
/// already performed in `Config::validate()` at startup. Keeping a single
/// validation site avoids duplicate maintenance.
async fn load_from_files(cert_path: &Path, key_path: &Path) -> Result<RustlsConfig> {
    tracing::info!("Loading TLS certificate from: {}", cert_path.display());
    tracing::info!("Loading TLS private key from: {}", key_path.display());

    RustlsConfig::from_pem_file(cert_path, key_path)
        .await
        .context("Failed to load TLS certificate/key files. Ensure they are valid PEM format.")
}

/// Load a previously generated self-signed certificate, or generate a new one.
///
/// Certificates are regenerated when:
/// - No certificate files exist on disk yet.
/// - The existing certificate is expired or close to expiry (based on file mtime).
async fn load_or_generate_self_signed() -> Result<RustlsConfig> {
    let tls_dir = default_tls_dir()?;
    let cert_path = tls_dir.join(CERT_FILENAME);
    let key_path = tls_dir.join(KEY_FILENAME);

    // Reuse existing self-signed cert if both files are present and not expired
    if cert_path.exists() && key_path.exists() {
        if is_self_signed_cert_expired(&cert_path) {
            tracing::warn!(
                "Self-signed certificate in {} has expired or is close to expiry — regenerating.",
                tls_dir.display()
            );
        } else {
            tracing::info!(
                "Reusing existing self-signed certificate from: {}",
                tls_dir.display()
            );
            return RustlsConfig::from_pem_file(&cert_path, &key_path)
                .await
                .context(
                    "Failed to load previously generated self-signed certificate. \
                     Delete the files and restart to regenerate.",
                );
        }
    }

    // Generate a new self-signed certificate
    tracing::info!("No TLS certificate configured — generating a self-signed certificate...");

    let (cert_pem, key_pem) = generate_self_signed_cert()?;

    // Persist to disk so we can reuse on next startup
    create_tls_dir(&tls_dir)?;
    write_cert_file(&cert_path, &cert_pem)?;
    write_key_file(&key_path, &key_pem)?;

    tracing::info!(
        "✅ Self-signed certificate saved to: {}",
        tls_dir.display()
    );
    tracing::warn!(
        "⚠️  Self-signed certificates are not trusted by browsers/clients. \
         Use --tls-cert and --tls-key to provide your own certificate for production use."
    );

    RustlsConfig::from_pem(cert_pem.into(), key_pem.into())
        .await
        .context("Failed to build RustlsConfig from generated self-signed certificate")
}

/// Generate a self-signed certificate and return (cert_pem, key_pem).
fn generate_self_signed_cert() -> Result<(String, String)> {
    let mut params = CertificateParams::default();

    // Set SANs with correct types — IP addresses must be SanType::IpAddress,
    // not DNS names, for clients to match them correctly.
    params.subject_alt_names = vec![
        SanType::DnsName(
            "localhost"
                .to_string()
                .try_into()
                .context("Failed to create DNS SAN for localhost")?,
        ),
        SanType::IpAddress(
            "127.0.0.1"
                .parse::<IpAddr>()
                .context("Failed to parse 127.0.0.1")?,
        ),
        SanType::IpAddress(
            "::1".parse::<IpAddr>().context("Failed to parse ::1")?,
        ),
    ];

    // Set validity period
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now + time::Duration::days(SELF_SIGNED_VALIDITY_DAYS as i64);

    // Generate key pair and self-sign
    let key_pair = KeyPair::generate().context("Failed to generate TLS key pair")?;
    let cert = params
        .self_signed(&key_pair)
        .context("Failed to generate self-signed certificate")?;

    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();

    tracing::info!(
        "Generated self-signed certificate (valid for {} days)",
        SELF_SIGNED_VALIDITY_DAYS
    );

    Ok((cert_pem, key_pem))
}

// ---------------------------------------------------------------------------
// Filesystem helpers
// ---------------------------------------------------------------------------

/// Create the TLS directory with restricted permissions.
fn create_tls_dir(tls_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(tls_dir).with_context(|| {
        format!("Failed to create TLS directory: {}", tls_dir.display())
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o700);
        if let Err(e) = std::fs::set_permissions(tls_dir, perms) {
            tracing::warn!(
                "Failed to restrict TLS directory permissions to 0700: {}. \
                 Other users may be able to list its contents.",
                e
            );
        }
    }

    Ok(())
}

/// Write the certificate PEM file (not sensitive — default permissions are fine).
fn write_cert_file(path: &Path, pem: &str) -> Result<()> {
    std::fs::write(path, pem)
        .with_context(|| format!("Failed to write certificate to: {}", path.display()))
}

/// Write the private key PEM file with restricted permissions (0600 on Unix).
///
/// On Unix the file is created with mode 0600 from the start to avoid a
/// TOCTOU window where the key is briefly world-readable.
fn write_key_file(path: &Path, pem: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .with_context(|| {
                format!("Failed to create private key file: {}", path.display())
            })?;

        file.write_all(pem.as_bytes()).with_context(|| {
            format!("Failed to write private key to: {}", path.display())
        })?;

        Ok(())
    }

    #[cfg(not(unix))]
    {
        std::fs::write(path, pem).with_context(|| {
            format!("Failed to write private key to: {}", path.display())
        })?;

        tracing::warn!(
            "Non-Unix platform: could not restrict private key file permissions. \
             Please ensure {} is not readable by other users.",
            path.display()
        );

        Ok(())
    }
}

/// Check whether a self-signed certificate is expired or close to expiry.
///
/// Uses the file's modification time as a proxy for the certificate's
/// `not_before` date — this is safe because we are the ones who generated
/// and wrote the file. Regeneration is triggered `RENEWAL_BUFFER_DAYS`
/// before actual expiry so clients never hit a hard failure.
fn is_self_signed_cert_expired(cert_path: &Path) -> bool {
    let metadata = match std::fs::metadata(cert_path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let modified = match metadata.modified() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let age = modified.elapsed().unwrap_or_default();
    let max_age_secs =
        (SELF_SIGNED_VALIDITY_DAYS - RENEWAL_BUFFER_DAYS) as u64 * 24 * 60 * 60;
    age > std::time::Duration::from_secs(max_age_secs)
}

/// Return the default directory for storing auto-generated TLS certificates.
///
/// Resolves to `~/.kiro-gateway/tls/`.
fn default_tls_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context(
        "Could not determine home directory. \
         Please provide --tls-cert and --tls-key explicitly.",
    )?;
    Ok(home.join(".kiro-gateway").join(TLS_DIR_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_self_signed_cert() {
        let (cert_pem, key_pem) = generate_self_signed_cert().unwrap();

        assert!(cert_pem.starts_with("-----BEGIN CERTIFICATE-----"));
        assert!(cert_pem.contains("-----END CERTIFICATE-----"));
        assert!(key_pem.starts_with("-----BEGIN PRIVATE KEY-----"));
        assert!(key_pem.contains("-----END PRIVATE KEY-----"));
    }

    #[test]
    fn test_default_tls_dir() {
        let dir = default_tls_dir().unwrap();
        assert!(dir.ends_with(".kiro-gateway/tls"));
    }

    #[test]
    fn test_tls_config_requires_both_cert_and_key() {
        // cert without key
        let config = TlsConfig {
            cert_path: Some(PathBuf::from("/tmp/cert.pem")),
            key_path: None,
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(config.build_rustls_config());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("without a key path")
        );

        // key without cert
        let config = TlsConfig {
            cert_path: None,
            key_path: Some(PathBuf::from("/tmp/key.pem")),
        };
        let result = rt.block_on(config.build_rustls_config());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("without a certificate path")
        );
    }

    #[test]
    fn test_is_self_signed_cert_expired_nonexistent_file() {
        // Non-existent file should not be considered expired
        assert!(!is_self_signed_cert_expired(Path::new(
            "/tmp/nonexistent_cert_12345.pem"
        )));
    }

    #[test]
    fn test_is_self_signed_cert_expired_fresh_file() {
        // A freshly created file should not be expired
        let dir = std::env::temp_dir().join("kiro_tls_test_fresh");
        std::fs::create_dir_all(&dir).unwrap();
        let cert_path = dir.join("test_cert.pem");
        std::fs::write(&cert_path, "test").unwrap();

        assert!(!is_self_signed_cert_expired(&cert_path));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }
}
