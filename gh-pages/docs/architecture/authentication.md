---
layout: default
title: Authentication
parent: Architecture
nav_order: 2
permalink: /architecture/authentication/
---

# Authentication System
{: .no_toc }

Kiro Gateway uses a two-layer authentication model: a client-facing API key (`PROXY_API_KEY`) protects the gateway itself, while an OAuth device code flow via AWS SSO OIDC authenticates the gateway against the Kiro backend. This page covers both layers in detail.

## Table of Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Authentication Architecture Overview

```mermaid
flowchart TB
    subgraph ClientAuth["Layer 1: Client Authentication"]
        CLIENT["AI Client<br/>(Cursor, Claude Code, etc.)"]
        MW["Auth Middleware"]
        CLIENT -->|"Authorization: Bearer {PROXY_API_KEY}<br/>or x-api-key: {PROXY_API_KEY}"| MW
        MW -->|Valid| HANDLER["Route Handler"]
        MW -->|Invalid| REJECT["401 Unauthorized"]
    end

    subgraph BackendAuth["Layer 2: Backend Authentication"]
        HANDLER --> AUTHMGR["AuthManager"]
        AUTHMGR -->|"get_access_token()"| TOKEN_CHECK{"Token<br/>expiring soon?"}
        TOKEN_CHECK -->|No| USE_TOKEN["Use cached token"]
        TOKEN_CHECK -->|Yes| REFRESH["Refresh via AWS SSO OIDC"]
        REFRESH --> OIDC["oidc.{region}.amazonaws.com"]
        OIDC --> UPDATE["Update cached token"]
        UPDATE --> USE_TOKEN
        USE_TOKEN --> KIRO["Kiro API<br/>(Bearer token)"]
    end

    subgraph Storage["Credential Storage"]
        PG[("PostgreSQL")]
        AUTHMGR -.->|"Load credentials"| PG
        WEBUI["Web UI<br/>(Device Code Flow)"] -->|"Store tokens"| PG
    end
```

---

## Layer 1: Client-Facing Authentication

The auth middleware (`src/middleware/mod.rs:auth_middleware()`) protects all API routes. It accepts two authentication methods:

### Bearer Token
```
Authorization: Bearer {PROXY_API_KEY}
```

### API Key Header
```
x-api-key: {PROXY_API_KEY}
```

The middleware checks both headers in order. If neither matches the configured `PROXY_API_KEY`, a `401 Unauthorized` response is returned with a JSON error body.

Routes that bypass authentication:
- `GET /` — Simple health check (for load balancers)
- `GET /health` — Detailed health check
- `/_ui/*` — Web UI routes (protected by their own session logic)

The `PROXY_API_KEY` is read from the shared `Config` via `RwLock`, which means it can be changed at runtime through the Web UI without restarting the gateway.

---

## Layer 2: Backend Authentication (AWS SSO OIDC)

The gateway authenticates against the Kiro API using OAuth 2.0 tokens obtained through the AWS SSO OIDC device code flow. The `AuthManager` (`src/auth/manager.rs`) manages the complete token lifecycle.

### OAuth Device Code Flow

The initial authentication is performed through the Web UI. The user triggers a device code flow that registers an OAuth client, obtains a device code, and polls for authorization.

```mermaid
sequenceDiagram
    participant User
    participant WebUI as Web UI (Browser)
    participant Gateway as Kiro Gateway
    participant OIDC as AWS SSO OIDC
    participant AWS as AWS Login Page

    User->>WebUI: Click "Login with AWS"
    WebUI->>Gateway: POST /_ui/api/auth/start

    Gateway->>OIDC: POST /client/register
    Note right of Gateway: clientName: "kiro-gateway"<br/>clientType: "public"<br/>grantTypes: ["device_code", "refresh_token"]<br/>scopes: ["codewhisperer:*"]
    OIDC-->>Gateway: {client_id, client_secret}

    Gateway->>OIDC: POST /device_authorization
    Note right of Gateway: clientId, clientSecret,<br/>startUrl (optional)
    OIDC-->>Gateway: {device_code, user_code,<br/>verification_uri_complete}

    Gateway-->>WebUI: {user_code, verification_uri_complete}
    WebUI->>User: "Enter code: ABCD-EFGH"
    User->>AWS: Open verification URL
    User->>AWS: Confirm authorization

    loop Poll every {interval} seconds
        Gateway->>OIDC: POST /token (device_code grant)
        alt authorization_pending
            OIDC-->>Gateway: Pending
        else slow_down
            OIDC-->>Gateway: Slow down (increase interval)
        else Success
            OIDC-->>Gateway: {access_token, refresh_token}
        end
    end

    Gateway->>Gateway: Store credentials in PostgreSQL
    Note right of Gateway: oauth_client_id<br/>oauth_client_secret<br/>kiro_refresh_token<br/>oauth_sso_region

    Gateway-->>WebUI: Authentication complete
    WebUI-->>User: "Login successful"
```

The OAuth module (`src/auth/oauth.rs`) implements all the OIDC protocol operations:

| Function | Purpose |
|----------|---------|
| `register_client()` | Register OAuth client with AWS SSO OIDC |
| `generate_pkce()` | Generate PKCE code verifier and challenge (for browser flow) |
| `build_authorize_url()` | Build authorization URL (for browser flow) |
| `start_device_authorization()` | Initiate device code flow |
| `poll_device_token()` | Poll for device authorization completion |
| `exchange_authorization_code()` | Exchange auth code for tokens (browser flow) |

The gateway supports two OAuth flows:
- **Device code flow** (primary) — Used for headless/CLI setups. The user authorizes on a separate device.
- **Browser redirect flow** — Uses PKCE (S256) for the authorization code exchange via browser redirect.

### Required OAuth Scopes

```
codewhisperer:completions
codewhisperer:analysis
codewhisperer:conversations
```

---

## AuthManager Architecture

The `AuthManager` struct (`src/auth/manager.rs`) is the central token management component. It provides thread-safe access to credentials and handles automatic token refresh.

```mermaid
classDiagram
    class AuthManager {
        -Arc~RwLock~Credentials~~ credentials
        -Arc~RwLock~Option~String~~~ access_token
        -Arc~RwLock~Option~DateTime~~~ expires_at
        -Client client
        -Option~Arc~ConfigDb~~ config_db
        -i64 refresh_threshold
        +new(config_db, threshold) Result~Self~
        +new_placeholder(region, threshold) Result~Self~
        +get_access_token() Result~String~
        +get_region() String
        +get_profile_arn() Option~String~
        -is_token_expiring_soon() bool
        -is_token_expired() bool
        -refresh_token() Result~()~
    }

    class Credentials {
        +String refresh_token
        +Option~String~ access_token
        +Option~DateTime~ expires_at
        +Option~String~ profile_arn
        +String region
        +Option~String~ client_id
        +Option~String~ client_secret
        +Option~String~ sso_region
    }

    class TokenData {
        +String access_token
        +Option~String~ refresh_token
        +DateTime expires_at
        +Option~String~ profile_arn
    }

    AuthManager --> Credentials : manages
    AuthManager ..> TokenData : receives from refresh
```

### Token Refresh Mechanism

The token refresh flow is triggered automatically when `get_access_token()` detects the token is expiring within the `refresh_threshold` (default: 300 seconds / 5 minutes).

```mermaid
flowchart TD
    START["get_access_token()"] --> CHECK{"Token expiring<br/>within threshold?"}

    CHECK -->|No| RETURN["Return cached token"]

    CHECK -->|Yes| REFRESH["refresh_token()"]
    REFRESH --> OIDC_CALL["POST to AWS SSO OIDC<br/>/token endpoint"]
    OIDC_CALL --> OIDC_RESULT{Success?}

    OIDC_RESULT -->|Yes| UPDATE["Update access_token,<br/>expires_at, refresh_token"]
    UPDATE --> RETURN

    OIDC_RESULT -->|No, 400 error| RELOAD{"Config DB<br/>available?"}
    RELOAD -->|Yes| RELOAD_CREDS["Reload credentials<br/>from PostgreSQL"]
    RELOAD_CREDS --> RETRY["Retry OIDC refresh<br/>with fresh credentials"]
    RETRY --> RETRY_RESULT{Success?}
    RETRY_RESULT -->|Yes| UPDATE
    RETRY_RESULT -->|No| DEGRADE

    RELOAD -->|No| DEGRADE

    OIDC_RESULT -->|No, other error| DEGRADE{"Token actually<br/>expired?"}
    DEGRADE -->|No| WARN["Log warning,<br/>use existing token"]
    WARN --> RETURN
    DEGRADE -->|Yes| FAIL["Return error:<br/>no valid token"]
```

Key behaviors:

1. **Proactive refresh**: Tokens are refreshed before they expire, not after. The 5-minute threshold ensures there's always a valid token available.

2. **Credential reload on 400**: If the OIDC endpoint returns a 400 error (typically meaning the refresh token was rotated externally), the AuthManager reloads credentials from PostgreSQL and retries. This handles the case where the Web UI re-authenticated while the gateway was running.

3. **Graceful degradation**: If refresh fails but the token hasn't actually expired yet, the gateway continues using the existing token and logs a warning. This prevents transient OIDC outages from causing immediate failures.

4. **Thread safety**: All token state is behind `tokio::sync::RwLock`, allowing concurrent reads from multiple request handlers while serializing refresh operations.

### The Refresh Request

The actual OIDC refresh (`src/auth/refresh.rs:refresh_aws_sso_oidc()`) sends a JSON POST to `https://oidc.{sso_region}.amazonaws.com/token`:

```json
{
  "grantType": "refresh_token",
  "clientId": "...",
  "clientSecret": "...",
  "refreshToken": "..."
}
```

The SSO region may differ from the API region (e.g., SSO in `us-east-1` but API in `eu-west-1`). The response provides a new `access_token` and optionally a rotated `refresh_token`. Token expiration is calculated as `expires_in - 60 seconds` (a 60-second safety buffer).

---

## Credential Storage in PostgreSQL

Credentials are stored in the gateway's PostgreSQL config database (`web_ui::config_db::ConfigDb`). The credential loader (`src/auth/credentials.rs:load_from_config_db()`) reads:

| Config Key | Description | Required |
|-----------|-------------|----------|
| `kiro_refresh_token` | OAuth refresh token | Yes |
| `kiro_region` | AWS region for API calls | No (default: `us-east-1`) |
| `oauth_client_id` | OAuth client ID from registration | Yes |
| `oauth_client_secret` | OAuth client secret from registration | Yes |
| `oauth_sso_region` | AWS region for SSO OIDC endpoint | No (defaults to `kiro_region`) |

If `oauth_client_id` or `oauth_client_secret` is missing, the credential loader returns an error directing the user to complete the device code login via the Web UI.

---

## Auth Module Structure

```mermaid
flowchart LR
    subgraph "src/auth/"
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

## How Auth Integrates with the Request Flow

The authentication system touches the request flow at two points:

1. **Middleware layer** — The `auth_middleware` validates the client's `PROXY_API_KEY` before the request reaches any handler. This is a simple string comparison, not an OAuth flow.

2. **Handler layer** — Inside `chat_completions_handler` and `anthropic_messages_handler`, the handler calls `auth_manager.get_access_token()` to obtain a valid Kiro API token. This may trigger a background refresh if the token is expiring soon.

The `KiroHttpClient` also holds its own `Arc<AuthManager>` reference for connection-level retry logic. When a request to the Kiro API returns 403, the HTTP client can independently refresh the token and retry without involving the route handler.

Two separate `AuthManager` instances exist at runtime:
- One owned by `KiroHttpClient` (wrapped in `Arc<AuthManager>`) for retry-level token refresh
- One in `AppState` (wrapped in `Arc<tokio::sync::RwLock<AuthManager>>`) that can be swapped entirely when the user re-authenticates through the Web UI
