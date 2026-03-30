# Plan: Gateway Mode OAuth Support for Copilot, Anthropic & OpenAI Codex

## Context

The gateway/proxy mode (`docker-compose.gateway.yml`) currently authenticates to Anthropic and OpenAI via **static API keys** (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`), while the full deployment uses **OAuth PKCE relay** flows. Additionally, the Copilot device code flow works at startup but has **no runtime token refresh** (session tokens expire ~30 min). The custom provider exists only in gateway mode and has no equivalent in full deployment.

**Goal**: Make gateway mode use OAuth-based auth for Anthropic and OpenAI Codex (matching full deployment), fix Copilot token refresh, and remove the custom provider.

**Constraints**: No database. Single container. File-based token persistence in `/data/tokens.json`.

## Consultation Summary

- **rust-backend-engineer**: Proxy mode builds `ProviderRegistry` from static env vars in `from_proxy_config()`. Full mode uses `provider_oauth.rs` relay + `user_provider_tokens` DB table with refresh. Copilot proxy mode has no token refresh. OAuth client IDs are DB-backed config in full mode; need env vars for proxy.
- **react-frontend-engineer**: All OAuth flows are backend-driven. Relay flow shows `curl | sh` command — already CLI-friendly. Frontend NOT required for gateway mode OAuth.
- **database-engineer**: Zero schema changes needed for file-based approach. Existing `/data/tokens.json` pattern is sufficient.
- **devops-engineer**: Gateway runs single container, no DB. `entrypoint.sh` handles Kiro/Copilot device flows at startup. Dockerfile uses `debian:bookworm-slim`.
- **backend-qa**: 257 existing tests across provider auth. `MockExchanger` and `make_proxy_creds()` available as test helpers. No gateway-mode OAuth tests exist.
- **frontend-qa**: No gateway-mode E2E tests exist. 4 providers expected in registry.
- **document-writer**: Dead `GITHUB_COPILOT_CLIENT_ID/SECRET/CALLBACK_URL` env vars in `docker-compose.yml`. `.env.example` missing OAuth client ID vars.

## Approach: Relay-Script OAuth via Rust Binary (Codex-adjusted)

> **Codex HIGH finding addressed**: The original plan proposed manual stdin code-paste in `entrypoint.sh`, but `docker compose up -d` runs detached with no stdin. The existing Kiro/Copilot device flows work because they print a URL and poll remotely — no terminal input. Adjusted approach: **mount relay endpoints in the Rust binary for proxy mode** and let users connect via `curl | sh` from their host machine.

### How it works:

1. `entrypoint.sh` handles Kiro (device code) and Copilot (device code) as before — these work in detached mode
2. `entrypoint.sh` starts the Rust binary with `exec /app/harbangan` (unchanged)
3. The Rust binary detects unconfigured Anthropic/OpenAI providers and logs relay instructions:
   ```
   [INFO] Anthropic not connected. To connect, run on your host:
         curl -fsSL 'http://localhost:8000/_proxy/providers/anthropic/relay-script' | sh
   ```
4. User runs the `curl | sh` command on their host machine
5. The relay script (served by the binary) handles the full PKCE OAuth flow locally on the user's machine (opens browser, catches callback on localhost)
6. Script sends tokens back to the gateway's relay callback endpoint
7. Gateway stores tokens in `/data/tokens.json` and updates live credentials
8. Background refresh task keeps tokens alive

This reuses the existing `provider_oauth.rs` relay code with file-backed storage instead of DB.

## File Manifest

> **Codex HIGH finding addressed**: Custom provider removal touches 15 files, not just 5. Expanded manifest below.

| File | Action | Owner | Wave |
|------|--------|-------|------|
| `backend/src/config.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/providers/types.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/providers/registry.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/providers/mod.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/providers/custom.rs` | delete | rust-backend-engineer | 1 |
| `backend/src/routes/openai.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/routes/mod.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/web_ui/model_registry.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/web_ui/admin_pool.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/converters/mod.rs` | modify (if Custom ref) | rust-backend-engineer | 1 |
| `backend/src/proxy_token_manager.rs` | create | rust-backend-engineer | 1 |
| `backend/src/main.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/web_ui/provider_oauth.rs` | modify (add proxy relay routes) | rust-backend-engineer | 1 |
| `backend/entrypoint.sh` | modify | devops-engineer | 2 |
| `docker-compose.gateway.yml` | modify | devops-engineer | 2 |
| `docker-compose.yml` | modify | devops-engineer | 2 |
| `.env.example` | modify | devops-engineer | 2 |

## Wave 1: Backend — Relay Endpoints, Token Refresh & Custom Removal (rust-backend-engineer)

### 1.1 Update `ProxyConfig` struct (`backend/src/config.rs:9-26`)

Remove:
- `anthropic_api_key`, `openai_api_key`, `openai_base_url`
- `custom_provider_url`, `custom_provider_key`, `custom_provider_models`

Add:
- `anthropic_enabled: bool` (from `ANTHROPIC_ENABLED`, default false)
- `anthropic_access_token: Option<String>` (from `ANTHROPIC_ACCESS_TOKEN`)
- `anthropic_refresh_token: Option<String>` (from `ANTHROPIC_REFRESH_TOKEN`)
- `anthropic_oauth_client_id: Option<String>` (from `ANTHROPIC_OAUTH_CLIENT_ID`)
- `openai_enabled: bool` (from `OPENAI_ENABLED`, default false)
- `openai_access_token: Option<String>` (from `OPENAI_ACCESS_TOKEN`)
- `openai_refresh_token: Option<String>` (from `OPENAI_REFRESH_TOKEN`)
- `openai_oauth_client_id: Option<String>` (from `OPENAI_OAUTH_CLIENT_ID`)
- `copilot_github_token: Option<String>` (from `COPILOT_GITHUB_TOKEN`)

### 1.2 Remove Custom provider (all call sites)

Delete `backend/src/providers/custom.rs` entirely. Remove `ProviderId::Custom` from:
- `providers/types.rs` — enum variant, `as_str()`, `display_name()`, `category()`, `supports_pool()`, `all_visible()`, `default_base_url()`, `FromStr`, `Display`, all Custom tests
- `providers/registry.rs` — `custom_models` field, `new_with_proxy()` custom_models param, `from_proxy_config()` Custom block, `resolve_from_proxy_creds()` Custom branch, `custom_model_names()` method, all Custom-related tests (~20 tests)
- `providers/mod.rs` — Custom module import and dispatch
- `routes/openai.rs:66` — custom model listing in `/v1/models`
- `routes/mod.rs:239` — `test_get_models_proxy_custom_models_appear` test
- `web_ui/model_registry.rs:400` — `ProviderId::Custom => None` match arm
- `web_ui/admin_pool.rs` — any Custom references in pool tests
- `converters/mod.rs` — any Custom dispatch (verify before editing)

### 1.3 Mount relay endpoints in proxy mode (`backend/src/main.rs`, `provider_oauth.rs`)

Currently `main.rs:676` only mounts web UI routes in full mode (`if !is_proxy_only`). Add a **minimal proxy relay router** that mounts in proxy mode:

```
/_proxy/providers/:provider/connect      → initiate relay (no session auth, use PROXY_API_KEY)
/_proxy/providers/:provider/relay-script  → serve the Python relay script
/_proxy/providers/:provider/relay         → receive tokens from relay script
/_proxy/providers/status                  → check which providers are connected
```

These endpoints reuse the existing `provider_oauth.rs` relay logic but:
- Auth via `PROXY_API_KEY` header instead of session cookie
- Store tokens to `/data/tokens.json` instead of DB
- Use `ProxyConfig` OAuth client IDs instead of DB config

### 1.4 Create `ProxyTokenManager` (`backend/src/proxy_token_manager.rs`)

> **Codex MEDIUM finding addressed**: Use `tokio::sync::Mutex` for serialized file writes, atomic temp-file rename with `0o600` permissions (not umask).

```rust
pub struct ProxyTokenManager {
    token_file: PathBuf,
    file_lock: tokio::sync::Mutex<()>,  // serialize all file writes
    http_client: reqwest::Client,
}
```

**File write pattern**: Read file → modify in memory → write to `.tmp` → `std::fs::set_permissions(0o600)` → `std::fs::rename()` (atomic). All writes go through `file_lock`.

**Token refresh:**
- **Anthropic/OpenAI**: Reuse `TokenExchanger::refresh_token()` from `provider_oauth.rs`. Check `expires_at`, refresh when `expires_at - now < 300s`. Update file + registry.
- **Copilot**: Use cached `github_token` to call `api.github.com/copilot_internal/v2/token` for new session token. Check `expires_at` from Copilot response.

> **Codex MEDIUM finding addressed**: Copilot cache schema now includes `expires_at` for refresh threshold decisions.

### 1.5 Wire up in `main.rs`

- Build `ProviderRegistry` with `Arc<DashMap<ProviderId, ProviderCredentials>>` for proxy credentials (instead of `Option<HashMap>` — allows concurrent reads, serialized writes via `ProxyTokenManager`)
- Spawn `ProxyTokenManager` refresh tasks after startup
- Log relay instructions for unconfigured providers

### 1.6 Copilot GitHub token persistence

> **Codex MEDIUM finding addressed**: Document security tradeoff. Make opt-in via `COPILOT_PERSIST_GITHUB_TOKEN=true` env var.

The GitHub access token (`read:user` scope, never expires) is more sensitive than the short-lived Copilot session token. Persisting it enables automatic refresh but expands the attack surface of `/data/tokens.json`.

- Default: do NOT persist GitHub token (current behavior). Copilot sessions expire after ~30 min without refresh.
- Opt-in: Set `COPILOT_PERSIST_GITHUB_TOKEN=true` to enable persistence + background refresh.
- File permissions: `0o600` enforced on `/data/tokens.json`.

### 1.7 Update `ProviderRegistry::from_proxy_config()`

- Replace Anthropic API key credential with OAuth `access_token`
- Replace OpenAI API key credential with OAuth `access_token`
- Remove all Custom provider logic

## Wave 2: Docker Config & Entrypoint Cleanup (devops-engineer)

### 2.1 Update `entrypoint.sh`

- Update Copilot section to also export `COPILOT_GITHUB_TOKEN` and cache `github_token` + `expires_at` (when `COPILOT_PERSIST_GITHUB_TOKEN=true`)
- Remove Anthropic/OpenAI API key references from summary section (OAuth happens via relay after binary starts)
- Remove Custom provider summary line
- Add log message pointing users to relay endpoints for Anthropic/OpenAI

### 2.2 Update `docker-compose.gateway.yml` env vars

Remove:
- `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `OPENAI_BASE_URL`
- `CUSTOM_PROVIDER_URL`, `CUSTOM_PROVIDER_KEY`, `CUSTOM_PROVIDER_MODELS`

Add:
- `ANTHROPIC_ENABLED` (default false), `ANTHROPIC_OAUTH_CLIENT_ID`
- `OPENAI_ENABLED` (default false), `OPENAI_OAUTH_CLIENT_ID`
- `COPILOT_PERSIST_GITHUB_TOKEN` (default false)

Update header comments.

### 2.3 Clean up `docker-compose.yml` dead env vars

Remove unused `GITHUB_COPILOT_CLIENT_ID`, `GITHUB_COPILOT_CLIENT_SECRET`, `GITHUB_COPILOT_CALLBACK_URL` (lines 34-36).

### 2.4 Update `.env.example`

Add `ANTHROPIC_OAUTH_CLIENT_ID` and `OPENAI_OAUTH_CLIENT_ID` with placeholder descriptions.

## Wave 3: Tests & Verification (rust-backend-engineer)

> **Codex MEDIUM finding addressed**: Wave 3 test ownership reassigned to `rust-backend-engineer` per file ownership rules in `team-coordination.md`.

### 3.1 Update `from_proxy_config()` tests

- Remove all Custom provider tests
- Add tests for OAuth token credentials for Anthropic/OpenAI
- Test `ANTHROPIC_ENABLED=false` results in no credentials

### 3.2 Add `ProxyTokenManager` unit tests

- Test atomic file read/write with `0o600` permissions
- Test token expiry detection logic
- Test credential update via `MockExchanger`
- Test Copilot session refresh with GitHub token
- Test file lock serialization

### 3.3 Add proxy relay endpoint tests

- Test `/_proxy/providers/anthropic/connect` returns relay script URL
- Test relay callback stores tokens to file
- Test `/_proxy/providers/status` reflects connected state
- Test PROXY_API_KEY auth on relay endpoints

### 3.4 Remove Custom provider tests across all files

Ensure `cargo test --lib` passes with zero Custom references.

### 3.5 Verification commands

```bash
cd backend && cargo clippy --all-targets    # zero warnings
cd backend && cargo fmt --check             # no diffs
cd backend && cargo test --lib              # all pass
docker compose -f docker-compose.gateway.yml config --quiet  # valid compose
```

## Interface Contracts

### Proxy relay endpoints (new)

```
GET  /_proxy/providers/status                         → { providers: { anthropic: { connected: bool }, ... } }
POST /_proxy/providers/:provider/connect              → { relay_script_url: "..." }
GET  /_proxy/providers/:provider/relay-script?token=X → shell script (text/plain)
POST /_proxy/providers/:provider/relay                → receive tokens from script
```

Auth: `Authorization: Bearer <PROXY_API_KEY>` (connect/status) or relay token (relay-script/relay).

### /data/tokens.json schema (updated)

Existing Kiro/Copilot entries plus new provider entries:
- `anthropic`: `access_token`, `refresh_token`, `expires_at` (unix timestamp)
- `openai`: `access_token`, `refresh_token`, `expires_at` (unix timestamp)
- `copilot`: adds `github_token` (opt-in) and `expires_at`

## Verification

1. **Build**: `cd backend && cargo build`
2. **Lint**: `cd backend && cargo clippy --all-targets` — zero warnings
3. **Tests**: `cd backend && cargo test --lib` — all pass
4. **Docker**: `docker compose -f docker-compose.gateway.yml config --quiet` — valid
5. **Manual E2E**: Start gateway with `ANTHROPIC_ENABLED=true` + client ID, run the logged `curl | sh` command from host, verify tokens cached and refresh works

## Branch

`feat/gateway-oauth`

## Review Status

- Codex review: **passed with adjustments**
- HIGH findings addressed: 2/2
  - Manual code-paste → relay-script via Rust binary (no stdin needed)
  - Custom removal manifest expanded to all 15 files
- MEDIUM findings addressed: 4/4
  - ProxyTokenManager: atomic file writes + Mutex (not umask)
  - Copilot cache: added expires_at to schema
  - GitHub token: opt-in persistence with security documentation
  - Wave 3 ownership: reassigned to rust-backend-engineer
- LOW findings: 2 acknowledged (DashMap for proxy creds, shell PKCE details now moot since relay is in Rust)
- Disputed findings: 0
