---
layout: default
title: Quickstart
nav_order: 3
---

# Quickstart
{: .no_toc }

Get Kiro Gateway running and make your first API call in under 5 minutes using Docker.

<details open markdown="block">
  <summary>Table of contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## 1. Clone and configure

```bash
git clone https://github.com/if414013/rkgw.git
cd rkgw
cp .env.example .env
```

Edit `.env` and set a strong password:

```bash
PROXY_API_KEY=my-super-secret-key-change-me
KIRO_REGION=us-east-1
```

## 2. Start with Docker Compose

```bash
docker compose up -d --build
```

This starts PostgreSQL and the gateway. The first build takes a few minutes (Rust compilation + React build). Watch the logs:

```bash
docker compose logs -f gateway
```

Wait until you see:

```
Setup not complete — starting in setup-only mode
Server listening on https://0.0.0.0:9001
```

## 3. Complete setup via Web UI

Open `https://localhost:9001/_ui/` in your browser (accept the self-signed certificate warning).

The setup wizard walks you through the OAuth device code flow:

1. Enter your **gateway password**, **AWS SSO Start URL**, and **region**
2. Click start — you'll get a **user code** and a **verification URL**
3. Open the verification URL, enter the code, and authorize
4. The gateway detects authorization and saves your credentials

Once complete, the dashboard appears and the gateway is fully operational.

## 4. Verify it works

```bash
# Health check
curl -k https://localhost:9001/health
# → {"status":"ok"}

# List models
curl -k -H "Authorization: Bearer my-super-secret-key-change-me" \
  https://localhost:9001/v1/models
```

## 5. Make your first API call

### OpenAI format

```bash
curl -k -X POST https://localhost:9001/v1/chat/completions \
  -H "Authorization: Bearer my-super-secret-key-change-me" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4",
    "messages": [
      {"role": "user", "content": "Write a haiku about coding"}
    ],
    "stream": true
  }'
```

### Anthropic format

```bash
curl -k -X POST https://localhost:9001/v1/messages \
  -H "x-api-key: my-super-secret-key-change-me" \
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

- [Getting Started](getting-started.html) — Full installation guide with build-from-source instructions, OAuth flow details, and SDK integration examples
- [Configuration Reference](configuration.html) — All environment variables, CLI arguments, TLS setup, and tuning options
