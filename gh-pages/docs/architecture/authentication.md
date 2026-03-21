---
layout: default
title: Authentication
parent: Architecture
nav_order: 2
permalink: /architecture/authentication/
---

# Authentication System
{: .no_toc }

Kiro Gateway has three layers of authentication:

1. **Client authentication** — API keys for proxy endpoints (`/v1/*`), Google SSO or password+TOTP for the web UI (`/_ui/api/*`)
2. **Provider authentication** — Per-user credentials for each AI provider (Kiro, Anthropic, OpenAI Codex, Copilot, Qwen, Custom)
3. **Provider OAuth flows** — Web UI flows for connecting provider accounts (PKCE relay for Anthropic/OpenAI, GitHub OAuth for Copilot, device flow for Qwen)

The deployment mode determines which features are active:

- **Full Deployment** uses per-user API keys for proxy endpoints, Google SSO with PKCE for the web UI, per-user Kiro credentials stored in PostgreSQL, and multi-provider OAuth for connecting additional AI providers.
- **Proxy-Only Mode** uses a single `PROXY_API_KEY` for all requests and a single set of Kiro credentials obtained via an AWS SSO device code flow on first boot.

## Table of Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Proxy-Only Mode Authentication

In Proxy-Only Mode (`docker-compose.gateway.yml`), authentication is simplified to a single API key and a single set of Kiro credentials.

### PROXY_API_KEY Validation

All requests to `/v1/*` endpoints must include the `PROXY_API_KEY` value:

```bash
# Via Authorization header
curl -H "Authorization: Bearer YOUR_PROXY_API_KEY" http://localhost:8000/v1/models

# Via x-api-key header
curl -H "x-api-key: YOUR_PROXY_API_KEY" http://localhost:8000/v1/models
```

The key is set via the `PROXY_API_KEY` environment variable. There is no per-user key management, no database lookup, and no web UI authentication.

### Device Code Flow (First Boot)

On first boot, the `backend/entrypoint.sh` script runs an AWS SSO OIDC device code flow to obtain Kiro credentials:

```mermaid
sequenceDiagram
    participant Container as Gateway Container
    participant OIDC as AWS SSO OIDC
    participant User as User (Browser)

    Container->>OIDC: Register OIDC client
    OIDC-->>Container: client_id, client_secret

    Container->>OIDC: Start device authorization
    OIDC-->>Container: device_code, user_code, verification_url

    Note over Container: Prints URL + user code to logs
    Container-->>User: "Open this URL in your browser"

    User->>OIDC: Open URL, enter user code, authorize
    OIDC-->>User: Authorization granted

    loop Poll for token (every N seconds)
        Container->>OIDC: POST /token (device_code grant)
        OIDC-->>Container: authorization_pending / access_token
    end

    Container->>Container: Save refresh_token to /data/tokens.json
    Note over Container: Start gateway binary
```

The flow supports two SSO modes:

- **Builder ID (free):** Default when `KIRO_SSO_URL` is not set. Uses `https://view.awsapps.com/start` as the start URL.
- **Identity Center (pro):** Set `KIRO_SSO_URL` to your organization's SSO URL. The device flow uses your Identity Center for authorization.

### Credential Caching

Credentials are cached to `/data/tokens.json` inside the `gateway-data` Docker volume:

```json
{"refresh_token":"...","client_id":"...","client_secret":"..."}
```

On subsequent restarts:
1. The entrypoint loads cached credentials from the volume
2. Validates them with a test token refresh
3. If valid, starts the gateway immediately (no user interaction needed)
4. If expired or invalid, clears the cache and re-runs the device code flow

To force re-authorization, remove the Docker volume:

```bash
docker volume rm harbangan_gateway-data
```

---

## Full Deployment Authentication

### Authentication Architecture Overview

```mermaid
flowchart TB
    subgraph ProxyAuth["API Key Authentication (/v1/*)"]
        CLIENT["AI Client<br/>(Cursor, Claude Code, etc.)"]
        MW["Auth Middleware"]
        CLIENT -->|"Authorization: Bearer {api-key}<br/>or x-api-key: {api-key}"| MW
        MW -->|"SHA-256 hash → cache/DB lookup"| LOOKUP["Identify user + key"]
        LOOKUP -->|Valid| INJECT["Inject user identity"]
        LOOKUP -->|Invalid| REJECT["401 Unauthorized"]
        INJECT --> HANDLER["Route Handler"]
    end

    subgraph WebAuth["Google SSO Authentication (/_ui/api/*)"]
        BROWSER["Web Browser"]
        SESSION["Session Middleware"]
        BROWSER -->|"Cookie: kgw_session={uuid}"| SESSION
        SESSION -->|"Lookup in session_cache"| SESSION_CHECK{"Valid session?"}
        SESSION_CHECK -->|Yes| WEBHANDLER["Web UI Handler"]
        SESSION_CHECK -->|No| LOGIN["Redirect to Google SSO"]
    end

    subgraph ProviderAuth["Provider Resolution (per-request)"]
        HANDLER --> REGISTRY["ProviderRegistry"]
        REGISTRY -->|"resolve_provider(user_id, model)"| PROV_CACHE{"Credential cache<br/>(5-min TTL)?"}
        PROV_CACHE -->|Hit| SELECT["Select by priority"]
        PROV_CACHE -->|Miss| LOAD_CREDS["Load from DB +<br/>refresh if expiring"]
        LOAD_CREDS --> SELECT
        SELECT --> KIRO_PATH["Kiro: AWS SSO OIDC"]
        SELECT --> DIRECT_PATH["Direct: Anthropic/OpenAI Codex/<br/>Copilot/Qwen/Custom"]
    end

    subgraph BackendAuth["Kiro Token Management"]
        KIRO_PATH --> AUTHMGR["AuthManager"]
        AUTHMGR -->|"get per-user token"| TOKEN_CACHE{"Token in cache<br/>(4-min TTL)?"}
        TOKEN_CACHE -->|Yes| USE_TOKEN["Use cached token"]
        TOKEN_CACHE -->|No| REFRESH["Refresh via AWS SSO OIDC"]
        REFRESH --> OIDC["oidc.{region}.amazonaws.com"]
        OIDC --> UPDATE["Update token cache"]
        UPDATE --> USE_TOKEN
        USE_TOKEN --> KIRO["Kiro API<br/>(Bearer token)"]
    end

    subgraph DirectAuth["Direct Provider Auth"]
        DIRECT_PATH --> PROVIDER_API["Provider API<br/>(Bearer token / API key)"]
    end

    subgraph Storage["Credential Storage"]
        PG[("PostgreSQL")]
        AUTHMGR -.->|"Load per-user Kiro credentials"| PG
        REGISTRY -.->|"Load provider tokens<br/>(user_provider_tokens table)"| PG
        WEBHANDLER -->|"Manage users, API keys,<br/>provider connections"| PG
    end
```

---

## API Key Authentication (Proxy Endpoints)

The auth middleware (`backend/src/middleware/mod.rs`) protects all `/v1/*` proxy routes using per-user API keys.

### How It Works

1. Client sends a request with an API key via `Authorization: Bearer {key}` or `x-api-key: {key}` header
2. Middleware SHA-256 hashes the key
3. Hash is looked up in `api_key_cache` (in-memory DashMap) for fast path
4. On cache miss, hash is looked up in PostgreSQL
5. If found, the user ID and key ID are extracted and per-user Kiro credentials are injected into the request context
6. If not found, a `401 Unauthorized` JSON error is returned

### Per-User API Keys

Each user can create multiple API keys through the web UI. Keys are:
- Generated as random strings and shown to the user once at creation time
- Stored as SHA-256 hashes in PostgreSQL (the plaintext key is never stored)
- Cached in `api_key_cache: Arc<DashMap<String, (Uuid, Uuid)>>` mapping hash to `(user_id, key_id)`
- Individually revocable without affecting other keys

### Routes That Bypass API Key Auth

- `GET /` — Status JSON (for load balancers)
- `GET /health` — Health check
- `/_ui/api/*` — Web UI API routes (protected by session auth instead)

---

## Google SSO Authentication (Web UI)

The web UI uses Google SSO with PKCE + OpenID Connect for user authentication. This is implemented in `backend/src/web_ui/google_auth.rs`.

### OAuth Flow

```mermaid
sequenceDiagram
    participant User
    participant Browser
    participant Backend as Backend API
    participant Google as Google OAuth

    User->>Browser: Navigate to /_ui/
    Browser->>Backend: GET /_ui/ (via Vite proxy)

    User->>Browser: Click "Sign in with Google"
    Browser->>Backend: GET /_ui/api/auth/google

    Backend->>Backend: Generate PKCE code_verifier + code_challenge
    Backend->>Backend: Generate state parameter
    Backend->>Backend: Store {code_verifier, state} in oauth_pending (10-min TTL)
    Backend-->>Browser: 302 Redirect to Google

    Browser->>Google: Authorization request with code_challenge
    User->>Google: Consent + authorize
    Google-->>Browser: 302 Redirect to callback with code + state

    Browser->>Backend: GET /_ui/api/auth/google/callback?code=...&state=...
    Backend->>Backend: Verify state matches oauth_pending entry
    Backend->>Google: Exchange code + code_verifier for tokens
    Google-->>Backend: {id_token, access_token}

    Backend->>Backend: Verify id_token (email, email_verified)
    Backend->>Backend: Create/update user in PostgreSQL
    Backend->>Backend: Create session in session_cache (24h TTL)
    Backend-->>Browser: Set-Cookie: kgw_session={uuid} + CSRF cookie
    Browser-->>User: Redirected to dashboard
```

### Session Management

Sessions are managed by `backend/src/web_ui/session.rs`:

- **Session cookie**: `kgw_session` — HttpOnly, Secure, SameSite=Strict, 24-hour TTL
- **CSRF cookie**: Separate cookie for CSRF token validation on mutation requests
- **Session storage**: `session_cache: Arc<DashMap<Uuid, SessionInfo>>` — in-memory, backed by PostgreSQL for persistence across restarts
- **SessionInfo** contains: user ID, email, role (Admin/User), expiry timestamp, auth method, TOTP status, must-change-password flag
- **Sliding expiry**: Sessions automatically extend when more than 12 hours have passed since creation

### CSRF Protection

All mutation endpoints (POST, PUT, DELETE) under `/_ui/api/*` require a valid CSRF token:
- The CSRF token is set as a cookie when the session is created
- Clients must include the token in a request header for mutations
- This prevents cross-site request forgery attacks against the web UI

### Roles

| Role | Capabilities |
|------|-------------|
| Admin | Full access: manage users, update config, manage domain allowlist, manage guardrail profiles/rules, all user capabilities |
| User | Manage own API keys, manage own provider credentials, view usage |

The first user to sign in (via Google SSO or password auth) is automatically assigned the Admin role.

### Login Rate Limiting

Password login attempts are rate-limited per email address to prevent brute-force attacks:

- **Limit**: 5 failed attempts within a 15-minute window
- **Response**: `423 Account Locked` with `retry_after_secs` field
- **Tracking**: In-memory `DashMap<String, (u32, Instant)>` keyed by email
- **Reset**: Counter resets after 15 minutes of no failed attempts

### Admin-Only Feature Routes

The following feature admin routes follow the same session + CSRF pattern as other Web UI mutation endpoints:

- **Guardrails** (`/_ui/api/guardrails/*`) — CRUD for guardrail profiles and rules, test endpoint, CEL validation
- **User management** (`/_ui/api/users/*`) — List users, update roles, delete users
- **Admin user creation** (`POST /_ui/api/admin/users/create`) — Create users with password auth
- **Password reset** (`POST /_ui/api/admin/users/:id/reset-password`) — Reset user password (forces `must_change_password`)
- **Usage** (`/_ui/api/admin/usage/*`) — Global usage stats and per-user breakdown
- **Config** (`PUT /_ui/api/config`) — Update runtime configuration

---

## Password + TOTP 2FA Authentication

Implemented in `backend/src/web_ui/password_auth.rs`. This is an alternative to Google SSO for environments where Google OAuth is not available.

### Login Flow

1. `POST /_ui/api/auth/login` with `{email, password}` — Argon2 password verification
2. If TOTP is enabled: returns `{needs_2fa: true, login_token: uuid}` — pending 2FA token stored in DB (5-minute TTL)
3. `POST /_ui/api/auth/login/2fa` with `{login_token, code}` — verifies TOTP code (30s window, 1 skew tolerance) or recovery code
4. On success: creates session, sets `kgw_session` + CSRF cookies

### 2FA Setup

All password users must enable TOTP 2FA. The `SessionGate` forces redirect to `/_ui/setup-2fa` for password users without TOTP enabled.

1. `GET /_ui/api/auth/2fa/setup` — generates TOTP secret + QR URI (otpauth:// format)
2. `POST /_ui/api/auth/2fa/verify` — verifies a TOTP code and enables 2FA
3. On success: generates 8 single-use recovery codes (SHA-256 hashed in DB)

### Recovery Codes

- 8 alphanumeric codes generated on 2FA setup
- SHA-256 hashed and stored in `totp_recovery_codes` table
- Single-use: marked as `used` after successful verification
- Can be used instead of TOTP code during login

---

## Backend Authentication (Kiro API)

Each user has their own Kiro credentials (refresh token, client ID, client secret) stored in PostgreSQL. The `AuthManager` (`backend/src/auth/manager.rs`) handles per-user token lifecycle.

### Per-User Token Flow

```mermaid
flowchart TD
    REQ["Incoming API request"] --> IDENTIFY["Identify user via API key"]
    IDENTIFY --> CACHE_CHECK{"Per-user token<br/>in kiro_token_cache?"}

    CACHE_CHECK -->|"Yes (< 4 min old)"| USE["Use cached access token"]
    CACHE_CHECK -->|No| LOAD["Load user's Kiro credentials from DB"]

    LOAD --> REFRESH["Refresh via AWS SSO OIDC"]
    REFRESH --> OIDC["POST to oidc.{region}.amazonaws.com/token"]
    OIDC --> RESULT{Success?}

    RESULT -->|Yes| CACHE_UPDATE["Store in kiro_token_cache<br/>(4-min TTL)"]
    CACHE_UPDATE --> USE

    RESULT -->|No| DEGRADE{"Token actually<br/>expired?"}
    DEGRADE -->|No| WARN["Log warning,<br/>use existing token"]
    WARN --> USE
    DEGRADE -->|Yes| FAIL["Return error:<br/>no valid token"]

    USE --> KIRO["POST to Kiro API<br/>with Bearer token"]
```

The `kiro_token_cache: Arc<DashMap<Uuid, (String, String, Instant)>>` maps user IDs to `(access_token, region, cached_at)` tuples. Tokens are refreshed when older than 4 minutes.

### Kiro Credential Setup

Users configure their Kiro credentials through the web UI. The credentials are stored in PostgreSQL per-user:

| Field | Description |
|-------|-------------|
| `kiro_refresh_token` | OAuth refresh token for Kiro API |
| `kiro_region` | AWS region for API calls (e.g., `us-east-1`) |
| `oauth_client_id` | OAuth client ID from AWS SSO OIDC registration |
| `oauth_client_secret` | OAuth client secret from registration |
| `oauth_sso_region` | AWS region for the SSO OIDC endpoint |

### Token Refresh Mechanism

The token refresh uses AWS SSO OIDC (`backend/src/auth/refresh.rs`):

1. **Proactive refresh**: Tokens are refreshed before they expire. The `kiro_token_cache` 4-minute TTL ensures tokens are refreshed well within typical token lifetimes.

2. **Graceful degradation**: If refresh fails but the token hasn't actually expired yet, the gateway continues using the existing token and logs a warning.

3. **Per-user isolation**: Each user's token refresh is independent. A refresh failure for one user does not affect others.

4. **HTTP client retry**: The `KiroHttpClient` can independently refresh tokens on 403 responses and retry the request.

### The Refresh Request

The OIDC refresh (`backend/src/auth/refresh.rs:refresh_aws_sso_oidc()`) sends a JSON POST to `https://oidc.{sso_region}.amazonaws.com/token`:

```json
{
  "grantType": "refresh_token",
  "clientId": "...",
  "clientSecret": "...",
  "refreshToken": "..."
}
```

The SSO region may differ from the API region (e.g., SSO in `us-east-1` but API in `eu-west-1`). The response provides a new `access_token` and optionally a rotated `refresh_token`.

---

## Multi-Provider Authentication

Beyond the default Kiro provider, users can connect additional AI providers through the web UI. Each provider has its own OAuth flow and credential storage. Provider tokens are stored in the `user_provider_tokens` PostgreSQL table and cached in the `ProviderRegistry` with a 5-minute TTL.

### Provider Credential Storage

All provider tokens are stored per-user in PostgreSQL:

| Column | Description |
|--------|-------------|
| `user_id` | Foreign key to users table |
| `provider` | Provider identifier (`anthropic`, `openai`, `gemini`, `copilot`, `qwen`) |
| `access_token` | Current access token (encrypted at rest) |
| `refresh_token` | Refresh token for OAuth providers |
| `expires_at` | Token expiry timestamp |
| `base_url` | Optional API endpoint override |
| `priority` | Provider priority (lower = preferred) |
| `metadata` | Provider-specific metadata (JSON) |

### GitHub Copilot OAuth Flow

Copilot authentication uses a two-step process: GitHub OAuth for user authorization, then a Copilot-specific token exchange. Implemented in `backend/src/web_ui/copilot_auth.rs`.

```mermaid
sequenceDiagram
    participant User
    participant Browser
    participant Backend as Backend API
    participant GitHub as GitHub OAuth
    participant Copilot as Copilot Token API

    User->>Browser: Click "Connect Copilot"
    Browser->>Backend: GET /_ui/api/providers/copilot/auth

    Backend->>Backend: Generate state parameter
    Backend->>Backend: Store state in oauth_pending
    Backend-->>Browser: 302 Redirect to GitHub

    Browser->>GitHub: Authorization request (scope: read:user)
    User->>GitHub: Authorize application
    GitHub-->>Browser: 302 Redirect with code + state

    Browser->>Backend: GET /_ui/api/providers/copilot/callback?code=...&state=...
    Backend->>Backend: Verify state matches oauth_pending
    Backend->>GitHub: Exchange code for GitHub access_token
    GitHub-->>Backend: {access_token}

    Backend->>GitHub: GET /user (verify GitHub identity)
    GitHub-->>Backend: {login: "username"}

    Backend->>Copilot: GET /copilot_internal/v2/token
    Note over Backend,Copilot: Headers: Authorization, Editor-Version,<br/>Editor-Plugin-Version, Copilot-Integration-Id
    Copilot-->>Backend: {token, expires_at, endpoints.api}

    Backend->>Backend: Store Copilot token + base_url in DB
    Backend->>Backend: Cache in copilot_token_cache
    Backend-->>Browser: Redirect to /_ui/ (success)
```

The Copilot token includes a `base_url` from the `endpoints.api` field, which may vary (e.g., `https://api.githubcopilot.com` vs `https://api.business.githubcopilot.com` for enterprise). Tokens are cached in `copilot_token_cache: Arc<DashMap<Uuid, (String, String, Instant)>>` mapping user IDs to `(token, base_url, cached_at)`.

### Qwen Coder Device Flow

Qwen uses RFC 8628 (OAuth Device Authorization Grant) — the user authorizes on a separate device/browser. Implemented in `backend/src/web_ui/qwen_auth.rs`.

```mermaid
sequenceDiagram
    participant User
    participant Browser
    participant Backend as Backend API
    participant Qwen as Qwen OAuth API

    User->>Browser: Click "Connect Qwen"
    Browser->>Backend: POST /_ui/api/providers/qwen/device-code

    Backend->>Backend: Generate PKCE code_verifier + code_challenge
    Backend->>Qwen: POST /api/v1/oauth2/device/code
    Note over Backend,Qwen: {client_id, code_challenge, code_challenge_method}
    Qwen-->>Backend: {device_code, user_code, verification_uri, interval}

    Backend->>Backend: Store {device_code, code_verifier, user_id} in qwen_device_pending
    Backend-->>Browser: {device_code, user_code, verification_uri, interval}

    Note over Browser: Display user_code and verification_uri
    User->>Qwen: Open verification_uri, enter user_code, authorize

    loop Poll for token (every interval seconds)
        Browser->>Backend: GET /_ui/api/providers/qwen/device-poll?device_code=...
        Backend->>Qwen: POST /api/v1/oauth2/token
        Note over Backend,Qwen: {client_id, device_code, code_verifier,<br/>grant_type: urn:ietf:params:oauth:grant-type:device_code}
        alt Authorization pending
            Qwen-->>Backend: {error: "authorization_pending"}
            Backend-->>Browser: {status: "pending"}
        else Authorized
            Qwen-->>Backend: {access_token, refresh_token, expires_in}
            Backend->>Backend: Store tokens in user_provider_tokens
            Backend->>Backend: Remove from qwen_device_pending
            Backend-->>Browser: {status: "complete"}
        else Expired / Denied
            Qwen-->>Backend: {error: "expired_token" | "access_denied"}
            Backend-->>Browser: {status: "error", message: "..."}
        end
    end
```

Pending device flow states are stored in `qwen_device_pending: Arc<DashMap<String, QwenDevicePending>>` with a 10-minute TTL and 10k capacity cap.

### Anthropic OAuth Relay (PKCE)

Anthropic uses a standard OAuth 2.0 authorization code flow with PKCE. The gateway acts as a relay, redirecting the user to `claude.ai/oauth/authorize` and exchanging the code for tokens. Implemented in `backend/src/web_ui/provider_oauth.rs`.

```mermaid
sequenceDiagram
    participant User
    participant Browser
    participant Backend as Backend API
    participant Anthropic as Anthropic OAuth

    User->>Browser: Click "Connect Anthropic"
    Browser->>Backend: GET /_ui/api/providers/anthropic/auth

    Backend->>Backend: Generate PKCE code_verifier + code_challenge
    Backend->>Backend: Generate state parameter
    Backend->>Backend: Store {pkce_verifier, state, user_id} in provider_oauth_pending
    Backend-->>Browser: 302 Redirect to claude.ai/oauth/authorize
    Note over Browser,Anthropic: Params: client_id, code_challenge,<br/>redirect_uri (localhost:54545), scopes

    Browser->>Anthropic: Authorization request
    User->>Anthropic: Consent + authorize
    Anthropic-->>Browser: Redirect to localhost callback with code + state

    Browser->>Backend: Callback with code + state
    Backend->>Backend: Verify state matches provider_oauth_pending
    Backend->>Anthropic: POST /v1/oauth/token (code + code_verifier)
    Anthropic-->>Backend: {access_token, refresh_token, expires_in}

    Backend->>Backend: Store tokens in user_provider_tokens
    Backend-->>Browser: Success response
```

The `TokenExchanger` trait abstracts the token exchange, making it mockable for tests. Provider OAuth pending states are stored separately from Google SSO in `provider_oauth_pending: Arc<DashMap<String, ProviderOAuthPendingState>>`.

### OpenAI and Gemini (API Key Storage)

OpenAI and Gemini use simple API key authentication — no OAuth flow required. Users enter their API key in the web UI, and it's stored directly in the `user_provider_tokens` table. The key is used as-is in the `Authorization: Bearer {key}` header when making requests to the provider API.

---

## Setup-Only Mode

On first run (no admin user in DB), the gateway enters setup-only mode:

1. `setup_complete` `AtomicBool` is set to `false`
2. All `/v1/*` proxy endpoints return `503 Service Unavailable`
3. Only the web UI and health endpoints are accessible
4. The first user to complete Google SSO is assigned the Admin role
5. `setup_complete` transitions to `true` and the gateway begins serving proxy requests

This ensures the gateway cannot be used as an open proxy before authentication is configured.

---

## Auth Module Structure

```mermaid
flowchart LR
    subgraph "backend/src/auth/"
        MOD["mod.rs<br/><i>Exports: AuthManager, PollResult</i>"]
        MGR["manager.rs<br/><i>AuthManager struct</i>"]
        CREDS["credentials.rs<br/><i>Load from ConfigDb</i>"]
        REFRESH["refresh.rs<br/><i>AWS SSO OIDC refresh</i>"]
        OAUTH["oauth.rs<br/><i>Client registration,<br/>device flow, PKCE</i>"]
        TYPES["types.rs<br/><i>Credentials, TokenData,<br/>AuthType, PollResult</i>"]
    end

    MOD --> MGR
    MOD --> TYPES
    MGR --> CREDS
    MGR --> REFRESH
    MGR --> TYPES
    CREDS --> TYPES
    REFRESH --> TYPES
    OAUTH --> TYPES
```

---

## Web UI Auth Module Structure

```mermaid
flowchart LR
    subgraph "backend/src/web_ui/"
        GOOGLE["google_auth.rs<br/><i>Google SSO + PKCE</i>"]
        SESSION["session.rs<br/><i>Cookie sessions + CSRF</i>"]
        APIKEYS["api_keys.rs<br/><i>Per-user API key CRUD</i>"]
        USERKIRO["user_kiro.rs<br/><i>Per-user Kiro token mgmt</i>"]
        USERS["users.rs<br/><i>User admin (admin-only)</i>"]
        COPILOT["copilot_auth.rs<br/><i>GitHub OAuth → Copilot token</i>"]
        QWEN["qwen_auth.rs<br/><i>Qwen device flow (RFC 8628)</i>"]
        PROVIDER_OAUTH["provider_oauth.rs<br/><i>Anthropic PKCE relay,<br/>TokenExchanger trait</i>"]
    end

    GOOGLE --> SESSION
    SESSION --> APIKEYS
    SESSION --> USERKIRO
    SESSION --> USERS
    SESSION --> COPILOT
    SESSION --> QWEN
    SESSION --> PROVIDER_OAUTH
```

---

## How Auth Integrates with the Request Flow

The authentication system touches the request flow at three points:

1. **Middleware layer** — The `auth_middleware` SHA-256 hashes the client's API key and looks up the user in cache/DB. If valid, it injects the user's identity into the request extensions. This is a fast hash + DashMap lookup, not an OAuth flow.

2. **Provider resolution** — The `ProviderRegistry` resolves which provider to use for the request based on the user's configured credentials and priority. It checks its 5-minute credential cache first, then loads from PostgreSQL on cache miss. For OAuth-based providers (Copilot, Qwen, Anthropic), it proactively refreshes tokens nearing expiry using per-provider mutexes to prevent refresh storms.

3. **Handler layer** — For Kiro-bound requests, the handler retrieves the per-user Kiro access token (from cache or via refresh). For direct providers, the `Provider` trait implementation uses the credentials from the registry to call the provider API directly.

The `KiroHttpClient` also holds its own `Arc<AuthManager>` reference for connection-level retry logic. When a request to the Kiro API returns 403, the HTTP client can independently refresh the token and retry without involving the route handler.
