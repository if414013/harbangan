---
layout: default
title: Web Dashboard
nav_order: 10
---

# Web Dashboard

Harbangan includes a web dashboard served as a React single-page application (SPA) at `/_ui/`. It provides a browser-based interface for initial setup, user management, provider connections, configuration, usage tracking, and content guardrails.

---

## Overview

The frontend is a React 19 SPA built with Vite and TypeScript. In development, the Vite dev server serves the SPA and proxies API requests to the Rust backend. Authentication supports Google SSO (PKCE) and password + TOTP 2FA for web UI access, and per-user API keys for programmatic access.

```mermaid
flowchart TB
    subgraph Browser["Browser (React SPA)"]
        Login[Login Page]
        Profile[Profile Page]
        Providers[Providers Page]
        Usage[Usage Page]
        Config[Config Manager]
        Admin[Admin Panel]
        Guardrails[Guardrails Config]
    end

    subgraph Vite["Vite Dev Server (:5173)"]
        Static["/_ui/* → React SPA"]
        Proxy["/_ui/api/* → backend:9999"]
    end

    subgraph Backend["Rust Backend (HTTP :9999)"]
        subgraph PublicAPI["Public API (no auth)"]
            StatusAPI[GET /status]
            GoogleAuth[GET /auth/google]
            GoogleCB[GET /auth/google/callback]
            PasswordLogin[POST /auth/login]
            Login2FA[POST /auth/login/2fa]
        end
        subgraph SessionAPI["Session-Authenticated API"]
            AuthMe[GET /auth/me]
            ConfigRead[GET /config]
            SchemaAPI[GET /config/schema]
            SystemAPI[GET /system]
            ModelsAPI[GET /models]
            UsageAPI[GET /usage]
            KiroRoutes[Kiro token mgmt]
            KeyRoutes[API key mgmt]
            CopilotRoutes[Copilot OAuth]
            QwenRoutes[Qwen device flow]
            ProviderPriority[Provider priority]
            TwoFA[2FA setup/verify]
            PasswordChange[Password change]
            ModelRegistry[Model registry]
        end
        subgraph AdminAPI["Admin API (+ CSRF)"]
            ConfigWrite[PUT /config]
            UserMgmt[User Management]
            UserCreate[POST /admin/users/create]
            AdminUsage[GET /admin/usage]
            AdminPool[Admin provider pool]
            DomainAllow[Domain Allowlist]
            GuardrailsAdmin[Guardrails CRUD]
        end
    end

    Browser -->|HTTP| Vite
    Static --> Browser
    Proxy --> Backend
    Login --> GoogleAuth
    Login --> PasswordLogin
    Profile --> KeyRoutes
    Profile --> TwoFA
    Profile --> PasswordChange
    Providers --> KiroRoutes
    Providers --> CopilotRoutes
    Providers --> QwenRoutes
    Providers --> ProviderPriority
    Providers --> ModelRegistry
    Usage --> UsageAPI
    Config --> ConfigRead
    Config --> ConfigWrite
    Admin --> UserMgmt
    Admin --> UserCreate
    Admin --> AdminUsage
    Admin --> AdminPool
    Guardrails --> GuardrailsAdmin
```

---

## Accessing the Dashboard

Once the gateway is running via `docker compose up -d`, open your browser and navigate to:

```
https://your-domain/_ui/
```

In development, access via `http://localhost:5173/_ui/`. The Vite dev server proxies API requests to the backend.

---

## Authentication

The web dashboard supports two authentication methods, configurable via the admin panel:

- **Google SSO** (default) — PKCE (Proof Key for Code Exchange) with OpenID Connect
- **Password + mandatory TOTP 2FA** — Argon2 password hashing with mandatory TOTP-based two-factor authentication, recovery codes, and per-email rate limiting (5 attempts, 15-minute lockout)

### Login Flow (Google SSO)

1. Navigate to `/_ui/` — the SPA redirects unauthenticated users to the login page
2. Click "Sign in with Google" — initiates the PKCE flow
3. Authenticate with your Google account — the OAuth callback returns to the gateway
4. A session cookie (`kgw_session`) is set with a 24-hour TTL
5. A CSRF token cookie is also set for mutation protection

### Login Flow (Password + 2FA)

1. Navigate to `/_ui/` — the SPA redirects unauthenticated users to the login page
2. Enter email and password — `POST /_ui/api/auth/login`
3. If valid, the server returns a pending 2FA token
4. Enter TOTP code from authenticator app (or a recovery code) — `POST /_ui/api/auth/login/2fa`
5. A session cookie (`kgw_session`) is set with a 24-hour TTL

### Roles

The gateway supports two user roles:

| Role | Capabilities |
|------|-------------|
| **Admin** | Full access: view metrics/logs, manage configuration, manage users, manage domain allowlist, manage own API keys and Kiro tokens |
| **User** | Standard access: view metrics/logs, view configuration, manage own API keys and Kiro tokens |

The first user to sign in (via either method) is automatically assigned the **Admin** role.

### Session Management

- Sessions are stored in an in-memory cache (`session_cache` in AppState)
- Session cookie: `kgw_session` with 24-hour TTL
- CSRF token: separate cookie, required for all mutation endpoints (POST, PUT, DELETE)
- Use `GET /_ui/api/auth/me` to check current session status and user info
- Use `POST /_ui/api/auth/logout` to end the session

---

## Pages

The Web UI is organized into the following pages, accessible via the sidebar navigation:

### Profile (`/_ui/profile`)

The default landing page after login. Each user manages their account and security here:

- **Account Info** — Displays name, email, and role.
- **API Keys** — Create, list, and revoke personal API keys for programmatic access to `/v1/*` endpoints.
- **Security** — Account security management:
  - **Google Account Linking** — Link a Google account for SSO login (available for password-authenticated users).
  - **2FA Setup** — Set up or manage TOTP-based two-factor authentication.
  - **Password Management** — Change password (for password-authenticated users).

### Providers (`/_ui/providers`)

Multi-provider management with three tabs:

- **Status** — Provider health cards showing connection status for each configured provider.
- **Connections** — Connect and manage AI provider accounts via OAuth relay or device code flows (Kiro, Anthropic, OpenAI, Copilot, Qwen). Per-user provider priority ordering via `/_ui/api/providers/priority`.
- **Models** — Model registry management. Enable/disable models, populate from providers, delete entries.

### Usage (`/_ui/usage`)

Token usage tracking and analytics:

- **Personal Usage** — View your own usage statistics grouped by day, model, or provider.
- **Date Range Filter** — Filter usage by date range (default: last 30 days).
- **Admin Views** — Admins can view global usage statistics and per-user breakdowns.

### TOTP Setup (`/_ui/setup-2fa`)

TOTP two-factor authentication setup wizard:

- Generates a TOTP secret and displays a QR code for scanning with an authenticator app.
- Requires verification of a TOTP code before enabling 2FA.
- Generates 8 recovery codes (displayed once, stored as SHA-256 hashes).
- Forced setup on first login for password-authenticated users.

### Password Change (`/_ui/change-password`)

Password change form for password-authenticated users:

- Requires current password and new password.
- Forced change when an admin resets a user's password.

### Configuration (`/_ui/config`) — Admin Only

Gateway runtime configuration management:

- View all configuration settings with current values
- Edit settings with immediate hot-reload (where supported)
- Configuration schema with field types, descriptions, and validation rules
- Configuration change history with timestamps and old/new values

### Guardrails (`/_ui/guardrails`) — Admin Only

Content safety configuration powered by AWS Bedrock:

- **Profiles** — Create and manage AWS Bedrock guardrail connections (guardrail ID, version, region, AWS credentials). Enable/disable individually. Test profiles against sample content.
- **Rules** — Define when guardrails apply using CEL expressions. Configure apply direction (input/output/both), sampling rate, timeout, and linked profiles. Validate CEL syntax before saving.

### Admin (`/_ui/admin`) — Admin Only

User and access management:

- **User List** — View all registered users with their roles, status, and last login.
- **User Detail** (`/_ui/admin/users/:userId`) — View individual user details, change roles (Admin/User), manage user status.
- **Create User** — Create password-authenticated users with initial credentials (`POST /_ui/api/admin/users/create`).
- **Reset Password** — Force-reset a user's password (`POST /_ui/api/admin/users/:id/reset-password`).
- **Provider Pool** — Manage shared provider accounts (admin pool). Add, remove, enable/disable shared API keys.
- **Domain Allowlist** — Restrict Google SSO sign-in to specific email domains.

### Login (`/_ui/login`)

Login page supporting Google SSO (PKCE flow) and password + TOTP 2FA. Unauthenticated users are redirected here automatically. The available login methods depend on admin configuration.

---

## Setup Wizard

When the gateway starts for the first time with no admin user in the database, it enters **setup-only mode**. The proxy API endpoints (`/v1/*`) return `503 Service Unavailable` until setup is complete.

### Setup Flow

```mermaid
sequenceDiagram
    participant User as Browser
    participant UI as React SPA
    participant GW as Backend API
    participant Google as Google OAuth

    User->>UI: Navigate to http://localhost:5173/_ui/
    UI->>GW: GET /_ui/api/status
    GW-->>UI: {setup_complete: false}
    UI->>User: Show Setup Wizard

    Note over User,Google: Google SSO with PKCE
    User->>UI: Click "Sign in with Google"
    UI->>GW: GET /_ui/api/auth/google
    GW-->>User: Redirect to Google OAuth
    User->>Google: Authenticate & authorize
    Google-->>GW: GET /_ui/api/auth/google/callback (code + state)
    GW->>Google: Exchange code for tokens (with PKCE verifier)
    Google-->>GW: ID token + access token
    GW-->>GW: Create user (admin role), create session
    GW-->>User: Set session cookie, redirect to /_ui/

    Note over User,UI: Setup complete
    UI->>GW: GET /_ui/api/status
    GW-->>UI: {setup_complete: true}
    UI->>User: Show profile page
```

### Step-by-Step Walkthrough

1. **Navigate to the web UI** — The SPA detects that setup is incomplete and shows the setup wizard

2. **Sign in with Google** — Click the sign-in button to start the Google SSO flow. The gateway uses PKCE for security. You must use a Google account that matches the allowed domain (if domain allowlisting is configured).

3. **First user becomes admin** — After successful Google authentication, the first user is created with the **Admin** role. Setup is marked complete, and the proxy endpoints (`/v1/*`) become available.

4. **Configure Kiro credentials** — After setup, navigate to the Kiro token management section to provide your Kiro (AWS CodeWhisperer) credentials for API proxying.

---

## Configuration Management

After setup, admin users can manage gateway configuration through the dashboard.

### Viewing Configuration

The current configuration is available via `GET /_ui/api/config`, which returns all settings with their current values. The schema endpoint (`GET /_ui/api/config/schema`) provides field metadata including types, descriptions, and validation rules.

### Updating Configuration

Configuration changes are submitted via `PUT /_ui/api/config` (admin-only, requires CSRF token) with a JSON body containing the fields to update:

```json
{
  "log_level": "debug",
  "debug_mode": "errors",
  "truncation_recovery": true
}
```

### Configuration History

The `GET /_ui/api/config/history` endpoint returns a log of all configuration changes, allowing you to track when and what was modified.

---

## User & Access Management

### Per-User API Keys

Each user can create their own API keys for programmatic access to the `/v1/*` proxy endpoints. API keys are managed through the dashboard or via the `/_ui/api/` key management endpoints.

- Keys are stored as SHA-256 hashes in PostgreSQL
- Keys are cached in memory (`api_key_cache`) for fast lookup
- Clients authenticate with `Authorization: Bearer <api-key>` or `x-api-key: <api-key>`
- Each API key is associated with the user who created it, enabling per-user Kiro credential resolution

### Per-User Kiro Tokens

Each user manages their own Kiro (AWS CodeWhisperer) credentials. When a request arrives with a user's API key, the gateway uses that user's Kiro tokens to proxy the request.

- Kiro tokens are cached in memory (`kiro_token_cache`) with a 4-minute TTL
- Tokens auto-refresh before expiry
- Managed via the Kiro token routes in the dashboard

### Multi-Provider Credentials

Users manage provider connections on the **Providers page** (`/_ui/providers`):

- **Kiro (AWS)** — Connect via AWS SSO device code flow.
- **Anthropic** — Connect via OAuth PKCE relay.
- **GitHub Copilot** — OAuth authorization code flow. Requires server-side configuration (`GITHUB_COPILOT_CLIENT_ID`, `GITHUB_COPILOT_CLIENT_SECRET`, `GITHUB_COPILOT_CALLBACK_URL`).
- **Qwen Coder** — Device code flow. Requires `QWEN_OAUTH_CLIENT_ID` in server configuration.
- **Custom** — User-configured endpoint with API key.
- **Provider Priority** — Users set a priority order for provider fallback. The gateway routes requests to the highest-priority provider with valid credentials.

### Domain Allowlist (Admin)

Admins can configure a domain allowlist to restrict which Google accounts can sign in. Only email addresses matching an allowed domain will be permitted to create accounts.

### User Management (Admin)

Admins can view all users, change roles (admin/user), and remove users through the user management panel.

---

## System & Model Information

### System Information

The `GET /_ui/api/system` endpoint provides process-level system metrics:
- CPU usage (percentage)
- Memory consumption (bytes)
- Process uptime

### Available Models

The `GET /_ui/api/models` endpoint returns the list of models currently available through configured providers, useful for verifying that authentication is working and seeing which models you can use.

> **Note:** The previous real-time metrics streaming (`GET /_ui/api/stream/metrics`) and log streaming (`GET /_ui/api/stream/logs`) endpoints have been removed. Metrics and observability are now handled via Datadog APM/OTLP integration. Usage tracking is available via the Usage page and `/_ui/api/usage` endpoints.

---

## Content Guardrails Management

The Content Guardrails system provides content validation powered by AWS Bedrock guardrails with a flexible CEL (Common Expression Language) rule engine. This section is admin-only.

### Guardrail Profiles

A profile represents a connection to an AWS Bedrock guardrail. To create a profile, provide:

- **Name** — A descriptive name for the profile
- **Guardrail ID** — The AWS Bedrock guardrail identifier
- **Guardrail Version** — The version of the guardrail to use (e.g., `1`)
- **Region** — AWS region where the guardrail is deployed (e.g., `us-east-1`)
- **AWS Credentials** — Access key and secret key for authenticating with Bedrock (displayed masked in the UI)

Profiles can be enabled or disabled individually.

### Guardrail Rules

Rules define when and how guardrails are applied. Each rule includes:

- **Name and Description** — Identifies the rule's purpose
- **CEL Expression** — A condition that determines which requests the rule applies to (leave empty to match all requests)
- **Apply To** — Whether to validate input (before sending to Kiro), output (before returning to client), or both
- **Sampling Rate** — Percentage of matching requests to validate (0–100%). Use lower values for high-traffic scenarios
- **Timeout** — Maximum time to wait for Bedrock validation per rule
- **Linked Profiles** — One or more guardrail profiles to apply when this rule matches

Rules can be enabled or disabled individually.

### CEL Expression Variables

The following variables are available in CEL expressions:

| Variable | Type | Description |
|----------|------|-------------|
| `request.model` | string | Model name (e.g., `claude-sonnet-4-20250514`) |
| `request.api_format` | string | API format (`openai` or `anthropic`) |
| `request.message_count` | int | Number of messages in the conversation |
| `request.has_tools` | bool | Whether the request includes tool definitions |
| `request.is_streaming` | bool | Whether streaming is enabled |
| `request.content_length` | int | Total content length in bytes |

**Example expressions:**
- `request.model == "claude-opus-4"` — Only apply to a specific model
- `request.message_count > 5 && request.has_tools` — Apply to longer tool-using conversations
- `request.api_format == "openai"` — Apply only to OpenAI-format requests

### Testing

- **Test a profile** — Submit sample content against a specific profile to verify it's working correctly. Returns the guardrail action and response time.
- **Validate CEL expression** — Check that a CEL expression compiles without errors before saving a rule.

### Fail-Open Design

Guardrails are designed to fail open — if a Bedrock API call fails or times out, the request proceeds without blocking. This prevents guardrail infrastructure issues from causing outages.

**Note:** Output validation is only available for non-streaming requests. Streaming responses bypass output guardrail checks by design.

---

## API Endpoint Reference

All web UI API endpoints are nested under `/_ui/api/`.

### Public Endpoints (No Authentication)

| Method | Path | Description |
|---|---|---|
| `GET` | `/_ui/api/status` | Gateway status (includes `setup_complete` flag) |
| `GET` | `/_ui/api/auth/google` | Initiate Google SSO PKCE flow |
| `GET` | `/_ui/api/auth/google/callback` | Google OAuth callback handler |
| `POST` | `/_ui/api/auth/login` | Password login (returns pending 2FA token) |
| `POST` | `/_ui/api/auth/login/2fa` | Complete login with TOTP code or recovery code |

### Session-Authenticated Endpoints

These require a valid `kgw_session` cookie (obtained via Google SSO or password login).

| Method | Path | Description |
|---|---|---|
| `GET` | `/_ui/api/auth/me` | Current user info and session status |
| `GET` | `/_ui/api/auth/google/link` | Link Google account to existing user |
| `GET` | `/_ui/api/auth/2fa/setup` | Initiate TOTP 2FA setup (returns secret + QR URL) |
| `POST` | `/_ui/api/auth/2fa/verify` | Verify TOTP code to enable 2FA |
| `POST` | `/_ui/api/auth/password/change` | Change password (requires current password) |
| `GET` | `/_ui/api/system` | System info (CPU, memory, uptime) |
| `GET` | `/_ui/api/models` | List available models |
| `GET` | `/_ui/api/usage` | Personal usage statistics |
| `GET` | `/_ui/api/config` | Get current configuration |
| `GET` | `/_ui/api/config/schema` | Configuration field schema |
| `GET` | `/_ui/api/config/history` | Configuration change history |
| `GET` | `/_ui/api/providers/rate-limits` | Provider rate limit monitoring |

### Mutation Endpoints (Session + CSRF Token)

These require a valid session and CSRF token.

| Method | Path | Description |
|---|---|---|
| `POST` | `/_ui/api/auth/logout` | End current session |
| `*` | `/_ui/api/kiro/*` | Kiro token management (per-user) |
| `*` | `/_ui/api/keys/*` | API key management (per-user) |
| `*` | `/_ui/api/copilot/*` | GitHub Copilot OAuth connect/disconnect (per-user) |
| `*` | `/_ui/api/qwen/*` | Qwen Coder device flow connect/disconnect (per-user) |
| `*` | `/_ui/api/providers/*` | Provider OAuth relay, priority management, and account management (per-user) |
| `*` | `/_ui/api/models/registry/*` | Model registry CRUD (GET, PATCH, DELETE, POST populate) |

### Admin-Only Endpoints (Session + CSRF + Admin Role)

| Method | Path | Description |
|---|---|---|
| `PUT` | `/_ui/api/config` | Update gateway configuration |
| `*` | `/_ui/api/domains/*` | Domain allowlist management |
| `*` | `/_ui/api/users/*` | User management |
| `POST` | `/_ui/api/admin/users/create` | Create password-authenticated user |
| `POST` | `/_ui/api/admin/users/:id/reset-password` | Reset user's password |
| `GET` | `/_ui/api/admin/usage` | Global usage statistics |
| `GET` | `/_ui/api/admin/usage/users` | Per-user usage breakdown |
| `*` | `/_ui/api/admin/pool/*` | Provider pool account management |
| `*` | `/_ui/api/admin/guardrails/*` | Guardrails profile/rule management |

---

## Architecture

The web UI is implemented across several Rust modules in `backend/src/web_ui/`:

- **`mod.rs`** — Router construction, separating public, session-authenticated, and admin routes
- **`routes.rs`** — API HTTP handlers
- **`google_auth.rs`** — Google SSO with PKCE flow (OpenID Connect)
- **`session.rs`** — Session cookie management and CSRF validation
- **`api_keys.rs`** — Per-user API key CRUD (create, list, revoke)
- **`user_kiro.rs`** — Per-user Kiro token management
- **`copilot_auth.rs`** — GitHub Copilot OAuth connect/callback/status/disconnect
- **`qwen_auth.rs`** — Qwen Coder device flow connect/poll/status/disconnect
- **`provider_oauth.rs`** — Multi-provider OAuth relay and public callback routes
- **`provider_priority.rs`** — Per-user provider priority ordering
- **`users.rs`** — User management (admin)
- **`config_api.rs`** — Configuration validation, change classification, and field descriptions
- **`config_db.rs`** — PostgreSQL persistence layer for configuration key-value storage
- **`password_auth.rs`** — Password authentication with mandatory TOTP 2FA, rate limiting, recovery codes, admin user creation/password reset
- **`usage.rs`** — Usage tracking endpoints (personal and admin views)
- **`admin_pool.rs`** — Admin provider pool account management and rate limit monitoring
- **`model_registry.rs`** — Model registry data layer (PostgreSQL)
- **`model_registry_handlers.rs`** — Model registry HTTP handlers (CRUD endpoints)
- **`crypto.rs`** — AES-256-GCM encryption for sensitive config values

The React frontend source lives in `frontend/` (Vite + TypeScript). In development, the Vite dev server serves the SPA and proxies API requests to the backend.
