---
layout: default
title: Getting Started
nav_order: 2
---

# Getting Started
{: .no_toc }

This guide walks you through setting up Harbangan for the first time. By the end, you will have a working gateway that translates OpenAI and Anthropic API calls into Kiro (AWS CodeWhisperer) backend requests.

<details open markdown="block">
  <summary>Table of contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## What is Harbangan?

Harbangan is a proxy server that exposes industry-standard OpenAI (`/v1/chat/completions`) and Anthropic (`/v1/messages`) endpoints, translating every request into the Kiro API format used by AWS CodeWhisperer. Any tool or library that speaks the OpenAI or Anthropic protocol can use Kiro models without modification.

Key capabilities:

- Bidirectional format translation (OpenAI/Anthropic to Kiro and back)
- Streaming responses via Server-Sent Events (SSE)
- Two deployment modes: **Proxy-Only Mode** (single user) and **Full Deployment** (multi-user)
- Multi-provider support: Kiro (default), GitHub Copilot, and Qwen Coder — with per-user provider priority
- Multi-user support with Google SSO and per-user API keys (Full Deployment)
- Role-based access control (Admin / User)
- Web dashboard for configuration, monitoring, and log streaming
- Content guardrails via AWS Bedrock with CEL rule engine
- MCP Gateway for connecting external tool servers
- Per-user credential management with automatic token refresh
- Model alias resolution (use familiar model names like `claude-sonnet-4`)

---

## Choose Your Deployment Mode

Harbangan supports two deployment modes:

| | Proxy-Only Mode | Full Deployment |
|:---|:---|:---|
| **Docker Compose file** | `docker-compose.gateway.yml` | `docker-compose.yml` |
| **Containers** | 1 (backend only) | 3 (backend, db, frontend) |
| **Authentication** | Single `PROXY_API_KEY` | Per-user API keys + Google SSO |
| **Kiro credentials** | Device code flow on first boot | Per-user via Web UI |
| **Database** | None | PostgreSQL |
| **Web UI** | No | Yes |
| **Best for** | Personal use, quick evaluation | Teams, development |

---

## Proxy-Only Mode

The fastest way to get started. Runs a single backend container with no database, web UI, or TLS.

### Prerequisites

| Requirement | Minimum version | How to check |
|:---|:---|:---|
| Docker | 20.10+ | `docker --version` |
| Docker Compose | 2.0+ (V2 plugin) | `docker compose version` |

You also need an **AWS Builder ID** (free) or **Identity Center** (pro) account for Kiro API access.

### Step 1: Clone the repository

```bash
git clone https://github.com/if414013/harbangan.git
cd harbangan
```

### Step 2: Configure environment variables

Create `.env.proxy`:

```bash
PROXY_API_KEY=your-secret-api-key
KIRO_REGION=us-east-1
# For Identity Center (pro): set your SSO URL
# KIRO_SSO_URL=https://your-org.awsapps.com/start
# KIRO_SSO_REGION=us-east-1
```

### Step 3: Start the gateway

```bash
docker compose -f docker-compose.gateway.yml --env-file .env.proxy up
```

On first boot, the container runs an AWS SSO device code flow. Check the logs for a URL to open in your browser:

```
╔═══════════════════════════════════════════════════════════╗
║  Open this URL in your browser to authorize:             ║
║  https://device.sso.us-east-1.amazonaws.com/?user_code=… ║
╚═══════════════════════════════════════════════════════════╝
```

Open the URL, sign in with your Builder ID (free) or Identity Center (pro) account, and authorize the gateway. Credentials are cached in a Docker volume (`gateway-data`) — you only need to authorize once. On subsequent restarts, the gateway reuses the cached tokens automatically.

### Step 4: Verify it works

```bash
# Health check
curl http://localhost:8000/health
# → {"status":"ok"}

# Test a chat completion
curl http://localhost:8000/v1/chat/completions \
  -H "Authorization: Bearer your-secret-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4-6",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

You should see a streaming SSE response with the model's reply.

{: .note }
> Proxy-Only Mode uses `http://localhost:8000` by default.

---

## Full Deployment

Multi-user mode with PostgreSQL, Google SSO, per-user API keys, and web dashboard.

### Prerequisites

| Requirement | Minimum version | How to check |
|:---|:---|:---|
| Docker | 20.10+ | `docker --version` |
| Docker Compose | 2.0+ (V2 plugin) | `docker compose version` |

You also need:

- **Google OAuth credentials** (Client ID + Client Secret) from the [Google Cloud Console](https://console.cloud.google.com/apis/credentials). Create an OAuth 2.0 Client ID with the authorized redirect URI set to `http://localhost:9999/_ui/api/auth/google/callback`.

### Installation

The Full Deployment runs via docker-compose with three services: PostgreSQL, Rust backend, and Vite frontend dev server.

### Step 1: Clone the repository

```bash
git clone https://github.com/if414013/harbangan.git
cd harbangan
```

### Step 2: Configure environment variables

```bash
cp .env.example .env
```

Edit `.env` and fill in all values:

```bash
# PostgreSQL password
POSTGRES_PASSWORD=your_secure_password_here

# Google SSO (required)
GOOGLE_CLIENT_ID=your-google-client-id.apps.googleusercontent.com
GOOGLE_CLIENT_SECRET=your-google-client-secret
GOOGLE_CALLBACK_URL=http://localhost:9999/_ui/api/auth/google/callback

# GitHub Copilot OAuth (optional)
# GITHUB_COPILOT_CLIENT_ID=
# GITHUB_COPILOT_CLIENT_SECRET=
# GITHUB_COPILOT_CALLBACK_URL=https://gateway.example.com/_ui/api/copilot/callback

# Qwen Coder OAuth (optional — device flow, no secret required)
# QWEN_OAUTH_CLIENT_ID=f0304373b74a44d2b584a3fb70ca9e56
```

### Step 3: Start all services

```bash
docker compose up -d --build
```

The first build compiles the Rust backend and React frontend inside Docker, which takes a few minutes. Subsequent builds are much faster.

Watch the logs:

```bash
docker compose logs -f backend
```

Wait until you see:

```
Setup not complete — starting in setup-only mode
Server listening on http://0.0.0.0:9999
```

---

## First-Time Setup Wizard

On first launch, the gateway starts in **setup-only mode**. The `/v1/*` proxy endpoints return 503 until you complete setup through the Web UI.

### Step 1: Open the Web UI

Navigate to `http://localhost:5173/_ui/` in your browser.

### Step 2: Sign in with Google

Click **Sign in with Google** to authenticate via Google SSO. The first user to sign in is automatically granted the **Admin** role.

### Step 3: Add provider credentials

After signing in, navigate to the **Profile** page to connect your AI provider accounts. The gateway supports multiple providers:

- **Kiro (AWS)** — Default provider. Uses an OAuth device code flow to authenticate with AWS SSO and store a refresh token.
- **GitHub Copilot** (optional) — Connect via GitHub OAuth. Requires `GITHUB_COPILOT_CLIENT_ID`, `GITHUB_COPILOT_CLIENT_SECRET`, and `GITHUB_COPILOT_CALLBACK_URL` in `.env`.
- **Qwen Coder** (optional) — Connect via device code flow. Requires `QWEN_OAUTH_CLIENT_ID` in `.env` (a default public client ID is provided).

Each user manages their own provider credentials and can set a priority order for provider fallback.

### Step 4: Create an API key

Navigate to the API Keys section in the Web UI and create a personal API key. This key is what you'll use in `Authorization: Bearer <key>` headers when making API calls.

### Step 5: Invite users (optional)

As an admin, you can manage users and roles from the Web UI. Additional users sign in via Google SSO and can be granted Admin or User roles.

---

## Setup Flow Diagram

```mermaid
sequenceDiagram
    participant User as User / Browser
    participant FE as Frontend (Vite)
    participant GW as Backend API
    participant DB as PostgreSQL
    participant Google as Google SSO

    Note over GW: First launch — setup mode
    GW->>DB: Connect & create tables
    DB-->>GW: Ready

    User->>FE: Open http://localhost:5173/_ui/
    FE->>GW: Proxy request
    GW-->>User: Login page

    User->>Google: Sign in with Google (PKCE)
    Google-->>GW: Authorization code
    GW->>DB: Create admin user
    GW-->>User: Session cookie — redirect to dashboard

    User->>GW: Add provider credentials (Kiro, Copilot, Qwen)
    GW->>DB: Save refresh tokens

    User->>GW: Create personal API key
    GW->>DB: Save API key (SHA-256 hashed)

    Note over GW: Setup complete — normal operation

    User->>GW: POST /v1/chat/completions
    GW->>GW: Validate API key → find user → get provider creds
    GW->>GW: Convert OpenAI → provider format
    GW-->>User: SSE stream (Kiro → OpenAI format)
```

---

## Verifying the Installation

Once setup is complete, verify that everything is working.

### Health check

```bash
curl http://localhost:9999/health
```

Expected response:

```json
{"status":"ok"}
```

### List available models

```bash
curl -H "Authorization: Bearer YOUR_API_KEY" \
  http://localhost:9999/v1/models
```

### Send a test chat request (OpenAI format)

```bash
curl -X POST http://localhost:9999/v1/chat/completions \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4",
    "messages": [
      {"role": "user", "content": "Hello! What can you do?"}
    ],
    "stream": true
  }'
```

### Send a test chat request (Anthropic format)

```bash
curl -X POST http://localhost:9999/v1/messages \
  -H "x-api-key: YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -d '{
    "model": "claude-sonnet-4",
    "max_tokens": 1024,
    "messages": [
      {"role": "user", "content": "Hello! What can you do?"}
    ],
    "stream": true
  }'
```

### Check the Web UI dashboard

Open `http://localhost:9999/_ui/` to see:

- Profile page with provider credential management (Kiro, Copilot, Qwen)
- Configuration management (admin-only)
- MCP client management (admin-only)
- Content guardrails configuration (admin-only)
- User and API key management (admin-only)

---

## Connecting AI Tools

Once the gateway is running, point your AI tools at it using your personal API key.

### Cursor / VS Code extensions

Set the API base URL to your gateway:

```
http://localhost:9999/v1
```

Use your personal API key as the API key.

### OpenAI Python SDK

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:9999/v1",
    api_key="YOUR_API_KEY",
)

response = client.chat.completions.create(
    model="claude-sonnet-4",
    messages=[{"role": "user", "content": "Hello!"}],
)
print(response.choices[0].message.content)
```

### Anthropic Python SDK

```python
import anthropic

client = anthropic.Anthropic(
    base_url="http://localhost:9999",
    api_key="YOUR_API_KEY",
)

message = client.messages.create(
    model="claude-sonnet-4",
    max_tokens=1024,
    messages=[{"role": "user", "content": "Hello!"}],
)
print(message.content[0].text)
```

---

## Next Steps

- [Quickstart](quickstart.html) — Get running in under 5 minutes with Docker
- [Configuration Reference](configuration.html) — Environment variables and runtime settings for both modes
- [Deployment Guide](deployment.html) — Production deployment for Proxy-Only Mode and Full Deployment
