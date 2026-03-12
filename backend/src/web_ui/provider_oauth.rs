use async_trait::async_trait;
use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::ApiError;
use crate::routes::{AppState, SessionInfo};

// ── Provider OAuth Pending State ─────────────────────────────────────

/// Pending state for the provider OAuth relay flow.
/// Separate from Google SSO's `OAuthPendingState` (different fields).
#[derive(Debug, Clone)]
pub struct ProviderOAuthPendingState {
    pub pkce_verifier: String,
    pub state: String,
    pub user_id: Uuid,
    pub provider: String,
    pub created_at: DateTime<Utc>,
}

// ── Token Exchanger Trait ────────────────────────────────────────────

/// Result of a token exchange or refresh.
#[derive(Debug, Clone)]
pub struct TokenExchangeResult {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub email: String,
}

/// Abstraction for OAuth token exchange (testable via mock).
#[async_trait]
pub trait TokenExchanger: Send + Sync {
    async fn exchange_code(
        &self,
        provider: &str,
        code: &str,
        state: &str,
        pkce_verifier: &str,
        redirect_uri: &str,
    ) -> Result<TokenExchangeResult, ApiError>;

    async fn refresh_token(
        &self,
        provider: &str,
        refresh_token: &str,
    ) -> Result<TokenExchangeResult, ApiError>;
}

// ── Provider Config ──────────────────────────────────────────────────

/// OAuth configuration for a single provider.
struct ProviderOAuthConfig {
    client_id: String,
    client_secret: String,
    token_url: &'static str,
    auth_url: &'static str,
    redirect_uri: &'static str,
    port: u16,
    scopes: &'static [&'static str],
}

fn anthropic_config() -> ProviderOAuthConfig {
    ProviderOAuthConfig {
        client_id: std::env::var("ANTHROPIC_OAUTH_CLIENT_ID")
            .unwrap_or_else(|_| "9d1c250a-e61b-44d9-88ed-5944d1962f5e".to_string()),
        client_secret: String::new(),
        token_url: "https://api.anthropic.com/v1/oauth/token",
        auth_url: "https://claude.ai/oauth/authorize",
        redirect_uri: "http://localhost:54545/callback",
        port: 54545,
        scopes: &["org:create_api_key", "user:profile", "user:inference"],
    }
}

fn gemini_config() -> Result<ProviderOAuthConfig, ApiError> {
    let client_id = std::env::var("GEMINI_OAUTH_CLIENT_ID")
        .or_else(|_| std::env::var("GOOGLE_CLIENT_ID"))
        .unwrap_or_default();
    let client_secret = std::env::var("GEMINI_OAUTH_CLIENT_SECRET")
        .or_else(|_| std::env::var("GOOGLE_CLIENT_SECRET"))
        .unwrap_or_default();
    if client_id.is_empty() || client_secret.is_empty() {
        return Err(ApiError::ValidationError(
            "Gemini OAuth requires GEMINI_OAUTH_CLIENT_ID/GOOGLE_CLIENT_ID and GEMINI_OAUTH_CLIENT_SECRET/GOOGLE_CLIENT_SECRET environment variables".into(),
        ));
    }
    Ok(ProviderOAuthConfig {
        client_id,
        client_secret,
        token_url: "https://oauth2.googleapis.com/token",
        auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
        redirect_uri: "http://localhost:8085/oauth2callback",
        port: 8085,
        scopes: &[
            "openid",
            "https://www.googleapis.com/auth/generative-language",
            "https://www.googleapis.com/auth/userinfo.email",
            "https://www.googleapis.com/auth/userinfo.profile",
        ],
    })
}

fn openai_codex_config() -> ProviderOAuthConfig {
    ProviderOAuthConfig {
        client_id: std::env::var("OPENAI_OAUTH_CLIENT_ID")
            .unwrap_or_else(|_| "app_EMoamEEZ73f0CkXaXp7hrann".to_string()),
        client_secret: String::new(),
        token_url: "https://auth.openai.com/oauth/token",
        auth_url: "https://auth.openai.com/oauth/authorize",
        redirect_uri: "http://localhost:1455/auth/callback",
        port: 1455,
        scopes: &["openid", "email", "profile", "offline_access"],
    }
}

fn get_provider_config(provider: &str) -> Result<ProviderOAuthConfig, ApiError> {
    match provider {
        "anthropic" => Ok(anthropic_config()),
        "gemini" => gemini_config(),
        "openai_codex" => Ok(openai_codex_config()),
        _ => Err(ApiError::ValidationError(format!(
            "Unknown provider: {}",
            provider
        ))),
    }
}

/// Validate that a provider path param is one of the supported providers.
fn validate_provider(provider: &str) -> Result<(), ApiError> {
    match provider {
        "anthropic" | "gemini" | "openai_codex" => Ok(()),
        _ => Err(ApiError::ValidationError(format!(
            "Unknown provider: {}. Must be one of: anthropic, gemini, openai_codex",
            provider
        ))),
    }
}

/// Validate that a domain string is safe for shell interpolation.
fn validate_domain(domain: &str) -> Result<(), ApiError> {
    if domain.is_empty() {
        return Err(ApiError::ConfigError("DOMAIN is not configured".into()));
    }
    if !domain
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._:-".contains(c))
    {
        return Err(ApiError::ConfigError(
            "DOMAIN contains invalid characters".into(),
        ));
    }
    Ok(())
}

// ── PKCE Helpers ─────────────────────────────────────────────────────

/// Generate a PKCE code verifier (128 chars, URL-safe, matching CLIProxyAPI).
fn generate_pkce_verifier() -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use rand::Rng;
    let mut random_bytes = [0u8; 96];
    rand::thread_rng().fill(&mut random_bytes[..]);
    URL_SAFE_NO_PAD.encode(random_bytes)
}

/// Compute S256 PKCE code challenge from verifier.
fn pkce_challenge(verifier: &str) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

// ── Production Token Exchanger ───────────────────────────────────────

/// Production implementation that makes real HTTP calls to provider token endpoints.
pub struct HttpTokenExchanger {
    client: reqwest::Client,
}

impl Default for HttpTokenExchanger {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl HttpTokenExchanger {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl TokenExchanger for HttpTokenExchanger {
    async fn exchange_code(
        &self,
        provider: &str,
        code: &str,
        state: &str,
        pkce_verifier: &str,
        redirect_uri: &str,
    ) -> Result<TokenExchangeResult, ApiError> {
        let config = get_provider_config(provider)?;

        // Anthropic expects JSON body; OpenAI/Gemini expect form-encoded
        let resp = if provider == "anthropic" {
            // Strip #fragment from code (Anthropic may append state after #)
            let clean_code = code.split('#').next().unwrap_or(code);
            let mut body = serde_json::json!({
                "grant_type": "authorization_code",
                "code": clean_code,
                "state": state,
                "redirect_uri": redirect_uri,
                "code_verifier": pkce_verifier,
                "client_id": config.client_id,
            });
            if !config.client_secret.is_empty() {
                body["client_secret"] = serde_json::Value::String(config.client_secret.clone());
            }
            tracing::debug!(provider = %provider, "Sending JSON token exchange to {}", config.token_url);
            self.client
                .post(config.token_url)
                .header("Accept", "application/json")
                .json(&body)
                .send()
                .await
        } else {
            let mut params = vec![
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("code_verifier", pkce_verifier),
                ("client_id", &config.client_id as &str),
            ];
            if !config.client_secret.is_empty() {
                params.push(("client_secret", &config.client_secret));
            }
            self.client
                .post(config.token_url)
                .form(&params)
                .send()
                .await
        }
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Token exchange failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ApiError::Internal(anyhow::anyhow!(
                "Token exchange returned {}: {}",
                status,
                body
            )));
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("Failed to parse token response: {}", e))
        })?;

        let access_token = body["access_token"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let refresh_token = body["refresh_token"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let expires_in = body["expires_in"].as_i64().unwrap_or(3600);

        // Extract email from id_token JWT or userinfo endpoint
        let email = self.extract_email(provider, &body, &access_token).await;

        Ok(TokenExchangeResult {
            access_token,
            refresh_token,
            expires_in,
            email,
        })
    }

    async fn refresh_token(
        &self,
        provider: &str,
        refresh_token: &str,
    ) -> Result<TokenExchangeResult, ApiError> {
        // Qwen uses its own token endpoint with JSON body
        if provider == "qwen" {
            return self.refresh_qwen_token(refresh_token).await;
        }

        let config = get_provider_config(provider)?;

        let mut params = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", &config.client_id),
        ];
        if !config.client_secret.is_empty() {
            params.push(("client_secret", &config.client_secret));
        }

        let resp = self
            .client
            .post(config.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Token refresh failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ApiError::Internal(anyhow::anyhow!(
                "Token refresh returned {}: {}",
                status,
                body
            )));
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| {
            ApiError::Internal(anyhow::anyhow!("Failed to parse refresh response: {}", e))
        })?;

        let access_token = body["access_token"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let new_refresh = body["refresh_token"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let expires_in = body["expires_in"].as_i64().unwrap_or(3600);

        Ok(TokenExchangeResult {
            access_token,
            refresh_token: new_refresh,
            expires_in,
            email: String::new(),
        })
    }
}

impl HttpTokenExchanger {
    /// Refresh a Qwen token via the Qwen OAuth token endpoint.
    async fn refresh_qwen_token(
        &self,
        refresh_token: &str,
    ) -> Result<TokenExchangeResult, ApiError> {
        let client_id = std::env::var("QWEN_OAUTH_CLIENT_ID")
            .unwrap_or_else(|_| "f0304373b74a44d2b584a3fb70ca9e56".to_string());

        let resp = self
            .client
            .post("https://chat.qwen.ai/api/v1/oauth2/token")
            .json(&serde_json::json!({
                "grant_type": "refresh_token",
                "refresh_token": refresh_token,
                "client_id": client_id,
            }))
            .send()
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Qwen token refresh failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ApiError::Internal(anyhow::anyhow!(
                "Qwen token refresh returned {}: {}",
                status,
                body
            )));
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| {
            ApiError::Internal(anyhow::anyhow!(
                "Failed to parse Qwen refresh response: {}",
                e
            ))
        })?;

        let access_token = body["access_token"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let new_refresh = body["refresh_token"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let expires_in = body["expires_in"].as_i64().unwrap_or(3600);

        Ok(TokenExchangeResult {
            access_token,
            refresh_token: new_refresh,
            expires_in,
            email: String::new(),
        })
    }

    /// Extract email from token response (Anthropic), id_token JWT (OpenAI), or userinfo endpoint (Gemini).
    async fn extract_email(
        &self,
        provider: &str,
        token_response: &serde_json::Value,
        access_token: &str,
    ) -> String {
        match provider {
            "anthropic" => {
                // Anthropic: email is in token response at account.email_address
                token_response["account"]["email_address"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string()
            }
            "gemini" => {
                // Gemini: call Google userinfo endpoint
                if let Ok(resp) = self
                    .client
                    .get("https://www.googleapis.com/oauth2/v3/userinfo")
                    .bearer_auth(access_token)
                    .send()
                    .await
                {
                    if let Ok(info) = resp.json::<serde_json::Value>().await {
                        return info["email"].as_str().unwrap_or_default().to_string();
                    }
                }
                String::new()
            }
            _ => {
                // OpenAI: decode id_token JWT payload
                if let Some(id_token) = token_response["id_token"].as_str() {
                    if let Some(email) = decode_jwt_email(id_token) {
                        return email;
                    }
                }
                String::new()
            }
        }
    }
}

/// Decode the email claim from a JWT payload (no signature verification — we trust the provider).
fn decode_jwt_email(jwt: &str) -> Option<String> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    // JWT payload may need padding
    let payload = parts[1];
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    claims["email"].as_str().map(|s| s.to_string())
}

// ── Request/Response Types ───────────────────────────────────────────

#[derive(Serialize)]
struct ProviderStatusInfo {
    connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
}

#[derive(Serialize)]
struct ProvidersStatusResponse {
    providers: serde_json::Map<String, serde_json::Value>,
}

#[derive(Serialize)]
struct ConnectResponse {
    relay_script_url: String,
}

#[derive(Deserialize)]
struct RelayScriptQuery {
    token: String,
}

#[derive(Deserialize)]
struct RelayRequest {
    relay_token: String,
    code: String,
    state: String,
}

// ── Handlers ─────────────────────────────────────────────────────────

/// TTL for pending relay tokens (10 minutes).
const RELAY_TOKEN_TTL_SECS: i64 = 600;
/// Max pending relay tokens per DashMap (prevent memory exhaustion).
const MAX_PENDING_RELAYS: usize = 10_000;

/// GET /_ui/api/providers/status
async fn providers_status(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
) -> Result<Json<ProvidersStatusResponse>, ApiError> {
    let user_id = session.user_id;
    let config_db = state.require_config_db()?;

    let connected = config_db
        .get_user_connected_oauth_providers(user_id)
        .await
        .map_err(ApiError::Internal)?;

    let connected_map: std::collections::HashMap<String, String> = connected.into_iter().collect();

    let mut providers = serde_json::Map::new();
    for pid in &["anthropic", "gemini", "openai_codex"] {
        let email = connected_map.get(*pid).cloned();
        let connected = email.is_some();
        providers.insert(
            pid.to_string(),
            serde_json::to_value(ProviderStatusInfo { connected, email }).unwrap_or_default(),
        );
    }

    // Add Copilot status (separate table, no email — uses github_username)
    let copilot_connected = config_db.has_copilot_token(user_id).await.unwrap_or(false);
    providers.insert(
        "copilot".to_string(),
        serde_json::to_value(ProviderStatusInfo {
            connected: copilot_connected,
            email: None,
        })
        .unwrap_or_default(),
    );

    Ok(Json(ProvidersStatusResponse { providers }))
}

/// GET /_ui/api/providers/:provider/connect
async fn provider_connect(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
    Path(provider): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ConnectResponse>, ApiError> {
    validate_provider(&provider)?;
    // Validate provider config early (catches missing Gemini env vars)
    get_provider_config(&provider)?;

    // Derive base URL from request headers (respects reverse proxy)
    // Priority: Origin (browser-set) > X-Forwarded-Host > Host
    let (scheme, host) = if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        if let Some(rest) = origin.strip_prefix("https://") {
            ("https", rest.to_string())
        } else if let Some(rest) = origin.strip_prefix("http://") {
            ("http", rest.to_string())
        } else {
            ("https", origin.to_string())
        }
    } else {
        let h = headers
            .get("x-forwarded-host")
            .or_else(|| headers.get("host"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("localhost")
            .to_string();
        let s = headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(if h.starts_with("localhost") {
                "http"
            } else {
                "https"
            });
        (s, h)
    };

    let user_id = session.user_id;

    // Generate PKCE
    let verifier = generate_pkce_verifier();

    // Generate state and relay_token
    let oauth_state = Uuid::new_v4().to_string();
    let relay_token = Uuid::new_v4().to_string();

    // Invalidate any existing pending relay for this (user_id, provider)
    let pending = &state.provider_oauth_pending;
    pending.retain(|_, v| !(v.user_id == user_id && v.provider == provider));

    // Enforce cap
    if pending.len() >= MAX_PENDING_RELAYS {
        // Evict expired entries
        let now = Utc::now();
        pending.retain(|_, v| (now - v.created_at).num_seconds() < RELAY_TOKEN_TTL_SECS);
    }

    // Store pending state keyed by relay_token
    pending.insert(
        relay_token.clone(),
        ProviderOAuthPendingState {
            pkce_verifier: verifier,
            state: oauth_state.clone(),
            user_id,
            provider: provider.clone(),
            created_at: Utc::now(),
        },
    );

    let relay_script_url = format!(
        "{}://{}/_ui/api/providers/{}/relay-script?token={}",
        scheme, host, provider, relay_token
    );

    tracing::info!(
        user_id = %user_id,
        provider = %provider,
        "Provider OAuth connect initiated"
    );

    Ok(Json(ConnectResponse { relay_script_url }))
}

/// GET /_ui/api/providers/{provider}/relay-script?token={relay_token}
/// No session auth — the relay_token IS the auth.
async fn relay_script(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(query): Query<RelayScriptQuery>,
    headers: axum::http::HeaderMap,
) -> Result<axum::response::Response, ApiError> {
    validate_provider(&provider)?;

    let relay_token = &query.token;

    // Verify the relay_token exists and matches the provider (don't consume it yet)
    let pending = state.provider_oauth_pending.get(relay_token);
    let pending_state = pending
        .as_ref()
        .ok_or_else(|| ApiError::AuthError("Invalid or expired relay token".into()))?;

    if pending_state.provider != provider {
        return Err(ApiError::ValidationError(
            "Provider mismatch in relay token".into(),
        ));
    }

    // Check TTL
    if (Utc::now() - pending_state.created_at).num_seconds() > RELAY_TOKEN_TTL_SECS {
        drop(pending);
        state.provider_oauth_pending.remove(relay_token);
        return Err(ApiError::AuthError("Relay token expired".into()));
    }

    let config = get_provider_config(&provider)?;

    // Derive base URL from request headers (same logic as provider_connect)
    let (scheme, host) = if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        if let Some(rest) = origin.strip_prefix("https://") {
            ("https", rest.to_string())
        } else if let Some(rest) = origin.strip_prefix("http://") {
            ("http", rest.to_string())
        } else {
            ("https", origin.to_string())
        }
    } else {
        let h = headers
            .get("x-forwarded-host")
            .or_else(|| headers.get("host"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("localhost")
            .to_string();
        let s = headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(if h.starts_with("localhost") {
                "http"
            } else {
                "https"
            });
        (s, h)
    };

    // Build auth URL
    let scopes = config.scopes.join(" ");
    let mut auth_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        config.auth_url,
        urlencoding::encode(&config.client_id),
        urlencoding::encode(config.redirect_uri),
        urlencoding::encode(&scopes),
        urlencoding::encode(&pending_state.state),
        urlencoding::encode(&pkce_challenge(&pending_state.pkce_verifier)),
    );

    // Provider-specific extra params
    match provider.as_str() {
        "openai_codex" => {
            auth_url.push_str(
                "&prompt=login&id_token_add_organizations=true&codex_cli_simplified_flow=true",
            );
        }
        "gemini" => {
            auth_url.push_str("&access_type=offline&prompt=consent");
        }
        _ => {}
    }

    let relay_url = format!("{}://{}/_ui/api/providers/{}/relay", scheme, host, provider);

    let script = format!(
        r#"#!/bin/sh
# harbangan provider relay helper — runs on your machine, relays OAuth code to harbangan
AUTH_URL="{auth_url}"
RELAY_URL="{relay_url}"
RELAY_TOKEN="{relay_token}"
PORT={port}

command -v python3 >/dev/null 2>&1 || {{ echo "Error: python3 is required but not found."; exit 1; }}

python3 -c "
import http.server, urllib.parse, json, urllib.request, sys, os, socket, time

try:
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(('localhost', $PORT))
    s.close()
except OSError:
    print('Error: Port $PORT is already in use. Close the conflicting process and try again.')
    sys.exit(1)

class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        p = urllib.parse.urlparse(self.path)
        q = urllib.parse.parse_qs(p.query)
        code = q.get('code',[''])[0]
        state = q.get('state',[''])[0]
        data = json.dumps({{'relay_token':'$RELAY_TOKEN','code':code,'state':state}}).encode()
        req = urllib.request.Request('$RELAY_URL', data=data, headers={{'Content-Type':'application/json'}})
        for attempt in range(2):
            try:
                resp = urllib.request.urlopen(req, timeout=10)
                if resp.status == 200:
                    self.send_response(200); self.end_headers()
                    self.wfile.write(b'<h2>Connected! You can close this window.</h2>')
                else:
                    self.send_response(200); self.end_headers()
                    self.wfile.write(b'<h2>Error: server returned ' + str(resp.status).encode() + b'</h2>')
                    print('Error: relay server returned HTTP ' + str(resp.status))
                break
            except Exception as e:
                if attempt == 0:
                    time.sleep(2)
                else:
                    self.send_response(200); self.end_headers()
                    self.wfile.write(b'<h2>Error connecting to server</h2>')
                    print('Error: ' + str(e))
        os._exit(0)
    def log_message(self, *a): pass

http.server.HTTPServer(('localhost', $PORT), H).handle_request()
" &
PY_PID=$!
if command -v open >/dev/null 2>&1; then open "$AUTH_URL"
elif command -v xdg-open >/dev/null 2>&1; then xdg-open "$AUTH_URL"
else echo "Open in browser: $AUTH_URL"; fi
echo "Waiting for authorization..."
wait $PY_PID
echo "Done! Provider connected."
"#,
        auth_url = auth_url,
        relay_url = relay_url,
        relay_token = relay_token,
        port = config.port,
    );

    Ok(axum::response::Response::builder()
        .header("content-type", "text/plain; charset=utf-8")
        .body(axum::body::Body::from(script))
        .unwrap())
}

/// POST /_ui/api/providers/{provider}/relay
/// No session auth — the relay_token IS the auth.
async fn relay_callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Json(body): Json<RelayRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    validate_provider(&provider)?;

    // Consume the relay_token (single-use)
    let (_, pending_state) = state
        .provider_oauth_pending
        .remove(&body.relay_token)
        .ok_or_else(|| ApiError::AuthError("Invalid or already consumed relay token".into()))?;

    // Check TTL
    if (Utc::now() - pending_state.created_at).num_seconds() > RELAY_TOKEN_TTL_SECS {
        return Err(ApiError::AuthError("Relay token expired".into()));
    }

    // Verify provider matches
    if pending_state.provider != provider {
        return Err(ApiError::ValidationError(
            "Provider mismatch in relay token".into(),
        ));
    }

    // Verify state matches
    if pending_state.state != body.state {
        return Err(ApiError::ValidationError("State parameter mismatch".into()));
    }

    let config = get_provider_config(&provider)?;

    // Exchange code for tokens
    let result = state
        .token_exchanger
        .exchange_code(
            &provider,
            &body.code,
            &body.state,
            &pending_state.pkce_verifier,
            config.redirect_uri,
        )
        .await?;

    let expires_at = Utc::now() + Duration::seconds(result.expires_in);

    // Store tokens in DB (retry once on failure)
    let config_db = state.require_config_db()?;
    let store_result = config_db
        .upsert_user_provider_token(
            pending_state.user_id,
            &provider,
            &result.access_token,
            &result.refresh_token,
            expires_at,
            &result.email,
        )
        .await;

    if let Err(e) = store_result {
        tracing::warn!(error = ?e, "First DB write failed, retrying...");
        config_db
            .upsert_user_provider_token(
                pending_state.user_id,
                &provider,
                &result.access_token,
                &result.refresh_token,
                expires_at,
                &result.email,
            )
            .await
            .map_err(|e2| {
                ApiError::Internal(anyhow::anyhow!(
                    "Failed to store provider token after retry: {}",
                    e2
                ))
            })?;
    }

    // Invalidate provider registry cache
    state.provider_registry.invalidate(pending_state.user_id);

    tracing::info!(
        user_id = %pending_state.user_id,
        provider = %provider,
        email = %result.email,
        "Provider OAuth token stored"
    );

    Ok(Json(serde_json::json!({ "ok": true })))
}

/// DELETE /_ui/api/providers/{provider}
async fn disconnect_provider(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
    Path(provider): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    validate_provider(&provider)?;

    let config_db = state.require_config_db()?;
    let deleted = config_db
        .delete_user_provider_token(session.user_id, &provider)
        .await
        .map_err(ApiError::Internal)?;

    if deleted == 0 {
        return Err(ApiError::NotFound(format!(
            "No {} connection found for this user",
            provider
        )));
    }

    // Invalidate provider registry cache
    state.provider_registry.invalidate(session.user_id);

    tracing::info!(
        user_id = %session.user_id,
        provider = %provider,
        "Provider disconnected"
    );

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Router ───────────────────────────────────────────────────────────

/// Build the provider OAuth router.
/// Session-authenticated routes: status, connect, disconnect.
/// Relay routes (relay-script, relay callback): authenticated by relay_token, no session needed.
pub fn provider_oauth_routes() -> Router<AppState> {
    Router::new()
        .route("/providers/status", get(providers_status))
        .route("/providers/:provider/connect", get(provider_connect))
        .route("/providers/:provider", delete(disconnect_provider))
}

/// Build the public (no session) relay routes.
/// These are authenticated by the relay_token, not by session cookie.
pub fn provider_oauth_public_routes() -> Router<AppState> {
    Router::new()
        .route("/providers/:provider/relay-script", get(relay_script))
        .route("/providers/:provider/relay", post(relay_callback))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashmap::DashMap;

    #[test]
    fn test_validate_provider_valid() {
        assert!(validate_provider("anthropic").is_ok());
        assert!(validate_provider("gemini").is_ok());
        assert!(validate_provider("openai_codex").is_ok());
    }

    #[test]
    fn test_validate_provider_invalid() {
        assert!(validate_provider("kiro").is_err());
        assert!(validate_provider("foobar").is_err());
        assert!(validate_provider("").is_err());
    }

    #[test]
    fn test_validate_domain_valid() {
        assert!(validate_domain("example.com").is_ok());
        assert!(validate_domain("my-gateway.example.com").is_ok());
        assert!(validate_domain("localhost:8000").is_ok());
    }

    #[test]
    fn test_validate_domain_invalid() {
        assert!(validate_domain("").is_err());
        assert!(validate_domain("example.com; rm -rf /").is_err());
        assert!(validate_domain("example.com$(whoami)").is_err());
    }

    #[test]
    fn test_pkce_verifier_length() {
        let verifier = generate_pkce_verifier();
        assert_eq!(verifier.len(), 128);
    }

    #[test]
    fn test_pkce_challenge_deterministic() {
        let verifier = "test-verifier-string";
        let c1 = pkce_challenge(verifier);
        let c2 = pkce_challenge(verifier);
        assert_eq!(c1, c2);
    }

    #[test]
    fn test_pkce_challenge_different_for_different_verifiers() {
        let c1 = pkce_challenge("verifier-1");
        let c2 = pkce_challenge("verifier-2");
        assert_ne!(c1, c2);
    }

    #[test]
    fn test_decode_jwt_email() {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        // Build a fake JWT with email claim
        let header = URL_SAFE_NO_PAD.encode(b"{}");
        let payload = URL_SAFE_NO_PAD.encode(
            serde_json::json!({"email": "test@example.com"})
                .to_string()
                .as_bytes(),
        );
        let jwt = format!("{}.{}.signature", header, payload);
        assert_eq!(decode_jwt_email(&jwt), Some("test@example.com".to_string()));
    }

    #[test]
    fn test_decode_jwt_email_no_email() {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        let header = URL_SAFE_NO_PAD.encode(b"{}");
        let payload =
            URL_SAFE_NO_PAD.encode(serde_json::json!({"sub": "12345"}).to_string().as_bytes());
        let jwt = format!("{}.{}.sig", header, payload);
        assert_eq!(decode_jwt_email(&jwt), None);
    }

    #[test]
    fn test_decode_jwt_email_invalid() {
        assert_eq!(decode_jwt_email("not-a-jwt"), None);
        assert_eq!(decode_jwt_email(""), None);
    }

    #[test]
    fn test_get_provider_config_all() {
        assert!(get_provider_config("anthropic").is_ok());
        // Gemini requires env vars — test that it errors without them
        std::env::remove_var("GEMINI_OAUTH_CLIENT_ID");
        assert!(get_provider_config("gemini").is_err());
        // With env vars set, it should succeed
        std::env::set_var("GEMINI_OAUTH_CLIENT_ID", "test-id");
        std::env::set_var("GEMINI_OAUTH_CLIENT_SECRET", "test-secret");
        assert!(get_provider_config("gemini").is_ok());
        assert!(get_provider_config("openai_codex").is_ok());
        assert!(get_provider_config("unknown").is_err());
    }

    #[test]
    fn test_provider_config_ports() {
        assert_eq!(get_provider_config("anthropic").unwrap().port, 54545);
        std::env::set_var("GEMINI_OAUTH_CLIENT_ID", "test-id");
        std::env::set_var("GEMINI_OAUTH_CLIENT_SECRET", "test-secret");
        assert_eq!(get_provider_config("gemini").unwrap().port, 8085);
        assert_eq!(get_provider_config("openai_codex").unwrap().port, 1455);
    }

    #[test]
    fn test_provider_status_serialization() {
        let status = ProviderStatusInfo {
            connected: true,
            email: Some("user@example.com".to_string()),
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["connected"], true);
        assert_eq!(json["email"], "user@example.com");
    }

    #[test]
    fn test_provider_status_disconnected_omits_email() {
        let status = ProviderStatusInfo {
            connected: false,
            email: None,
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["connected"], false);
        assert!(json.get("email").is_none());
    }

    #[test]
    fn test_pending_state_ttl_check() {
        let fresh = ProviderOAuthPendingState {
            pkce_verifier: "v".into(),
            state: "s".into(),
            user_id: Uuid::new_v4(),
            provider: "anthropic".into(),
            created_at: Utc::now(),
        };
        assert!((Utc::now() - fresh.created_at).num_seconds() < RELAY_TOKEN_TTL_SECS);

        let expired = ProviderOAuthPendingState {
            pkce_verifier: "v".into(),
            state: "s".into(),
            user_id: Uuid::new_v4(),
            provider: "anthropic".into(),
            created_at: Utc::now() - Duration::seconds(RELAY_TOKEN_TTL_SECS + 1),
        };
        assert!((Utc::now() - expired.created_at).num_seconds() > RELAY_TOKEN_TTL_SECS);
    }

    #[test]
    fn test_relay_request_deserialization() {
        let json = serde_json::json!({
            "relay_token": "abc-123",
            "code": "auth-code",
            "state": "state-param"
        });
        let req: RelayRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.relay_token, "abc-123");
        assert_eq!(req.code, "auth-code");
        assert_eq!(req.state, "state-param");
    }

    #[test]
    fn test_pending_state_invalidation_by_user_provider() {
        let pending: DashMap<String, ProviderOAuthPendingState> = DashMap::new();
        let user_id = Uuid::new_v4();

        pending.insert(
            "token-1".into(),
            ProviderOAuthPendingState {
                pkce_verifier: "v1".into(),
                state: "s1".into(),
                user_id,
                provider: "anthropic".into(),
                created_at: Utc::now(),
            },
        );
        pending.insert(
            "token-2".into(),
            ProviderOAuthPendingState {
                pkce_verifier: "v2".into(),
                state: "s2".into(),
                user_id: Uuid::new_v4(), // different user
                provider: "anthropic".into(),
                created_at: Utc::now(),
            },
        );

        // Invalidate for user_id + anthropic
        pending.retain(|_, v| !(v.user_id == user_id && v.provider == "anthropic"));

        assert_eq!(pending.len(), 1);
        assert!(pending.get("token-2").is_some());
    }

    #[test]
    fn test_connect_response_serialization() {
        let resp = ConnectResponse {
            relay_script_url:
                "https://gw.example.com/_ui/api/providers/anthropic/relay-script?token=abc"
                    .to_string(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json["relay_script_url"]
            .as_str()
            .unwrap()
            .contains("relay-script?token="));
    }

    #[test]
    fn test_relay_script_query_deserialization() {
        let json = serde_json::json!({"token": "my-relay-token"});
        let q: RelayScriptQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.token, "my-relay-token");
    }

    #[test]
    fn test_providers_status_response_structure() {
        let mut providers = serde_json::Map::new();
        for pid in &["anthropic", "gemini", "openai_codex"] {
            providers.insert(
                pid.to_string(),
                serde_json::to_value(ProviderStatusInfo {
                    connected: *pid == "anthropic",
                    email: if *pid == "anthropic" {
                        Some("user@anthropic.com".to_string())
                    } else {
                        None
                    },
                })
                .unwrap(),
            );
        }
        let resp = ProvidersStatusResponse { providers };
        let json = serde_json::to_value(&resp).unwrap();
        // All three providers must always be present
        assert!(json["providers"]["anthropic"]["connected"]
            .as_bool()
            .unwrap());
        assert_eq!(
            json["providers"]["anthropic"]["email"],
            "user@anthropic.com"
        );
        assert!(!json["providers"]["gemini"]["connected"].as_bool().unwrap());
        assert!(!json["providers"]["openai_codex"]["connected"]
            .as_bool()
            .unwrap());
    }

    #[test]
    fn test_relay_token_consumed_on_remove() {
        let pending: DashMap<String, ProviderOAuthPendingState> = DashMap::new();
        pending.insert(
            "token-once".into(),
            ProviderOAuthPendingState {
                pkce_verifier: "v".into(),
                state: "s".into(),
                user_id: Uuid::new_v4(),
                provider: "anthropic".into(),
                created_at: Utc::now(),
            },
        );

        // First remove succeeds (single-use consumption)
        let first = pending.remove("token-once");
        assert!(first.is_some());

        // Second remove fails (already consumed)
        let second = pending.remove("token-once");
        assert!(second.is_none());
    }

    #[test]
    fn test_relay_state_mismatch_detection() {
        let pending: DashMap<String, ProviderOAuthPendingState> = DashMap::new();
        let stored_state = "correct-state";
        pending.insert(
            "token-x".into(),
            ProviderOAuthPendingState {
                pkce_verifier: "v".into(),
                state: stored_state.into(),
                user_id: Uuid::new_v4(),
                provider: "anthropic".into(),
                created_at: Utc::now(),
            },
        );

        let entry = pending.get("token-x").unwrap();
        let request_state = "wrong-state";
        assert_ne!(
            entry.state, request_state,
            "State mismatch should be detected"
        );
        assert_eq!(entry.state, stored_state);
    }

    #[test]
    fn test_relay_provider_mismatch_detection() {
        let pending: DashMap<String, ProviderOAuthPendingState> = DashMap::new();
        pending.insert(
            "token-y".into(),
            ProviderOAuthPendingState {
                pkce_verifier: "v".into(),
                state: "s".into(),
                user_id: Uuid::new_v4(),
                provider: "anthropic".into(),
                created_at: Utc::now(),
            },
        );

        let entry = pending.get("token-y").unwrap();
        // Request comes in on /providers/openai_codex/relay but token was for anthropic
        assert_ne!(
            entry.provider, "openai_codex",
            "Provider mismatch should be detected"
        );
        assert_eq!(entry.provider, "anthropic");
    }

    #[test]
    fn test_max_pending_relays_cap_eviction() {
        let pending: DashMap<String, ProviderOAuthPendingState> = DashMap::new();

        // Insert MAX_PENDING_RELAYS entries, all expired
        for i in 0..MAX_PENDING_RELAYS {
            pending.insert(
                format!("token-{}", i),
                ProviderOAuthPendingState {
                    pkce_verifier: "v".into(),
                    state: "s".into(),
                    user_id: Uuid::new_v4(),
                    provider: "anthropic".into(),
                    created_at: Utc::now() - Duration::seconds(RELAY_TOKEN_TTL_SECS + 1),
                },
            );
        }
        assert_eq!(pending.len(), MAX_PENDING_RELAYS);

        // Simulate the cap enforcement logic from provider_connect handler
        if pending.len() >= MAX_PENDING_RELAYS {
            let now = Utc::now();
            pending.retain(|_, v| (now - v.created_at).num_seconds() < RELAY_TOKEN_TTL_SECS);
        }

        // All expired entries should be evicted
        assert_eq!(pending.len(), 0);
    }

    #[test]
    fn test_unknown_relay_token_returns_none() {
        let pending: DashMap<String, ProviderOAuthPendingState> = DashMap::new();
        // Random UUID that was never inserted
        let result = pending.remove(&Uuid::new_v4().to_string());
        assert!(result.is_none());
    }

    #[test]
    fn test_relay_token_ttl_constants() {
        assert_eq!(
            RELAY_TOKEN_TTL_SECS, 600,
            "Relay token TTL should be 10 minutes"
        );
        assert_eq!(
            MAX_PENDING_RELAYS, 10_000,
            "Max pending relays should be 10k"
        );
    }
}
