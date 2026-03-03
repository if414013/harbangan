#![allow(dead_code)]
use anyhow::{Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::Rng;
use reqwest::Client;
use sha2::{Digest, Sha256};

use super::types::*;

const SCOPES: &[&str] = &[
    "codewhisperer:completions",
    "codewhisperer:analysis",
    "codewhisperer:conversations",
];

fn get_oidc_base_url(region: &str) -> String {
    format!("https://oidc.{}.amazonaws.com", region)
}

/// Register an OAuth client with AWS SSO OIDC.
///
/// For the browser (authorization_code) flow, `redirect_uri` must be provided.
/// For the device flow, pass `redirect_uri` as `None`.
/// `start_url` is the Identity Center start URL (optional, omit for Builder ID).
pub async fn register_client(
    client: &Client,
    region: &str,
    flow: &str,
    redirect_uri: Option<&str>,
    start_url: Option<&str>,
) -> Result<ClientRegistrationResponse> {
    let url = format!("{}/client/register", get_oidc_base_url(region));

    let grant_types = if flow == "browser" {
        vec!["authorization_code", "refresh_token"]
    } else {
        vec![
            "urn:ietf:params:oauth:grant-type:device_code",
            "refresh_token",
        ]
    };

    let mut body = serde_json::json!({
        "clientName": "kiro-gateway",
        "clientType": "public",
        "scopes": SCOPES,
        "grantTypes": grant_types,
    });

    if let Some(uri) = redirect_uri {
        body["redirectUris"] = serde_json::json!([uri]);
    }

    if let Some(su) = start_url {
        if !su.is_empty() {
            body["issuerUrl"] = serde_json::json!(su);
        }
    }

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to register OAuth client")?;

    if !response.status().is_success() {
        let status = response.status();
        let error = response.text().await.unwrap_or_default();
        anyhow::bail!("Client registration failed: {} - {}", status, error);
    }

    response
        .json()
        .await
        .context("Failed to parse client registration response")
}

/// Generate PKCE code verifier and challenge
pub fn generate_pkce() -> PkceState {
    let mut rng = rand::thread_rng();
    let verifier_bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    let code_verifier = URL_SAFE_NO_PAD.encode(&verifier_bytes);

    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    let code_challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

    let state_bytes: Vec<u8> = (0..16).map(|_| rng.gen()).collect();
    let state = URL_SAFE_NO_PAD.encode(&state_bytes);

    PkceState {
        code_verifier,
        code_challenge,
        state,
    }
}

/// Build the authorization URL for browser redirect flow
pub fn build_authorize_url(
    region: &str,
    client_id: &str,
    redirect_uri: &str,
    pkce: &PkceState,
) -> String {
    let base = get_oidc_base_url(region);
    let scopes_str = SCOPES.join(" ");
    format!(
        "{}/authorize?response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state={}&scope={}",
        base,
        urlencoding::encode(client_id),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(&pkce.code_challenge),
        urlencoding::encode(&pkce.state),
        urlencoding::encode(&scopes_str),
    )
}

/// Exchange authorization code for tokens (browser flow completion)
pub async fn exchange_authorization_code(
    client: &Client,
    region: &str,
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<TokenExchangeResponse> {
    let url = format!("{}/token", get_oidc_base_url(region));
    let body = serde_json::json!({
        "grantType": "authorization_code",
        "clientId": client_id,
        "code": code,
        "redirectUri": redirect_uri,
        "codeVerifier": code_verifier,
    });

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to exchange authorization code")?;

    if !response.status().is_success() {
        let status = response.status();
        let error = response.text().await.unwrap_or_default();
        anyhow::bail!("Token exchange failed: {} - {}", status, error);
    }

    response
        .json()
        .await
        .context("Failed to parse token exchange response")
}

/// Start device authorization flow
pub async fn start_device_authorization(
    client: &Client,
    region: &str,
    client_id: &str,
    client_secret: &str,
    start_url: &str,
) -> Result<DeviceAuthResponse> {
    let url = format!("{}/device_authorization", get_oidc_base_url(region));
    let body = serde_json::json!({
        "clientId": client_id,
        "clientSecret": client_secret,
        "startUrl": start_url,
    });

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to start device authorization")?;

    if !response.status().is_success() {
        let status = response.status();
        let error = response.text().await.unwrap_or_default();
        anyhow::bail!("Device authorization failed: {} - {}", status, error);
    }

    response
        .json()
        .await
        .context("Failed to parse device authorization response")
}

/// Poll for device token (call repeatedly with interval)
pub async fn poll_device_token(
    client: &Client,
    region: &str,
    client_id: &str,
    client_secret: &str,
    device_code: &str,
) -> Result<PollResult> {
    let url = format!("{}/token", get_oidc_base_url(region));
    let body = serde_json::json!({
        "grantType": "urn:ietf:params:oauth:grant-type:device_code",
        "clientId": client_id,
        "clientSecret": client_secret,
        "deviceCode": device_code,
    });

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to poll device token")?;

    if response.status().is_success() {
        let token: TokenExchangeResponse = response
            .json()
            .await
            .context("Failed to parse token response")?;
        return Ok(PollResult::Success(token));
    }

    let error_text = response.text().await.unwrap_or_default();
    if error_text.contains("authorization_pending") {
        Ok(PollResult::Pending)
    } else if error_text.contains("slow_down") {
        Ok(PollResult::SlowDown)
    } else {
        anyhow::bail!("Device token polling failed: {}", error_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_pkce() {
        let pkce = generate_pkce();
        assert!(!pkce.code_verifier.is_empty());
        assert!(!pkce.code_challenge.is_empty());
        assert!(!pkce.state.is_empty());
        // Verifier should be base64url encoded (43+ chars for 32 bytes)
        assert!(pkce.code_verifier.len() >= 43);
        // Challenge should be different from verifier (it's a hash)
        assert_ne!(pkce.code_verifier, pkce.code_challenge);
    }

    #[test]
    fn test_build_authorize_url() {
        let pkce = generate_pkce();
        let url = build_authorize_url(
            "us-east-1",
            "client123",
            "http://localhost:9001/_ui/api/oauth/callback",
            &pkce,
        );
        assert!(url.starts_with("https://oidc.us-east-1.amazonaws.com/authorize?"));
        assert!(url.contains("client123"));
        assert!(url.contains("code_challenge="));
        assert!(url.contains("S256"));
    }

    #[test]
    fn test_pkce_deterministic_challenge() {
        let pkce = generate_pkce();
        let mut hasher = Sha256::new();
        hasher.update(pkce.code_verifier.as_bytes());
        let expected = URL_SAFE_NO_PAD.encode(hasher.finalize());
        assert_eq!(pkce.code_challenge, expected);
    }
}
