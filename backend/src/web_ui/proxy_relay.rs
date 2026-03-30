//! Proxy-mode relay endpoints for OAuth provider connection.
//!
//! These endpoints are mounted at `/_proxy/providers/` in proxy-only mode.
//! They reuse the existing PKCE relay flow from `provider_oauth.rs` but:
//! - Auth via `PROXY_API_KEY` header instead of session cookie
//! - Store tokens to file via `ProxyTokenManager` instead of DB
//! - Use `ProxyConfig` OAuth client IDs instead of DB config

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::routes::AppState;

// ── Request/Response types ───────────────────────────────────────────

#[derive(Serialize)]
struct ProxyConnectResponse {
    relay_script_url: String,
}

#[derive(Serialize)]
struct ProxyStatusResponse {
    providers: std::collections::HashMap<String, ProxyProviderStatus>,
}

#[derive(Serialize)]
struct ProxyProviderStatus {
    connected: bool,
}

#[derive(Deserialize)]
struct ProxyRelayScriptQuery {
    token: String,
}

#[derive(Deserialize)]
struct ProxyRelayRequest {
    relay_token: String,
    code: String,
    state: String,
}

// ── Auth helper ──────────────────────────────────────────────────────

/// Validate PROXY_API_KEY from Authorization header.
fn verify_proxy_key(headers: &axum::http::HeaderMap, state: &AppState) -> Result<(), ApiError> {
    let expected_hash = state
        .proxy_api_key_hash
        .ok_or_else(|| ApiError::AuthError("Proxy mode not configured".into()))?;

    let provided = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| {
            ApiError::AuthError("Missing Authorization: Bearer <PROXY_API_KEY>".into())
        })?;

    use sha2::{Digest, Sha256};
    let provided_hash: [u8; 32] = Sha256::digest(provided.as_bytes()).into();

    use subtle::ConstantTimeEq;
    if provided_hash.ct_eq(&expected_hash).into() {
        Ok(())
    } else {
        Err(ApiError::AuthError("Invalid PROXY_API_KEY".into()))
    }
}

// ── Handlers ─────────────────────────────────────────────────────────

/// GET /_proxy/providers/status
async fn proxy_providers_status(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ProxyStatusResponse>, ApiError> {
    verify_proxy_key(&headers, &state)?;

    let ptm = state
        .proxy_token_manager
        .as_ref()
        .ok_or_else(|| ApiError::ConfigError("ProxyTokenManager not available".into()))?;

    let status_map = ptm.provider_status();
    let providers = status_map
        .into_iter()
        .map(|(k, v)| (k, ProxyProviderStatus { connected: v }))
        .collect();

    Ok(Json(ProxyStatusResponse { providers }))
}

/// POST /_proxy/providers/:provider/connect
async fn proxy_provider_connect(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ProxyConnectResponse>, ApiError> {
    verify_proxy_key(&headers, &state)?;

    // Validate provider supports OAuth relay
    use std::str::FromStr;
    let pid = crate::providers::types::ProviderId::from_str(&provider)
        .map_err(ApiError::ValidationError)?;
    if pid.category() != "oauth_relay" {
        return Err(ApiError::ValidationError(format!(
            "Provider '{}' does not support OAuth relay",
            provider
        )));
    }

    // Validate provider config (client ID must be set)
    let app_config = state
        .config
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();

    // Check proxy config has the OAuth client ID
    let _client_id = match provider.as_str() {
        "anthropic" => {
            let cid = app_config
                .proxy
                .as_ref()
                .and_then(|p| p.anthropic_oauth_client_id.clone())
                .or_else(|| {
                    let id = &app_config.anthropic_oauth_client_id;
                    if id.is_empty() {
                        None
                    } else {
                        Some(id.clone())
                    }
                })
                .ok_or_else(|| {
                    ApiError::ConfigError("ANTHROPIC_OAUTH_CLIENT_ID not configured".into())
                })?;
            cid
        }
        "openai_codex" => {
            let cid = app_config
                .proxy
                .as_ref()
                .and_then(|p| p.openai_oauth_client_id.clone())
                .or_else(|| {
                    let id = &app_config.openai_oauth_client_id;
                    if id.is_empty() {
                        None
                    } else {
                        Some(id.clone())
                    }
                })
                .ok_or_else(|| {
                    ApiError::ConfigError("OPENAI_OAUTH_CLIENT_ID not configured".into())
                })?;
            cid
        }
        _ => {
            return Err(ApiError::ValidationError(format!(
                "Unknown OAuth relay provider: {}",
                provider
            )));
        }
    };

    // Generate PKCE + state + relay_token
    use crate::web_ui::provider_oauth::ProviderOAuthPendingState;
    let verifier = generate_pkce_verifier();
    let oauth_state = uuid::Uuid::new_v4().to_string();
    let relay_token = uuid::Uuid::new_v4().to_string();

    // Use PROXY_USER_ID as the user_id for proxy mode
    let user_id = crate::routes::PROXY_USER_ID;

    let pending = &state.provider_oauth_pending;
    // Invalidate existing pending for this provider
    pending.retain(|_, v| !(v.user_id == user_id && v.provider == provider));

    pending.insert(
        relay_token.clone(),
        ProviderOAuthPendingState {
            pkce_verifier: verifier,
            state: oauth_state,
            user_id,
            provider: provider.clone(),
            created_at: chrono::Utc::now(),
        },
    );

    // Derive base URL from request headers
    let (scheme, host) = derive_base_url(&headers);

    let relay_script_url = format!(
        "{}://{}/_proxy/providers/{}/relay-script?token={}",
        scheme, host, provider, relay_token
    );

    tracing::info!(provider = %provider, "Proxy OAuth connect initiated");

    Ok(Json(ProxyConnectResponse { relay_script_url }))
}

/// GET /_proxy/providers/:provider/relay-script?token=...
async fn proxy_relay_script(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(query): Query<ProxyRelayScriptQuery>,
    headers: axum::http::HeaderMap,
) -> Result<axum::response::Response, ApiError> {
    let relay_token = &query.token;

    let pending = state.provider_oauth_pending.get(relay_token);
    let pending_state = pending
        .as_ref()
        .ok_or_else(|| ApiError::AuthError("Invalid or expired relay token".into()))?;

    if pending_state.provider != provider {
        return Err(ApiError::ValidationError(
            "Provider mismatch in relay token".into(),
        ));
    }

    if (chrono::Utc::now() - pending_state.created_at).num_seconds() > 600 {
        drop(pending);
        state.provider_oauth_pending.remove(relay_token);
        return Err(ApiError::AuthError("Relay token expired".into()));
    }

    let app_config = state
        .config
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();

    // Get provider OAuth config from proxy config or global config
    let (client_id, _token_url, auth_url, redirect_uri, port, scopes) =
        proxy_provider_oauth_config(&provider, &app_config)?;

    let (scheme, host) = derive_base_url(&headers);

    let scopes_str = scopes.join(" ");
    let challenge = pkce_challenge(&pending_state.pkce_verifier);
    let mut auth_url_full = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        auth_url,
        urlencoding::encode(&client_id),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(&scopes_str),
        urlencoding::encode(&pending_state.state),
        urlencoding::encode(&challenge),
    );

    if provider == "openai_codex" {
        auth_url_full.push_str(
            "&prompt=login&id_token_add_organizations=true&codex_cli_simplified_flow=true",
        );
    }

    let relay_url = format!("{}://{}/_proxy/providers/{}/relay", scheme, host, provider);

    let script = format!(
        r#"#!/bin/sh
# harbangan proxy relay — runs on your machine, relays OAuth code to the gateway
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
        auth_url = auth_url_full,
        relay_url = relay_url,
        relay_token = relay_token,
        port = port,
    );

    Ok(axum::response::Response::builder()
        .header("content-type", "text/plain; charset=utf-8")
        .body(axum::body::Body::from(script))
        .unwrap())
}

/// POST /_proxy/providers/:provider/relay
async fn proxy_relay_callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Json(body): Json<ProxyRelayRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Consume the relay_token (single-use)
    let (_, pending_state) = state
        .provider_oauth_pending
        .remove(&body.relay_token)
        .ok_or_else(|| ApiError::AuthError("Invalid or already consumed relay token".into()))?;

    if (chrono::Utc::now() - pending_state.created_at).num_seconds() > 600 {
        return Err(ApiError::AuthError("Relay token expired".into()));
    }
    if pending_state.provider != provider {
        return Err(ApiError::ValidationError(
            "Provider mismatch in relay token".into(),
        ));
    }
    if pending_state.state != body.state {
        return Err(ApiError::ValidationError("State parameter mismatch".into()));
    }

    let app_config = state
        .config
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();

    let (_client_id, _token_url, _auth_url, redirect_uri, _port, _scopes) =
        proxy_provider_oauth_config(&provider, &app_config)?;

    // Exchange code for tokens
    let result = state
        .token_exchanger
        .exchange_code(
            &provider,
            &body.code,
            &body.state,
            &pending_state.pkce_verifier,
            &redirect_uri,
        )
        .await?;

    // Store tokens to file via ProxyTokenManager
    let ptm = state
        .proxy_token_manager
        .as_ref()
        .ok_or_else(|| ApiError::ConfigError("ProxyTokenManager not available".into()))?;

    ptm.store_oauth_tokens(
        &provider,
        &result.access_token,
        &result.refresh_token,
        result.expires_in,
    )
    .await
    .map_err(|e| ApiError::Internal(e))?;

    tracing::info!(provider = %provider, "Proxy OAuth relay completed — provider connected");

    Ok(Json(serde_json::json!({ "status": "connected" })))
}

// ── Helpers ──────────────────────────────────────────────────────────

fn derive_base_url(headers: &axum::http::HeaderMap) -> (&str, String) {
    if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        if let Some(rest) = origin.strip_prefix("https://") {
            return ("https", rest.to_string());
        } else if let Some(rest) = origin.strip_prefix("http://") {
            return ("http", rest.to_string());
        }
        return ("https", origin.to_string());
    }
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
}

fn generate_pkce_verifier() -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use rand::Rng;
    let mut random_bytes = [0u8; 96];
    rand::thread_rng().fill(&mut random_bytes[..]);
    URL_SAFE_NO_PAD.encode(random_bytes)
}

fn pkce_challenge(verifier: &str) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

/// Get OAuth config for a provider in proxy mode.
/// Returns (client_id, token_url, auth_url, redirect_uri, port, scopes).
fn proxy_provider_oauth_config(
    provider: &str,
    config: &crate::config::Config,
) -> Result<
    (
        String,
        &'static str,
        &'static str,
        String,
        u16,
        Vec<&'static str>,
    ),
    ApiError,
> {
    match provider {
        "anthropic" => {
            let client_id = config
                .proxy
                .as_ref()
                .and_then(|p| p.anthropic_oauth_client_id.clone())
                .or_else(|| {
                    let id = &config.anthropic_oauth_client_id;
                    if id.is_empty() {
                        None
                    } else {
                        Some(id.clone())
                    }
                })
                .ok_or_else(|| {
                    ApiError::ConfigError("ANTHROPIC_OAUTH_CLIENT_ID not configured".into())
                })?;
            Ok((
                client_id,
                "https://api.anthropic.com/v1/oauth/token",
                "https://claude.ai/oauth/authorize",
                "http://localhost:54545/callback".to_string(),
                54545,
                vec!["org:create_api_key", "user:profile", "user:inference"],
            ))
        }
        "openai_codex" | "openai" => {
            let client_id = config
                .proxy
                .as_ref()
                .and_then(|p| p.openai_oauth_client_id.clone())
                .or_else(|| {
                    let id = &config.openai_oauth_client_id;
                    if id.is_empty() {
                        None
                    } else {
                        Some(id.clone())
                    }
                })
                .ok_or_else(|| {
                    ApiError::ConfigError("OPENAI_OAUTH_CLIENT_ID not configured".into())
                })?;
            Ok((
                client_id,
                "https://auth.openai.com/oauth/token",
                "https://auth.openai.com/oauth/authorize",
                "http://localhost:1455/auth/callback".to_string(),
                1455,
                vec!["openid", "email", "profile", "offline_access"],
            ))
        }
        _ => Err(ApiError::ValidationError(format!(
            "Unknown OAuth relay provider: {}",
            provider
        ))),
    }
}

// ── Router ───────────────────────────────────────────────────────────

/// Build the proxy relay router (mounted at `/_proxy/providers` in proxy mode).
pub fn proxy_relay_routes(state: AppState) -> Router {
    Router::new()
        .route("/_proxy/providers/status", get(proxy_providers_status))
        .route(
            "/_proxy/providers/{provider}/connect",
            post(proxy_provider_connect),
        )
        .route(
            "/_proxy/providers/{provider}/relay-script",
            get(proxy_relay_script),
        )
        .route(
            "/_proxy/providers/{provider}/relay",
            post(proxy_relay_callback),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_base_url_from_origin() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("origin", "https://gateway.example.com".parse().unwrap());
        let (scheme, host) = derive_base_url(&headers);
        assert_eq!(scheme, "https");
        assert_eq!(host, "gateway.example.com");
    }

    #[test]
    fn test_derive_base_url_from_host() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("host", "localhost:8000".parse().unwrap());
        let (scheme, host) = derive_base_url(&headers);
        assert_eq!(scheme, "http");
        assert_eq!(host, "localhost:8000");
    }

    #[test]
    fn test_derive_base_url_from_forwarded() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-forwarded-host", "gw.example.com".parse().unwrap());
        headers.insert("x-forwarded-proto", "https".parse().unwrap());
        let (scheme, host) = derive_base_url(&headers);
        assert_eq!(scheme, "https");
        assert_eq!(host, "gw.example.com");
    }

    #[test]
    fn test_derive_base_url_no_headers() {
        let headers = axum::http::HeaderMap::new();
        let (scheme, host) = derive_base_url(&headers);
        assert_eq!(scheme, "http");
        assert_eq!(host, "localhost");
    }

    #[test]
    fn test_pkce_verifier_length() {
        let v = generate_pkce_verifier();
        assert_eq!(v.len(), 128);
    }

    #[test]
    fn test_pkce_challenge_deterministic() {
        let c1 = pkce_challenge("test-verifier");
        let c2 = pkce_challenge("test-verifier");
        assert_eq!(c1, c2);
        assert!(!c1.is_empty());
    }

    #[test]
    fn test_proxy_provider_oauth_config_anthropic() {
        let mut config = crate::config::Config::with_defaults();
        config.anthropic_oauth_client_id = "test-ant-id".to_string();
        let (cid, token_url, auth_url, redirect, port, scopes) =
            proxy_provider_oauth_config("anthropic", &config).unwrap();
        assert_eq!(cid, "test-ant-id");
        assert!(token_url.contains("anthropic"));
        assert!(auth_url.contains("claude"));
        assert_eq!(port, 54545);
        assert!(redirect.contains("54545"));
        assert!(!scopes.is_empty());
    }

    #[test]
    fn test_proxy_provider_oauth_config_openai() {
        let mut config = crate::config::Config::with_defaults();
        config.openai_oauth_client_id = "test-oai-id".to_string();
        let (cid, _, _, _, port, _) = proxy_provider_oauth_config("openai_codex", &config).unwrap();
        assert_eq!(cid, "test-oai-id");
        assert_eq!(port, 1455);
    }

    #[test]
    fn test_proxy_provider_oauth_config_unknown_fails() {
        let config = crate::config::Config::with_defaults();
        let result = proxy_provider_oauth_config("unknown", &config);
        assert!(result.is_err());
    }

    #[test]
    fn test_proxy_provider_oauth_config_missing_client_id() {
        let config = crate::config::Config::with_defaults();
        let result = proxy_provider_oauth_config("anthropic", &config);
        assert!(result.is_err());
    }

    #[test]
    fn test_proxy_provider_oauth_config_from_proxy_config() {
        let mut config = crate::config::Config::with_defaults();
        config.proxy = Some(crate::config::ProxyConfig {
            api_key: "test-key-long-enough".to_string(),
            anthropic_oauth_client_id: Some("proxy-ant-id".to_string()),
            ..Default::default()
        });
        let (cid, _, _, _, _, _) = proxy_provider_oauth_config("anthropic", &config).unwrap();
        assert_eq!(cid, "proxy-ant-id");
    }
}
