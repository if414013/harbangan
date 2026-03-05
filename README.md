<div align="center">

# rkgw — Rust Kiro Gateway

**Multi-user proxy gateway for Kiro API (AWS CodeWhisperer)**

[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)
[![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)](https://www.rust-lang.org/)

_A Rust rewrite of [kiro-gateway](https://github.com/jwadow/kiro-gateway) — Use Claude models through any OpenAI or Anthropic compatible tool_

[Features](#-features) • [Quick Start](#-quick-start) • [Configuration](#-configuration) • [API Reference](#-api-reference)

</div>

---

## Attribution

This project is a Rust rewrite of the original [kiro-gateway](https://github.com/jwadow/kiro-gateway) by [@Jwadow](https://github.com/jwadow). The original project is written in Python using FastAPI.

---

## Supported Models

| Model                 | Description                                           |
| --------------------- | ----------------------------------------------------- |
| **Claude Opus 4.6**   | Latest flagship. Adaptive thinking, complex reasoning |
| **Claude Sonnet 4.6** | Latest balanced. Coding, writing, general-purpose     |
| **Claude Haiku 4.5**  | Lightning fast. Quick responses, simple tasks         |

> **Smart Model Resolution:** Use any model name format — `claude-sonnet-4-6`, `claude-sonnet-4.6`, or versioned names like `claude-sonnet-4-20250514`. The gateway normalizes them automatically.

---

## Features

| Feature                       | Description                                                                              |
| ----------------------------- | ---------------------------------------------------------------------------------------- |
| **OpenAI-compatible API**     | Works with any OpenAI-compatible tool (`/v1/chat/completions`)                           |
| **Anthropic-compatible API**  | Native `/v1/messages` endpoint                                                           |
| **Extended Thinking**         | Reasoning support with adaptive effort levels                                            |
| **Vision Support**            | Send images to model                                                                     |
| **Tool Calling**              | Function calling support                                                                 |
| **Full message history**      | Complete conversation context                                                            |
| **SSE Streaming**             | Full server-sent events streaming support                                                |
| **Retry Logic**               | Automatic retries with truncation recovery                                               |
| **Multi-user support**        | Per-user Kiro credentials and API keys                                                   |
| **Google SSO**                | Sign in with Google (PKCE + OpenID Connect) for web UI                                   |
| **Role-based access control** | Admin and User roles with granular permissions                                           |
| **MCP Gateway**               | Connect external MCP tool servers (HTTP/SSE/STDIO) for tool injection into chat requests |
| **Content Guardrails**        | AWS Bedrock-powered input/output content validation with CEL rule engine                 |
| **Web UI Dashboard**          | Real-time metrics, logs, configuration, and user management                              |
| **Let's Encrypt TLS**         | Automated HTTPS via certbot with auto-renewal                                            |
| **Proxy-Only Mode**           | Single API key, no DB/SSO — just a pure proxy (`docker-compose.gateway.yml`)             |
| **Datadog APM**               | Optional observability: distributed tracing, metrics, log correlation, and frontend RUM  |

---

## Quick Start

### Proxy-Only Mode

If you just want a simple proxy without PostgreSQL, Google SSO, or the web UI:

```bash
git clone https://github.com/if414013/rkgw.git
cd rkgw
```

Create `.env.proxy`:

```env
PROXY_API_KEY=your-secret-api-key
KIRO_REGION=us-east-1
# For Identity Center (pro): set your SSO URL
# KIRO_SSO_URL=https://your-org.awsapps.com/start
# KIRO_SSO_REGION=us-east-1
```

```bash
docker compose -f docker-compose.gateway.yml --env-file .env.proxy up
```

On first boot, the container runs a device code flow — check the logs for a URL to open in your browser:

```
╔═══════════════════════════════════════════════════════════╗
║  Open this URL in your browser to authorize:             ║
║  https://device.sso.us-east-1.amazonaws.com/?user_code=… ║
╚═══════════════════════════════════════════════════════════╝
```

Credentials are cached in a Docker volume — you only need to authorize once. On subsequent restarts, the gateway reuses the cached tokens automatically.

The gateway binds to `127.0.0.1:8000` (localhost only) and authenticates requests with `PROXY_API_KEY`. To expose it externally, place a reverse proxy (nginx, Caddy, etc.) in front — this is the recommended approach as it adds TLS termination.

> **Security warning:** If you override the binding to `0.0.0.0:8000` (e.g. by editing the `ports` entry in `docker-compose.gateway.yml`), the gateway is directly reachable from any network interface with no TLS and no authentication beyond `PROXY_API_KEY`. Anyone who can reach the host on that port and knows the key can make API calls. Only do this behind a firewall with strict ingress rules, and use a strong randomly-generated key.

```bash
curl http://localhost:8000/v1/chat/completions \
  -H "Authorization: Bearer your-secret-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4-6",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

### Full Deployment (Multi-User)

#### Prerequisites

- [Docker](https://docs.docker.com/get-docker/) and [Docker Compose](https://docs.docker.com/compose/install/)
- A domain name pointing to your server (for Let's Encrypt TLS)
- [Google OAuth credentials](https://console.cloud.google.com/apis/credentials) (Client ID + Secret)
- [Kiro CLI](https://kiro.dev/cli/) installed and logged in (for your refresh token)

### 1. Clone and configure

```bash
git clone https://github.com/if414013/rkgw.git
cd rkgw
cp .env.example .env
```

Edit `.env` with your values:

```env
DOMAIN=gateway.example.com
EMAIL=admin@example.com
POSTGRES_PASSWORD=your_secure_password
GOOGLE_CLIENT_ID=your-client-id.apps.googleusercontent.com
GOOGLE_CLIENT_SECRET=your-client-secret
GOOGLE_CALLBACK_URL=https://gateway.example.com/_ui/api/auth/google/callback
```

### 2. Provision TLS certificates

```bash
chmod +x init-certs.sh
DOMAIN=gateway.example.com EMAIL=admin@example.com ./init-certs.sh
```

This creates a temporary self-signed cert so nginx can start, then obtains a real Let's Encrypt certificate via certbot.

### 3. Start the gateway

```bash
docker compose up -d
```

### 4. Complete setup

Open `https://gateway.example.com/_ui/` in your browser. Sign in with Google — the first user automatically gets the **admin** role. From the web UI you can:

- Add your Kiro refresh token (run `kiro login` first)
- Generate per-user API keys for programmatic access
- Configure model settings, timeouts, and debug options
- Manage additional users and their roles

> **Setup-only mode:** Until the first admin completes setup, the gateway returns 503 on all `/v1/*` proxy endpoints.

---

## Architecture

```
Internet → nginx (frontend, :443/:80)
              ├── /_ui/*           → React SPA static files
              ├── /_ui/api/*       → proxy → backend:8000
              ├── /v1/*            → proxy → backend:8000 (SSE streaming)
              └── /.well-known/    → certbot webroot
           certbot   → Let's Encrypt cert auto-renewal (12h cycle)
           backend   → Rust API server (plain HTTP, internal only)
           db        → PostgreSQL 16
```

Four docker-compose services:

| Service      | Image                   | Description                                                                             |
| ------------ | ----------------------- | --------------------------------------------------------------------------------------- |
| **db**       | `postgres:16-alpine`    | PostgreSQL database for config, users, API keys, guardrails rules, and MCP client state |
| **backend**  | `kiro-gateway-backend`  | Rust API server (Axum 0.7 + Tokio) with MCP Gateway and content guardrails              |
| **frontend** | `kiro-gateway-frontend` | nginx serving React SPA + reverse proxy to backend                                      |
| **certbot**  | `certbot/certbot`       | Let's Encrypt certificate provisioning and renewal                                      |

### Authentication

Two separate auth systems:

1. **API key auth** (for `/v1/*` proxy endpoints): Clients send `Authorization: Bearer <api-key>` or `x-api-key` header. The middleware SHA-256 hashes the key, looks up the user in cache/DB, and injects per-user Kiro credentials into the request.

2. **Google SSO** (for `/_ui/api/*` web UI): PKCE + OpenID Connect flow. Session cookie `kgw_session` (24h TTL), CSRF token in separate cookie. Admin vs User roles.

---

## Configuration

### Proxy-Only Environment Variables

For proxy-only mode (`docker-compose.gateway.yml`):

| Variable          | Required | Default       | Description                               |
| ----------------- | -------- | ------------- | ----------------------------------------- |
| `PROXY_API_KEY`   | Yes      |               | API key clients use to authenticate       |
| `KIRO_SSO_URL`    | No       |               | Identity Center URL (omit for Builder ID) |
| `KIRO_SSO_REGION` | No       | same as above | AWS SSO OIDC region                       |
| `KIRO_REGION`     | No       | `us-east-1`   | Kiro API region                           |
| `SERVER_PORT`     | No       | `8000`        | Listen port                               |
| `LOG_LEVEL`       | No       | `info`        | `debug`, `info`, `warn`, `error`          |
| `DEBUG_MODE`      | No       | `off`         | `off`, `errors`, `all`                    |

### Full Deployment Environment Variables

Set in `.env` (see `.env.example`):

| Variable               | Required | Description                        |
| ---------------------- | -------- | ---------------------------------- |
| `DOMAIN`               | Yes      | Domain for Let's Encrypt TLS certs |
| `EMAIL`                | Yes      | Let's Encrypt notification email   |
| `POSTGRES_PASSWORD`    | Yes      | PostgreSQL password                |
| `GOOGLE_CLIENT_ID`     | Yes      | Google OAuth Client ID             |
| `GOOGLE_CLIENT_SECRET` | Yes      | Google OAuth Client Secret         |
| `GOOGLE_CALLBACK_URL`  | Yes      | OAuth callback URL                 |

Auto-set by docker-compose: `DATABASE_URL`, `SERVER_HOST` (0.0.0.0), `SERVER_PORT` (8000).

### Datadog APM (Optional)

Both deployment modes support optional Datadog observability via a sidecar agent. Set `DD_API_KEY` and `DD_SITE` in your `.env`, then start with `--profile datadog`:

```bash
docker compose --profile datadog up -d
```

See [docs/docker-deploy.md](docs/docker-deploy.md#datadog-apm-optional) for full setup instructions and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#observability-datadog-apm) for the architecture details.

### Runtime Configuration

All runtime settings — region, timeouts, debug mode, allowed domains, etc. — are managed through the Web UI at `/_ui/` and persisted in PostgreSQL. There are no CLI flags or additional environment variables for runtime configuration.

---

## API Reference

### Endpoints

**Proxy (auth via per-user API key):**

- `POST /v1/chat/completions` — OpenAI-compatible
- `POST /v1/messages` — Anthropic-compatible
- `GET /v1/models` — List available models

**MCP (auth via API key):**

- `POST /v1/mcp/tool/execute` — Execute an MCP tool
- `POST /mcp` — JSON-RPC 2.0 MCP server protocol
- `GET /mcp` — MCP SSE stream

**Infrastructure:**

- `GET /health` — Health check
- `GET /` — Status JSON

**Web UI (`/_ui/`):**

- Dashboard with real-time metrics and logs (SSE)
- User management and role-based access control
- Per-user API key generation and Kiro token management
- Gateway configuration (region, timeouts, allowed domains)
- MCP server management — register, connect, and manage external tool servers (admin)
- Guardrails management — configure content safety profiles and CEL-based rules (admin)

---

## API Usage Examples

<details>
<summary>View API usage examples</summary>

### OpenAI API

```bash
curl https://gateway.example.com/v1/chat/completions \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4-6",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

### Anthropic API

```bash
curl https://gateway.example.com/v1/messages \
  -H "x-api-key: YOUR_API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4-6",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

> Replace `YOUR_API_KEY` with a per-user API key generated from the Web UI at `/_ui/`.

</details>

---

## Client Setup

### OpenCode

<details>
<summary>View OpenCode configuration</summary>

To use this gateway with [OpenCode](https://opencode.ai), add the following provider configuration to `~/.config/opencode/opencode.json`:

#### Tested Model Limits

| Model               | Context (tokens) | Max output tokens | Notes                                             |
| ------------------- | :--------------: | :---------------: | ------------------------------------------------- |
| `claude-opus-4.6`   |      ~195K       |      unknown      | Output probe errored (thinking mode interference) |
| `claude-sonnet-4.6` |      ~195K       |      unknown      | Model stopped early                               |
| `claude-haiku-4.5`  |      ~195K       |      unknown      | Model stopped early                               |

> Output token cap is unknown because the gateway has thinking mode enabled by default. Anthropic's documented standard limit is **8192 tokens** for all Claude 4.x models.

#### Configuration

```json
{
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "kiro": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "Kiro Proxy",
      "options": {
        "baseURL": "https://gateway.example.com/v1",
        "apiKey": "YOUR_API_KEY"
      },
      "models": {
        "auto": {
          "name": "Auto",
          "limit": {
            "context": 195000,
            "output": 8192
          }
        },
        "claude-haiku-4.5": {
          "name": "Claude Haiku 4.5",
          "limit": {
            "context": 195000,
            "output": 8192
          },
          "modalities": {
            "input": ["text", "image"],
            "output": ["text"]
          }
        },
        "claude-sonnet-4.6": {
          "name": "Claude Sonnet 4.6",
          "limit": {
            "context": 195000,
            "output": 8192
          },
          "modalities": {
            "input": ["text", "image"],
            "output": ["text"]
          }
        },
        "claude-opus-4.6": {
          "name": "Claude Opus 4.6",
          "limit": {
            "context": 195000,
            "output": 8192
          },
          "modalities": {
            "input": ["text", "image"],
            "output": ["text"]
          },
          "variants": {
            "low": {
              "thinkingConfig": { "type": "adaptive", "effort": "low" }
            },
            "max": {
              "thinkingConfig": { "type": "adaptive", "effort": "max" }
            }
          }
        }
      }
    }
  }
}
```

> Replace `gateway.example.com` with your domain and `YOUR_API_KEY` with a per-user API key from the Web UI.

</details>

### Claude Code CLI

<details>
<summary>View Claude Code CLI configuration</summary>

https://github.com/user-attachments/assets/f404096e-b326-41e5-a4b3-3f94a73d2ece

To use this gateway with [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code), set the following environment variables:

**One-liner:**

```bash
ANTHROPIC_BASE_URL=https://gateway.example.com ANTHROPIC_AUTH_TOKEN=YOUR_API_KEY CLAUDE_CODE_ENABLE_TELEMETRY=0 DISABLE_PROMPT_CACHING=1 DISABLE_NON_ESSENTIAL_MODEL_CALLS=1 CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 claude
```

**Or add to your shell profile** (`~/.bashrc`, `~/.zshrc`, etc.):

```bash
# Claude Code CLI configuration for Kiro Gateway
export ANTHROPIC_BASE_URL=https://gateway.example.com
export ANTHROPIC_AUTH_TOKEN=YOUR_API_KEY
export CLAUDE_CODE_ENABLE_TELEMETRY=0
export DISABLE_PROMPT_CACHING=1
export DISABLE_NON_ESSENTIAL_MODEL_CALLS=1
export CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1
```

| Variable                                   | Description                                       |
| ------------------------------------------ | ------------------------------------------------- |
| `ANTHROPIC_BASE_URL`                       | Points Claude Code to your gateway                |
| `ANTHROPIC_AUTH_TOKEN`                     | Your per-user API key from the Web UI             |
| `CLAUDE_CODE_ENABLE_TELEMETRY`             | Disable telemetry                                 |
| `DISABLE_PROMPT_CACHING`                   | Disable prompt caching (not supported by gateway) |
| `DISABLE_NON_ESSENTIAL_MODEL_CALLS`        | Reduce unnecessary API calls                      |
| `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` | Disable non-essential network traffic             |

> Replace `gateway.example.com` with your domain and `YOUR_API_KEY` with a per-user API key from the Web UI.

</details>

### Zed Editor

<details>
<summary>View Zed Editor configuration</summary>

To use this gateway with the [Zed Editor](https://zed.dev/)'s ACP Claude Agent, add the following to `~/.config/zed/settings.json`:

```json
{
  "agent_servers": {
    "claude": {
      "env": {
        "ANTHROPIC_BASE_URL": "https://gateway.example.com",
        "ANTHROPIC_AUTH_TOKEN": "YOUR_API_KEY",
        "CLAUDE_CODE_ENABLE_TELEMETRY": "0",
        "DISABLE_PROMPT_CACHING": "1",
        "DISABLE_NON_ESSENTIAL_MODEL_CALLS": "1",
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1"
      }
    }
  }
}
```

> Replace `gateway.example.com` with your domain and `YOUR_API_KEY` with a per-user API key from the Web UI. Restart Zed after updating settings.

</details>

---

## Known Limitations

<details>
<summary>View known limitations</summary>

### Web Search in Claude Code

Claude Code's built-in Web Search tool doesn't work through this proxy. The Kiro backend API doesn't support the `tool_use`/`tool_result` round-trip cycle that Claude Code's native tools rely on, so web search requests will fail.

**Workaround: Use MCP servers instead**

MCP (Model Context Protocol) tools run locally on your machine, so they bypass the proxy entirely. Add the following to your `~/.claude.json` under `"mcpServers"`:

```json
{
  "mcpServers": {
    "fetch": {
      "command": "uvx",
      "args": ["mcp-server-fetch"],
      "env": {},
      "disabled": false,
      "autoApprove": []
    },
    "exa": {
      "command": "npx",
      "args": ["-y", "exa-mcp-server"],
      "env": {
        "EXA_API_KEY": "your-exa-api-key"
      },
      "disabled": false,
      "autoApprove": []
    }
  }
}
```

| MCP Server         | Description                                                            |
| ------------------ | ---------------------------------------------------------------------- |
| `mcp-server-fetch` | Fetches and extracts content from any URL                              |
| `exa-mcp-server`   | AI-powered web search via [Exa](https://exa.ai/) (requires an API key) |

> After adding these, restart Claude Code to pick up the new MCP configuration.

</details>

---

## Building from Source

<details>
<summary>View build instructions</summary>

### Backend (Rust)

```bash
cd backend && cargo build                        # Debug build
cd backend && cargo build --release              # Release build
cd backend && cargo clippy                       # Lint
cd backend && cargo fmt                          # Format
cd backend && cargo test --lib                   # Unit tests
cd backend && cargo test --features test-utils   # Integration tests
```

### Frontend (React)

```bash
cd frontend && npm install    # Install dependencies
cd frontend && npm run build  # Production build (tsc -b && vite build)
cd frontend && npm run lint   # ESLint
cd frontend && npm run dev    # Dev server (port 5173, proxies /_ui/api → localhost:8000)
```

### Docker

```bash
docker compose build    # Build all images
docker compose up -d    # Start all services
```

</details>

---

## License

This project is licensed under the **GNU Affero General Public License v3.0 (AGPL-3.0)**.

This means:

- You can use, modify, and distribute this software
- You can use it for commercial purposes
- **You must disclose source code** when you distribute the software
- **Network use is distribution** — if you run a modified version on a server, you must make the source code available
- Modifications must be released under the same license

See the [LICENSE](LICENSE) file for the full license text.

### Contributor License Agreement (CLA)

By submitting a contribution to this project, you agree to the terms of our [Contributor License Agreement (CLA)](CLA.md).

---

## Disclaimer

This project is not affiliated with, endorsed by, or sponsored by Amazon Web Services (AWS), Anthropic, or Kiro IDE. Use at your own risk and in compliance with the terms of service of the underlying APIs.

---

<div align="center">

**[Back to Top](#rkgw--rust-kiro-gateway)**

</div>
