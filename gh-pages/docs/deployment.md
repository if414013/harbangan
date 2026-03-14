---
layout: default
title: Deployment
nav_order: 8
---

# Deployment Guide
{: .no_toc }

Deployment instructions for Harbangan. Covers both Proxy-Only Mode (single container) and Full Deployment (multi-user with Docker Compose).
{: .fs-6 .fw-300 }

<details open markdown="block">
  <summary>Table of contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Deployment Modes

Harbangan supports two deployment modes:

- **Proxy-Only Mode** — A single backend container with no database or web UI. Uses `docker-compose.gateway.yml`. Best for personal use or quick evaluation.
- **Full Deployment** — Three containers (backend, PostgreSQL, frontend) with Google SSO, per-user API keys, and web dashboard. Uses `docker-compose.yml`. Best for teams and development. Production deployment targets Kubernetes with TLS handled by an Ingress controller.

---

## Proxy-Only Mode Deployment

### Architecture

```
Client (localhost) → gateway container (127.0.0.1:8000, plain HTTP)
                         └── Kiro API (AWS CodeWhisperer)
```

A single container running the Rust backend. No database or web UI. Authentication uses a single `PROXY_API_KEY` environment variable. Kiro credentials are obtained via an AWS SSO device code flow on first boot and cached to a Docker volume. The port binds to `127.0.0.1` only — not accessible from external networks. The container runs as a non-root user (`appuser`) with a 512MB memory limit.

| Service | Image | Purpose |
|:---|:---|:---|
| `gateway` | `harbangan-backend:latest` (built locally) | Rust API server on configurable port (default 8000) |

### Prerequisites

- Docker Engine 20.10+ and Docker Compose v2
- An AWS **Builder ID** (free) or **Identity Center** (pro) account

### Step 1: Clone the Repository

```bash
git clone https://github.com/if414013/harbangan.git
cd harbangan
```

### Step 2: Configure Environment Variables

Create `.env.proxy`:

```bash
PROXY_API_KEY=your-secret-api-key
KIRO_REGION=us-east-1
# For Identity Center (pro): set your SSO URL
# KIRO_SSO_URL=https://your-org.awsapps.com/start
# KIRO_SSO_REGION=us-east-1
```

See the [Configuration Reference](configuration.html#proxy-only-mode-environment-variables) for all available variables.

### Step 3: Start the Gateway

```bash
docker compose -f docker-compose.gateway.yml --env-file .env.proxy up -d
```

On first boot, the container runs an AWS SSO device code flow. Check the logs:

```bash
docker compose -f docker-compose.gateway.yml logs -f gateway
```

You'll see a URL and user code to authorize in your browser. After authorization, credentials are cached in the `gateway-data` Docker volume and reused on subsequent restarts.

### Step 4: Verify

```bash
curl http://localhost:8000/health
# Expected: {"status":"ok"}

curl http://localhost:8000/v1/chat/completions \
  -H "Authorization: Bearer your-secret-api-key" \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-6","messages":[{"role":"user","content":"Hello!"}],"max_tokens":50}'
```

### Proxy-Only Mode Operations

```bash
# View logs
docker compose -f docker-compose.gateway.yml logs -f gateway

# Stop the gateway
docker compose -f docker-compose.gateway.yml down

# Restart (reuses cached credentials)
docker compose -f docker-compose.gateway.yml --env-file .env.proxy up -d

# Rebuild after code changes
docker compose -f docker-compose.gateway.yml --env-file .env.proxy up -d --build

# Re-authorize (clear cached credentials)
docker volume rm harbangan_gateway-data
docker compose -f docker-compose.gateway.yml --env-file .env.proxy up
```

### Proxy-Only Volume Layout

| Volume | Type | Purpose |
|:---|:---|:---|
| `gateway-data` | Named volume | Cached Kiro credentials (`/data/tokens.json`) |

---

## Full Deployment

### Architecture

The Full Deployment runs via Docker Compose with three services:

```mermaid
graph LR
    subgraph Internet
        client1["OpenAI Client<br/>(Python/Node.js)"]
        client2["Anthropic Client"]
        client3["Web Browser"]
    end

    subgraph Docker["Docker Compose Stack"]
        subgraph frontend_container["frontend (Vite)"]
            vite["Vite Dev Server<br/>:5173"]
        end
        subgraph backend_container["backend"]
            gw["Rust API Server<br/>:9999 (HTTP)"]
        end
        subgraph db_container["db"]
            pg["PostgreSQL 16<br/>:5432"]
        end
    end

    subgraph AWS["AWS"]
        kiro["Kiro API<br/>(CodeWhisperer)"]
        sso["AWS SSO OIDC"]
    end

    client1 -->|"HTTP /v1/*"| gw
    client2 -->|"HTTP /v1/*"| gw
    client3 -->|"HTTP /_ui/"| vite

    vite -->|"Proxy /_ui/api"| gw
    gw -->|"HTTPS Event Stream"| kiro
    gw -->|"OAuth token refresh"| sso
    gw -->|"Config + credentials"| pg

    style vite fill:#4a9eff,color:#fff
    style gw fill:#4a9eff,color:#fff
    style pg fill:#336791,color:#fff
    style kiro fill:#ff9900,color:#fff
    style sso fill:#ff9900,color:#fff
```

| Service | Image | Purpose |
|:---|:---|:---|
| `db` | `postgres:16-alpine` | PostgreSQL database for config, credentials, and user data |
| `backend` | `harbangan-backend:latest` (built locally) | Rust API server — plain HTTP on port 9999 |
| `frontend` | `harbangan-frontend:latest` (built locally) | Vite dev server — serves React SPA, proxies API requests to backend |

> **Note:** This Docker Compose setup is intended for development. Production deployment targets Kubernetes, where TLS is handled by an Ingress controller.

---

## Prerequisites

- Docker Engine 20.10+ and Docker Compose v2
- **Google OAuth credentials** (Client ID + Client Secret) from the [Google Cloud Console](https://console.cloud.google.com/apis/credentials)
- At least 1 GB RAM and 2 GB disk space

---

## Step 1: Clone the Repository

```bash
git clone https://github.com/if414013/harbangan.git
cd harbangan
```

## Step 2: Configure Environment Variables

```bash
cp .env.example .env
```

Edit `.env`:

```bash
# PostgreSQL password
POSTGRES_PASSWORD=your_secure_password_here

# Google SSO (required for Web UI authentication)
GOOGLE_CLIENT_ID=your-client-id.apps.googleusercontent.com
GOOGLE_CLIENT_SECRET=your-client-secret
GOOGLE_CALLBACK_URL=http://localhost:9999/_ui/api/auth/google/callback

# GitHub Copilot OAuth (optional)
# GITHUB_COPILOT_CLIENT_ID=
# GITHUB_COPILOT_CLIENT_SECRET=
# GITHUB_COPILOT_CALLBACK_URL=https://gateway.example.com/_ui/api/copilot/callback

# Qwen Coder OAuth (optional — device flow, no secret required)
# QWEN_OAUTH_CLIENT_ID=f0304373b74a44d2b584a3fb70ca9e56
```

The following are managed automatically by `docker-compose.yml` — do **not** set them in `.env`:

- `SERVER_HOST` — set to `0.0.0.0` for the backend
- `SERVER_PORT` — set to `9999` for the backend
- `DATABASE_URL` — constructed from `POSTGRES_PASSWORD`

## Step 3: Build and Start

```bash
docker compose up -d --build
```

The first build compiles the Rust backend and React frontend, which takes a few minutes. Subsequent builds are fast unless dependencies change.

Watch the logs to confirm startup:

```bash
docker compose logs -f
```

## Step 4: Complete Web UI Setup

On first launch, the backend starts in **setup-only mode** — the `/v1/*` proxy endpoints return 503 until an admin completes setup.

Open `http://localhost:5173/_ui/` and:

1. **Sign in with Google** — the first user is automatically granted the Admin role
2. **Add provider credentials** — connect Kiro (AWS SSO device code flow), and optionally GitHub Copilot or Qwen Coder on the Profile page
3. **Create an API key** — generate a personal API key for programmatic access

## Step 5: Verify

```bash
# Health check
curl http://localhost:9999/health
# Expected: {"status":"ok"}

# List models (use your personal API key)
curl -H "Authorization: Bearer YOUR_API_KEY" \
  http://localhost:9999/v1/models

# Test a chat completion
curl -X POST http://localhost:9999/v1/chat/completions \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4","messages":[{"role":"user","content":"Hello!"}],"max_tokens":50}'
```

---

## Docker Compose File Reference

The `docker-compose.yml` defines three services:

```yaml
services:
  db:
    image: postgres:16-alpine
    volumes:
      - pgdata:/var/lib/postgresql/data

  backend:
    build: ./backend
    ports:
      - "9999:9999"
    environment:
      SERVER_HOST: "0.0.0.0"
      SERVER_PORT: "9999"
      DATABASE_URL: postgres://kiro:${POSTGRES_PASSWORD}@db:5432/kiro_gateway
    depends_on:
      db: { condition: service_healthy }

  frontend:
    build: ./frontend
    ports:
      - "5173:5173"
    depends_on:
      backend: { condition: service_healthy }
```

### Volume Layout

| Volume | Type | Purpose |
|:---|:---|:---|
| `pgdata` | Named volume | PostgreSQL data (users, credentials, config, history) |

---

## Day-to-Day Operations

```bash
# View live logs
docker compose logs -f

# Check container health (should show "healthy" after ~30s)
docker compose ps

# Stop the stack
docker compose down

# Rebuild after code changes
docker compose up -d --build

# Restart without rebuild
docker compose restart backend

# View backend logs only
docker compose logs -f backend
```

### Database Backup

```bash
# Dump the database
docker compose exec db pg_dump -U kiro kiro_gateway > backup.sql

# Restore from backup
docker compose exec -T db psql -U kiro kiro_gateway < backup.sql
```

---

## PostgreSQL

The gateway uses PostgreSQL for persistent storage of:

- User accounts and roles
- Per-user Kiro credentials (refresh tokens)
- Per-user API keys (SHA-256 hashed)
- Runtime configuration
- Configuration change history

### Database tables

Tables are created automatically on first connection. Key tables include:

| Table | Purpose |
|:---|:---|
| `users` | User accounts (Google SSO identity, role, status) |
| `api_keys` | Per-user API keys (SHA-256 hashed, with labels) |
| `user_kiro_credentials` | Per-user Kiro refresh tokens |
| `user_provider_credentials` | Per-user provider credentials (Copilot, Qwen) |
| `user_provider_priority` | Per-user provider priority ordering |
| `config` | Key-value configuration store |
| `config_history` | Audit log of configuration changes |
| `guardrail_profiles` | AWS Bedrock guardrail profiles (credentials encrypted) |
| `guardrail_rules` | Guardrail rules (CEL expressions, sampling, timeouts) |

### Connection string

The `DATABASE_URL` is constructed by docker-compose from `POSTGRES_PASSWORD`:

```
postgres://kiro:<POSTGRES_PASSWORD>@db:5432/kiro_gateway
```

---

## Health Monitoring

### Health check endpoint

```bash
curl http://localhost:9999/health
```

Returns `200 OK` with:

```json
{"status":"ok"}
```

### Docker health checks

All services include built-in health checks:

```bash
docker compose ps
# NAME               SERVICE    STATUS          PORTS
# harbangan-db-1          db         Up (healthy)    5432/tcp
# harbangan-backend-1     backend    Up (healthy)    0.0.0.0:9999->9999/tcp
# harbangan-frontend-1    frontend   Up (healthy)    0.0.0.0:5173->5173/tcp
```

### Web UI metrics

The Web UI at `/_ui/` provides real-time monitoring:

- Active connections and total requests
- Latency percentiles (p50, p95, p99)
- Per-model statistics and error breakdown
- Live log streaming via SSE

### Log access

```bash
# All services
docker compose logs -f

# Backend only
docker compose logs -f backend

# Frontend only
docker compose logs -f frontend
```

The backend uses structured logging via `tracing`:

```
INFO kiro_gateway::routes: Request to /v1/chat/completions: model=claude-sonnet-4, stream=true, messages=3
```

---

## Datadog APM (Optional)

Both deployment modes support an optional Datadog Agent sidecar for distributed tracing, metrics, log forwarding, and frontend RUM. The integration is zero-overhead when not configured — when `DD_AGENT_HOST` is unset, no Datadog code runs.

### Step 1: Configure Datadog environment variables

Add to your `.env` (Full Deployment) or `.env.proxy` (Proxy-Only):

```bash
DD_API_KEY=your-datadog-api-key
DD_SITE=datadoghq.com   # or datadoghq.eu, us3.datadoghq.com, etc.
DD_ENV=production
```

For frontend Real User Monitoring (RUM), set these **before building** the frontend image:

```bash
VITE_DD_CLIENT_TOKEN=your-rum-client-token
VITE_DD_APPLICATION_ID=your-rum-application-id
VITE_DD_ENV=production
```

| Variable | Required | Default | Description |
|:---|:---|:---|:---|
| `DD_API_KEY` | Yes | | Datadog API key |
| `DD_SITE` | No | `datadoghq.com` | Datadog intake site |
| `DD_ENV` | No | | Environment tag (e.g. `production`, `staging`) |
| `VITE_DD_CLIENT_TOKEN` | No | | RUM client token (baked into frontend bundle at build time) |
| `VITE_DD_APPLICATION_ID` | No | | RUM application ID (baked into frontend bundle at build time) |

### Step 2: Start with the Datadog profile

Add `--profile datadog` to your compose command:

```bash
# Full Deployment
docker compose --profile datadog up -d

# Proxy-Only
docker compose -f docker-compose.gateway.yml --profile datadog --env-file .env.proxy up -d
```

The `datadog-agent` service starts alongside the gateway and receives traces via OTLP on port 4317. `DD_AGENT_HOST` is set automatically by docker-compose.

### Step 3: Verify

```bash
# Check agent is running
docker compose ps datadog-agent

# Check agent logs for connectivity
docker compose logs datadog-agent | grep -i "connected\|error"
```

Traces appear in your Datadog APM dashboard within ~30 seconds of the first request.

**What you'll see in Datadog:**
- Distributed traces for every `/v1/*` request with model, user, and latency breakdown
- Metrics: request rate, error rate, latency percentiles, token usage (per model and user)
- Logs correlated to traces via injected `dd.trace_id` / `dd.span_id` fields
- Frontend RUM sessions linked to backend traces (if `VITE_DD_*` vars are set at build time)

See the [Configuration Reference](configuration.html#datadog-apm-environment-variables) for all Datadog variables and the [Architecture docs](../architecture/#observability-datadog-apm) for implementation details.

---

## Next Steps

- [Configuration Reference](configuration.html) — Environment variables for both Proxy-Only Mode and Full Deployment
- [Getting Started](getting-started.html) — Full setup walkthrough with both deployment modes
