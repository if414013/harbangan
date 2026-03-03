use chrono::{DateTime, Utc};
use serde::Deserialize;

/// Type of authentication mechanism
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum AuthType {
    /// AWS SSO credentials via device code OAuth flow
    /// Uses https://oidc.{region}.amazonaws.com/token
    AwsSsoOidc,
}

/// Complete credential set
#[derive(Debug, Clone)]
pub struct Credentials {
    pub refresh_token: String,
    pub access_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub profile_arn: Option<String>,
    pub region: String,

    // AWS SSO OIDC specific fields
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub sso_region: Option<String>,
    #[allow(dead_code)]
    pub scopes: Option<Vec<String>>,
}

/// Token data from refresh response
#[derive(Debug, Clone)]
pub struct TokenData {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub profile_arn: Option<String>,
}

/// AWS SSO OIDC refresh response
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsSsoOidcResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
}

/// OAuth client registration response from AWS SSO OIDC
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientRegistrationResponse {
    pub client_id: String,
    pub client_secret: String,
    pub client_secret_expires_at: i64,
}

/// PKCE state for browser redirect flow
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct PkceState {
    pub code_verifier: String,
    pub code_challenge: String,
    pub state: String,
}

/// Device authorization response from AWS SSO OIDC
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAuthResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// Token exchange response (used by both browser and device flows)
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenExchangeResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    #[allow(dead_code)]
    pub expires_in: Option<u64>,
}

/// Result of polling for device code authorization
#[derive(Debug)]
pub enum PollResult {
    /// User hasn't authorized yet
    Pending,
    /// Polling too fast, slow down
    SlowDown,
    /// Authorization complete
    Success(TokenExchangeResponse),
}
