use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use reqwest::Client;

use super::types::{
    AwsSsoOidcResponse, Credentials, KiroRefreshRequest, KiroRefreshResponse, TokenData,
};

/// Get Kiro refresh URL for region
fn get_kiro_refresh_url(region: &str) -> String {
    format!("https://prod.{}.auth.desktop.kiro.dev/refreshToken", region)
}

/// Get AWS SSO OIDC URL for region
fn get_aws_sso_oidc_url(region: &str) -> String {
    format!("https://oidc.{}.amazonaws.com/token", region)
}

/// Get machine fingerprint for User-Agent
fn get_machine_fingerprint() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string());

    let mut hasher = DefaultHasher::new();
    hostname.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Refresh token using Kiro Desktop Auth
pub async fn refresh_kiro_desktop(client: &Client, creds: &Credentials) -> Result<TokenData> {
    tracing::info!("Refreshing Kiro token via Kiro Desktop Auth...");

    let url = get_kiro_refresh_url(&creds.region);
    let fingerprint = get_machine_fingerprint();

    let request = KiroRefreshRequest {
        refresh_token: creds.refresh_token.clone(),
    };

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("User-Agent", format!("KiroIDE-0.7.45-{}", fingerprint))
        .json(&request)
        .send()
        .await
        .context("Failed to send Kiro Desktop refresh request")?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        anyhow::bail!("Kiro Desktop refresh failed: {} - {}", status, error_text);
    }

    let data: KiroRefreshResponse = response
        .json()
        .await
        .context("Failed to parse Kiro Desktop refresh response")?;

    if data.access_token.is_empty() {
        anyhow::bail!("Kiro Desktop response does not contain accessToken");
    }

    // Calculate expiration time with buffer (minus 60 seconds)
    let expires_in = data.expires_in.unwrap_or(3600);
    let expires_at = Utc::now() + Duration::seconds(expires_in as i64 - 60);

    tracing::info!(
        "Token refreshed via Kiro Desktop Auth, expires: {}",
        expires_at.to_rfc3339()
    );

    Ok(TokenData {
        access_token: data.access_token,
        refresh_token: data.refresh_token,
        expires_at,
        profile_arn: data.profile_arn,
    })
}

/// Refresh token using AWS SSO OIDC
pub async fn refresh_aws_sso_oidc(client: &Client, creds: &Credentials) -> Result<TokenData> {
    tracing::info!("Refreshing Kiro token via AWS SSO OIDC...");

    let client_id = creds
        .client_id
        .as_ref()
        .context("Client ID is required for AWS SSO OIDC")?;
    let client_secret = creds
        .client_secret
        .as_ref()
        .context("Client secret is required for AWS SSO OIDC")?;

    // Use SSO region for OIDC endpoint (may differ from API region)
    let sso_region = creds.sso_region.as_deref().unwrap_or(&creds.region);
    let url = get_aws_sso_oidc_url(sso_region);

    tracing::debug!(
        "AWS SSO OIDC refresh request: url={}, sso_region={}, api_region={}, client_id={}...",
        url,
        sso_region,
        creds.region,
        &client_id[..8.min(client_id.len())]
    );

    // AWS SSO OIDC uses JSON with camelCase field names
    let body = serde_json::json!({
        "grantType": "refresh_token",
        "clientId": client_id,
        "clientSecret": client_secret,
        "refreshToken": &creds.refresh_token,
    });

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("Failed to send AWS SSO OIDC refresh request")?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        tracing::error!(
            "AWS SSO OIDC refresh failed: status={}, body={}",
            status,
            error_text
        );

        // Try to parse AWS error for more details
        if let Ok(error_json) = serde_json::from_str::<serde_json::Value>(&error_text) {
            if let (Some(error_code), Some(error_desc)) = (
                error_json.get("error").and_then(|v| v.as_str()),
                error_json.get("error_description").and_then(|v| v.as_str()),
            ) {
                tracing::error!(
                    "AWS SSO OIDC error details: error={}, description={}",
                    error_code,
                    error_desc
                );
            }
        }

        anyhow::bail!("AWS SSO OIDC refresh failed: {} - {}", status, error_text);
    }

    let data: AwsSsoOidcResponse = response
        .json()
        .await
        .context("Failed to parse AWS SSO OIDC refresh response")?;

    if data.access_token.is_empty() {
        anyhow::bail!("AWS SSO OIDC response does not contain accessToken");
    }

    // Calculate expiration time with buffer (minus 60 seconds)
    let expires_in = data.expires_in.unwrap_or(3600);
    let expires_at = Utc::now() + Duration::seconds(expires_in as i64 - 60);

    tracing::info!(
        "Token refreshed via AWS SSO OIDC, expires: {}",
        expires_at.to_rfc3339()
    );

    Ok(TokenData {
        access_token: data.access_token,
        refresh_token: data.refresh_token,
        expires_at,
        profile_arn: None,
    })
}
