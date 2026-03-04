---
layout: default
title: Configuration
nav_order: 4
---

# Configuration Reference
{: .no_toc }

Complete reference for all Kiro Gateway configuration options. Configuration varies by deployment mode.

<details open markdown="block">
  <summary>Table of contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Configuration Model

Kiro Gateway has two deployment modes with different configuration models:

- **Proxy-Only Mode** (`docker-compose.gateway.yml`) — All configuration via environment variables in `.env.proxy`. No database or Web UI.
- **Full Deployment** (`docker-compose.yml`) — Two-tier model: bootstrap settings via environment variables in `.env`, plus runtime settings via the Web UI (persisted in PostgreSQL).

---

## Proxy-Only Mode Environment Variables

Set these in `.env.proxy` and pass via `--env-file .env.proxy` when running `docker compose -f docker-compose.gateway.yml`.

### Required

| Variable | Description | Example |
|:---|:---|:---|
| `PROXY_API_KEY` | API key that clients use to authenticate all requests. | `my-secret-key` |

### Optional

| Variable | Default | Description |
|:---|:---|:---|
| `KIRO_REGION` | `us-east-1` | AWS region for the Kiro API endpoint. |
| `KIRO_SSO_URL` | _(omit for Builder ID)_ | Identity Center SSO issuer URL. Omit this to use Builder ID (free). |
| `KIRO_SSO_REGION` | same as `KIRO_REGION` | AWS region for the SSO OIDC endpoint. Only needed if different from `KIRO_REGION`. |
| `SERVER_PORT` | `8000` | Port the gateway listens on. |
| `LOG_LEVEL` | `info` | Log verbosity: `debug`, `info`, `warn`, `error`. |
| `DEBUG_MODE` | `off` | Debug logging: `off`, `errors`, `all`. |

### Builder ID vs Identity Center

- **Builder ID (free):** Leave `KIRO_SSO_URL` unset. The device code flow will prompt you to sign in with your personal AWS Builder ID.
- **Identity Center (pro):** Set `KIRO_SSO_URL` to your organization's SSO start URL (e.g., `https://your-org.awsapps.com/start`). The device code flow will prompt you to sign in with your corporate Identity Center account.

### Example `.env.proxy`

```bash
# Proxy-Only Mode — Kiro Gateway
PROXY_API_KEY=your-secret-api-key
KIRO_REGION=us-east-1

# Uncomment for Identity Center (pro):
# KIRO_SSO_URL=https://your-org.awsapps.com/start
# KIRO_SSO_REGION=us-east-1

# Optional overrides:
# SERVER_PORT=8000
# LOG_LEVEL=info
# DEBUG_MODE=off
```

---

## Full Deployment Environment Variables

Set these in your `.env` file before running `docker compose up`. They are read at startup by docker-compose and the backend container.

### Required

| Variable | Description | Example |
|:---|:---|:---|
| `DOMAIN` | Domain name for Let's Encrypt TLS certificates. Must have DNS pointing to the server. | `gateway.example.com` |
| `EMAIL` | Email address for Let's Encrypt certificate notifications. | `admin@example.com` |
| `POSTGRES_PASSWORD` | PostgreSQL password. Used by both the `db` and `backend` services. | `your_secure_password` |
| `GOOGLE_CLIENT_ID` | Google OAuth 2.0 Client ID for Web UI authentication. | `123456.apps.googleusercontent.com` |
| `GOOGLE_CLIENT_SECRET` | Google OAuth 2.0 Client Secret. | `GOCSPX-abc123` |
| `GOOGLE_CALLBACK_URL` | OAuth redirect URI. Must match the authorized redirect URI in Google Cloud Console. | `https://gateway.example.com/_ui/api/auth/google/callback` |

### Auto-managed by docker-compose

These are set automatically in `docker-compose.yml`. Do **not** set them in `.env`:

| Variable | Value in Docker | Description |
|:---|:---|:---|
| `SERVER_HOST` | `0.0.0.0` | Backend bind address (internal only). |
| `SERVER_PORT` | `8000` | Backend listen port (internal only). |
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
| `mcp_enabled` | `false` | Yes | Enable/disable MCP Gateway globally. |
| `mcp_tool_execution_timeout` | `30` (sec) | Yes | Tool call timeout in seconds (1–86400). |
| `mcp_health_check_interval` | `10` (sec) | Yes | Health monitor polling interval in seconds (1–86400). |
| `mcp_max_consecutive_failures` | `5` | Yes | Failures before marking an MCP client as Error (1–100). |
| `guardrails_enabled` | `false` | Yes | Enable/disable content guardrails globally. |

**Hot-reload = Yes** means the change applies immediately without restarting. **Hot-reload = No** means the change is saved to the database but requires a restart to take effect.

---

## Google OAuth Setup

To use Google SSO for Web UI authentication:

1. Go to the [Google Cloud Console](https://console.cloud.google.com/apis/credentials)
2. Create a new **OAuth 2.0 Client ID** (Web application type)
3. Add the authorized redirect URI: `https://YOUR_DOMAIN/_ui/api/auth/google/callback`
4. Copy the Client ID and Client Secret into your `.env` file

The gateway uses PKCE + OpenID Connect for the SSO flow. Session cookies (`kgw_session`) have a 24-hour TTL.

---

## Authentication

Kiro Gateway uses two separate authentication systems:

### API key auth (for `/v1/*` proxy endpoints)

Clients include their API key in requests:

```bash
# Via Authorization header
curl -H "Authorization: Bearer YOUR_API_KEY" https://your-domain.com/v1/models

# Via x-api-key header
curl -H "x-api-key: YOUR_API_KEY" https://your-domain.com/v1/models
```

API keys are per-user, created through the Web UI. The gateway SHA-256 hashes the key and looks up the user in cache/database to resolve their Kiro credentials.

### Google SSO (for `/_ui/*` web UI)

Web UI access requires signing in with Google. The first user to sign in gets the Admin role. Admins can manage users, configuration, and domain allowlists.

---

## Domain Allowlist

Admins can configure a domain allowlist to restrict which Google accounts can sign in. When the allowlist is empty, any Google account can sign in. When populated, only accounts with email addresses matching an allowed domain (e.g., `example.com`) can access the Web UI.

---

## Setup-Only Mode

On first launch (no admin user in the database), the gateway operates in **setup-only mode**:

- `/v1/*` proxy endpoints return **503 Service Unavailable**
- The Web UI is accessible for the first user to complete setup
- Once the first user signs in via Google SSO, they get the Admin role and setup mode ends

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
| `mcp_clients` | MCP server connections (config, state, encrypted headers) |
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
# Kiro Gateway — Full Deployment Configuration
# Copy to .env and fill in your values.

# Domain for TLS certificates (Let's Encrypt via certbot)
DOMAIN=gateway.example.com

# Email for Let's Encrypt certificate notifications
EMAIL=admin@example.com

# PostgreSQL password
POSTGRES_PASSWORD=change-me-to-something-strong

# Google SSO (required)
GOOGLE_CLIENT_ID=your-client-id.apps.googleusercontent.com
GOOGLE_CLIENT_SECRET=your-client-secret
GOOGLE_CALLBACK_URL=https://gateway.example.com/_ui/api/auth/google/callback
```

---

## Next Steps

- [Getting Started](getting-started.html) — Full setup walkthrough for both modes
- [Deployment Guide](deployment.html) — Production deployment for Proxy-Only Mode and Full Deployment
