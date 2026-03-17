---
layout: default
title: Quickstart
nav_order: 3
---

# Quickstart
{: .no_toc }

Get Harbangan running and make your first API call in under 5 minutes using Docker. Choose the mode that fits your needs:

- **Proxy-Only Mode** — Single container, single API key, no database or web UI. Best for personal use or quick evaluation.
- **Full Deployment** — Multi-user with Google SSO, per-user API keys, and web dashboard. Best for teams.

<details open markdown="block">
  <summary>Table of contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Proxy-Only Mode (Fastest)

A single container with no PostgreSQL or Google SSO. Authenticates via a single `PROXY_API_KEY`. **Supports Kiro (AWS CodeWhisperer) only** — no Copilot or Qwen in this mode.

### 1. Clone and configure

```bash
git clone https://github.com/if414013/harbangan.git
cd harbangan
```

Create `.env.proxy` (copy from `.env.proxy.example`):

```bash
GATEWAY_MODE=proxy
PROXY_API_KEY=your-secret-api-key

# Optional — defaults to us-east-1:
# KIRO_REGION=us-east-1

# For Identity Center (pro): set your SSO start URL
# KIRO_SSO_URL=https://your-org.awsapps.com/start
# KIRO_SSO_REGION=us-east-1
```

### 2. Start the gateway

```bash
docker compose -f docker-compose.gateway.yml --env-file .env.proxy up -d
```

On first boot, the container runs a device code flow — check the logs for a URL to open in your browser:

```
╔═══════════════════════════════════════════════════════════╗
║  Open this URL in your browser to authorize:             ║
║  https://device.sso.us-east-1.amazonaws.com/?user_code=… ║
╚═══════════════════════════════════════════════════════════╝
```

Credentials are cached in a Docker volume — you only need to authorize once. On subsequent restarts, the gateway reuses the cached tokens automatically.

### 3. Make your first API call

The gateway starts on port 8000 and authenticates requests with your `PROXY_API_KEY`:

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

That's it — you're running.

---

## Full Deployment (Multi-User)

Multi-user mode with PostgreSQL, Google SSO, per-user API keys, and web dashboard.

### 1. Clone and configure

```bash
git clone https://github.com/if414013/harbangan.git
cd harbangan
cp .env.example .env
```

Edit `.env` and fill in your values:

```bash
POSTGRES_PASSWORD=change-me
GOOGLE_CLIENT_ID=your-client-id.apps.googleusercontent.com
GOOGLE_CLIENT_SECRET=your-client-secret
GOOGLE_CALLBACK_URL=http://localhost:9999/_ui/api/auth/google/callback
```

You need a **Google OAuth Client ID** from the [Google Cloud Console](https://console.cloud.google.com/apis/credentials) with the redirect URI set to your callback URL above.

### 2. Start with Docker Compose

```bash
docker compose up -d --build
```

This starts three services: PostgreSQL, the Rust backend, and the Vite frontend. The first build takes a few minutes.

Watch the logs:

```bash
docker compose logs -f backend
```

Wait until you see:

```
Setup not complete — starting in setup-only mode
Server listening on http://0.0.0.0:8000
```

### 4. Complete setup via Web UI

Open `https://your-domain.com/_ui/` in your browser.

1. Click **Sign in with Google** — the first user gets the Admin role
2. Connect your **Kiro credentials** on the Profile page via AWS SSO device code flow
3. Create a **personal API key** in the API Keys section

### 5. Verify it works

```bash
# Health check
curl https://your-domain.com/health
# → {"status":"ok"}

# List models
curl -H "Authorization: Bearer YOUR_API_KEY" \
  https://your-domain.com/v1/models
```

### 6. Make your first API call

#### OpenAI format

```bash
curl -X POST https://your-domain.com/v1/chat/completions \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4",
    "messages": [
      {"role": "user", "content": "Write a haiku about coding"}
    ],
    "stream": true
  }'
```

#### Anthropic format

```bash
curl -X POST https://your-domain.com/v1/messages \
  -H "x-api-key: YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -d '{
    "model": "claude-sonnet-4",
    "max_tokens": 1024,
    "messages": [
      {"role": "user", "content": "Write a haiku about coding"}
    ],
    "stream": true
  }'
```

You should see a streaming SSE response with the model's reply.

---

## What's next?

- [Getting Started](getting-started.html) — Full walkthrough with Google OAuth setup, Kiro credential flow, and SDK integration examples
- [Configuration Reference](configuration.html) — Environment variables and runtime settings
- [Deployment Guide](deployment.html) — Production deployment for both modes
