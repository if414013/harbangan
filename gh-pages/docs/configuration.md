---
layout: default
title: Configuration
nav_order: 4
---

# Configuration Reference
{: .no_toc }

Complete reference for all Harbangan configuration options. Configuration varies by deployment mode.

<details open markdown="block">
  <summary>Table of contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Configuration Model

Harbangan has two deployment modes with different configuration models:

- **Proxy-Only Mode** (`docker-compose.gateway.yml`) — All configuration via environment variables in `.env.proxy`. No database or Web UI.
- **Full Deployment** (`docker-compose.yml`) — Two-tier model: bootstrap settings via environment variables in `.env`, plus runtime settings via the Web UI (persisted in PostgreSQL).

---

## Proxy-Only Mode Environment Variables

Set these in `.env.proxy` (copy from `.env.proxy.example`) and pass via `--env-file .env.proxy` when running `docker compose -f docker-compose.gateway.yml`.

Proxy-Only Mode supports **multiple providers** via environment variables: Kiro (AWS CodeWhisperer), Anthropic, OpenAI Codex, GitHub Copilot, and custom OpenAI-compatible endpoints.

### Required

| Variable | Description | Example |
|:---|:---|:---|
| `PROXY_API_KEY` | API key that clients use to authenticate all requests. | `my-secret-key` |

> `GATEWAY_MODE=proxy` is set automatically by `docker-compose.gateway.yml` — you do not need to set it in `.env.proxy`.

### Optional

| Variable | Default | Description |
|:---|:---|:---|
| `KIRO_ENABLED` | `true` | Set to `false` to skip Kiro device flow entirely. |
| `KIRO_REGION` | `us-east-1` | AWS region for the Kiro API endpoint. |
| `KIRO_SSO_URL` | _(omit for Builder ID)_ | Identity Center SSO issuer URL. Omit this to use Builder ID (free). |
| `KIRO_SSO_REGION` | same as `KIRO_REGION` | AWS region for the SSO OIDC endpoint. Only needed if different from `KIRO_REGION`. |
| `SERVER_PORT` | `8000` | Port the gateway listens on. |
| `BIND_ADDRESS` | `127.0.0.1` | Address to bind the gateway port. Set to `0.0.0.0` to allow external access. |
| `LOG_LEVEL` | `info` | Log verbosity: `debug`, `info`, `warn`, `error`. |
| `DEBUG_MODE` | `off` | Debug logging: `off`, `errors`, `all`. |
| `ANTHROPIC_API_KEY` | _(none)_ | Anthropic API key for direct Anthropic provider access. |
| `OPENAI_API_KEY` | _(none)_ | OpenAI API key for direct OpenAI provider access. |
| `OPENAI_BASE_URL` | `https://api.openai.com/v1` | OpenAI API base URL (override for compatible endpoints). |
| `COPILOT_ENABLED` | `false` | Set to `true` to enable Copilot device flow. |
| `COPILOT_TOKEN` | _(none)_ | GitHub Copilot token. |
| `COPILOT_BASE_URL` | `https://api.githubcopilot.com` | Copilot API base URL. |
| `CUSTOM_PROVIDER_URL` | _(none)_ | Custom OpenAI-compatible endpoint URL. |
| `CUSTOM_PROVIDER_KEY` | _(none)_ | Custom provider API key. |
| `CUSTOM_PROVIDER_MODELS` | _(none)_ | Comma-separated model names for custom provider. |

### Builder ID vs Identity Center

- **Builder ID (free):** Leave `KIRO_SSO_URL` unset. The device code flow will prompt you to sign in with your personal AWS Builder ID.
- **Identity Center (pro):** Set `KIRO_SSO_URL` to your organization's SSO start URL (e.g., `https://your-org.awsapps.com/start`). The device code flow will prompt you to sign in with your corporate Identity Center account.

### Example `.env.proxy`

```bash
# Proxy-Only Mode — Harbangan
# Usage: docker compose -f docker-compose.gateway.yml --env-file .env.proxy up -d

GATEWAY_MODE=proxy
PROXY_API_KEY=your-api-key-here

# Optional: Kiro API region (default: us-east-1)
# KIRO_REGION=us-east-1

# Optional: SSO OIDC region (defaults to KIRO_REGION)
# KIRO_SSO_REGION=

# Optional: SSO start URL (for Identity Center users)
# KIRO_SSO_URL=

# Optional: Server bind address and port
# BIND_ADDRESS=127.0.0.1
# SERVER_PORT=8000

# Optional: Logging
# LOG_LEVEL=info
# DEBUG_MODE=off
```

---

## Full Deployment Environment Variables

Set these in your `.env` file before running `docker compose up`. They are read at startup by docker-compose and the backend container.

### Required

| Variable | Description | Example |
|:---|:---|:---|
| `POSTGRES_PASSWORD` | PostgreSQL password. Used by both the `db` and `backend` services. | `your_secure_password` |

### Optional Variables

#### Admin Seeding

| Variable | Description | Example |
|:---|:---|:---|
| `INITIAL_ADMIN_EMAIL` | Seed admin email for headless/automated first-run setup. | `admin@example.com` |
| `INITIAL_ADMIN_PASSWORD` | Seed admin password (Argon2 hashed on first startup). | `changeme` |
| `INITIAL_ADMIN_TOTP_SECRET` | Base32 TOTP secret pre-configured for admin 2FA (enables automated E2E auth). | `JBSWY3DPEHPK3PXP` |

#### Google SSO (optional — configured via Admin UI after login)

Google SSO is an optional auth method. You can use password auth exclusively, or configure Google SSO via the Admin UI after initial login. If you prefer to set it via environment variables:

| Variable | Description | Example |
|:---|:---|:---|
| `GOOGLE_CLIENT_ID` | Google OAuth 2.0 Client ID for Web UI authentication. | `123456.apps.googleusercontent.com` |
| `GOOGLE_CLIENT_SECRET` | Google OAuth 2.0 Client Secret. | `GOCSPX-abc123` |
| `GOOGLE_CALLBACK_URL` | OAuth redirect URI. Must match the authorized redirect URI in Google Cloud Console. | `http://localhost:9999/_ui/api/auth/google/callback` |

#### Provider OAuth

| Variable | Description | Example |
|:---|:---|:---|
| `GITHUB_COPILOT_CLIENT_ID` | GitHub Copilot OAuth client ID. | _(your app's client ID)_ |
| `GITHUB_COPILOT_CLIENT_SECRET` | GitHub Copilot OAuth client secret. | _(your app's client secret)_ |
| `GITHUB_COPILOT_CALLBACK_URL` | GitHub Copilot OAuth callback URL. | `http://localhost:9999/_ui/api/copilot/callback` |

#### Security

| Variable | Description | Example |
|:---|:---|:---|
| `CONFIG_ENCRYPTION_KEY` | Base64-encoded 32-byte key for encrypting sensitive config values in PostgreSQL. | _(base64 string)_ |

### Auto-managed by docker-compose

These are set automatically in `docker-compose.yml`. Do **not** set them in `.env`:

| Variable | Value in Docker | Description |
|:---|:---|:---|
| `SERVER_HOST` | `0.0.0.0` | Backend bind address (internal only). |
| `SERVER_PORT` | `9999` | Backend listen port (internal only). |
| `DATABASE_URL` | `postgres://kiro:<POSTGRES_PASSWORD>@db:5432/kiro_gateway` | PostgreSQL connection string. |

---

## Runtime Configuration (Web UI)

These settings are managed through the Web UI at `/_ui/` and stored in PostgreSQL. Changes take effect based on their type:

| Setting | Default | Hot-reload | Description |
|:---|:---|:---|:---|
| `kiro_region` | `us-east-1` | No | AWS region for the Kiro API endpoint. |
| `log_level` | `info` | Yes | Log verbosity: `trace`, `debug`, `info`, `warn`, `error`. |
| `debug_mode` | `off` | Yes | Debug logging: `off`, `errors`, `all`. |
| `fake_reasoning_enabled` | `true` | Yes | Enable reasoning/thinking block extraction. |
| `fake_reasoning_max_tokens` | `4000` | Yes | Maximum tokens for reasoning content. |
| `truncation_recovery` | `true` | Yes | Detect and retry truncated API responses. |
| `tool_description_max_length` | `10000` | Yes | Max character length for tool descriptions. |
| `first_token_timeout` | `15` (sec) | Yes | Cancel and retry if no token received within this time. |
| `guardrails_enabled` | `false` | Yes | Enable/disable content guardrails globally. |

**Hot-reload = Yes** means the change applies immediately without restarting. **Hot-reload = No** means the change is saved to the database but requires a restart to take effect.

---

## Google OAuth Setup

Google SSO is optional. You can use password authentication exclusively. To enable Google SSO:

1. Go to the [Google Cloud Console](https://console.cloud.google.com/apis/credentials)
2. Create a new **OAuth 2.0 Client ID** (Web application type)
3. Add the authorized redirect URI: `https://YOUR_DOMAIN/_ui/api/auth/google/callback`
4. Sign in to the Admin UI at `/_ui/` and enter the Client ID, Client Secret, and Callback URL in the Configuration page

The gateway uses PKCE + OpenID Connect for the SSO flow. Session cookies (`kgw_session`) have a 24-hour TTL.

---

## Authentication

Harbangan uses two separate authentication systems:

### API key auth (for `/v1/*` proxy endpoints)

Clients include their API key in requests:

```bash
# Via Authorization header
curl -H "Authorization: Bearer YOUR_API_KEY" https://your-domain.com/v1/models

# Via x-api-key header
curl -H "x-api-key: YOUR_API_KEY" https://your-domain.com/v1/models
```

API keys are per-user, created through the Web UI. The gateway SHA-256 hashes the key and looks up the user in cache/database to resolve their Kiro credentials.

### Session Auth (for `/_ui/*` web UI)

Web UI access requires signing in with Google. The first user to sign in gets the Admin role. Admins can manage users, configuration, and domain allowlists.

---

## Domain Allowlist

Admins can configure a domain allowlist to restrict which Google accounts can sign in. When the allowlist is empty, any Google account can sign in. When populated, only accounts with email addresses matching an allowed domain (e.g., `example.com`) can access the Web UI.

---

## Setup-Only Mode

On first launch (no admin user in the database), the gateway operates in **setup-only mode**:

- `/v1/*` proxy endpoints return **503 Service Unavailable**
- The Web UI is accessible for the first user to complete setup
- Once the first user signs in (via Google SSO or password auth), they get the Admin role and setup mode ends

---

## PostgreSQL Database

### What's stored

| Table | Contents |
|:---|:---|
| `users` | User accounts (Google identity, role, status) |
| `api_keys` | Per-user API keys (SHA-256 hashed) |
| `user_kiro_credentials` | Per-user Kiro refresh tokens |
| `config` | Key-value runtime configuration |
| `config_history` | Audit log of configuration changes |
| `guardrail_profiles` | AWS Bedrock guardrail profiles (credentials encrypted) |
| `guardrail_rules` | Guardrail rules (CEL expressions, sampling, timeouts) |
| `guardrail_rule_profiles` | Many-to-many mapping of rules to profiles |

### Backup and restore

```bash
# Backup
docker compose exec db pg_dump -U kiro kiro_gateway > backup.sql

# Restore
docker compose exec -T db psql -U kiro kiro_gateway < backup.sql
```

---

## Configuration via Web UI

### Configuration page (`/_ui/`)

After setup, the configuration page lets admins modify runtime settings. Changes are persisted to PostgreSQL and take effect based on their hot-reload status.

### Configuration history

Every configuration change is logged with:

- The key that changed
- Old and new values (sensitive values are masked)
- Timestamp
- Source (e.g., `web_ui`, `setup`)

---

## Example `.env` File (Full Deployment)

```bash
# Harbangan — Full Deployment Configuration
# Copy to .env and fill in your values.

# PostgreSQL password
POSTGRES_PASSWORD=change-me-to-something-strong

# Google SSO (optional — or configure via Admin UI)
GOOGLE_CLIENT_ID=your-client-id.apps.googleusercontent.com
GOOGLE_CLIENT_SECRET=your-client-secret
GOOGLE_CALLBACK_URL=https://gateway.example.com/_ui/api/auth/google/callback

# Initial admin account (optional — for headless/automated setup)
# INITIAL_ADMIN_EMAIL=admin@example.com
# INITIAL_ADMIN_PASSWORD=changeme
# INITIAL_ADMIN_TOTP_SECRET=your-base32-totp-secret-here
```

---

## Datadog APM Environment Variables

Datadog APM is opt-in and zero-overhead when not configured. Set these in your `.env` (Full Deployment) or `.env.proxy` (Proxy-Only) to enable observability.

These variables are intended for production/Kubernetes deployments where a Datadog Agent is running separately. They are not used in local Docker Compose development. Set `DD_AGENT_HOST` to the Datadog Agent's address to enable tracing.

### Backend APM Variables

| Variable | Required | Default | Description |
|:---|:---|:---|:---|
| `DD_API_KEY` | Yes (for agent) | | Datadog API key. Required to activate the agent. |
| `DD_SITE` | No | `datadoghq.com` | Datadog intake site. Other options: `datadoghq.eu`, `us3.datadoghq.com`, `us5.datadoghq.com`. |
| `DD_ENV` | No | | Environment tag applied to all traces and metrics (e.g. `production`, `staging`). |
| `DD_AGENT_HOST` | Auto | | Set automatically by docker-compose to `datadog-agent`. Do not set manually. |

When `DD_AGENT_HOST` is set, the backend adds a Datadog tracing layer to the `tracing_subscriber` registry and exports spans via OTLP. When unset, no Datadog code runs.

### Frontend RUM Variables

These are **build-time** variables baked into the JavaScript bundle. They must be set before running `docker compose build frontend` (or `docker compose up --build`).

| Variable | Required | Default | Description |
|:---|:---|:---|:---|
| `VITE_DD_CLIENT_TOKEN` | Yes (for RUM) | | Browser RUM client token from Datadog. |
| `VITE_DD_APPLICATION_ID` | Yes (for RUM) | | Browser RUM application ID from Datadog. |
| `VITE_DD_ENV` | No | | RUM environment tag (e.g. `production`). |
| `VITE_DD_SERVICE` | No | `harbangan-frontend` | RUM service name. |

If `VITE_DD_CLIENT_TOKEN` or `VITE_DD_APPLICATION_ID` are empty at build time, the RUM SDK is not initialized and no browser data is sent.

### Example `.env` with Datadog

```bash
# ... existing required variables ...

# Datadog APM (optional)
DD_API_KEY=your-datadog-api-key
DD_SITE=datadoghq.com
DD_ENV=production

# Datadog RUM — set before building frontend
VITE_DD_CLIENT_TOKEN=your-rum-client-token
VITE_DD_APPLICATION_ID=your-rum-application-id
VITE_DD_ENV=production
```

---

## Next Steps

- [Getting Started](getting-started.html) — Full setup walkthrough for both modes
- [Deployment Guide](deployment.html) — Production deployment for Proxy-Only Mode and Full Deployment
