# Docker Deployment Runbook

This guide covers both deployment modes for the Kiro Gateway.

| Mode | Compose File | Services | Use Case |
|------|-------------|----------|----------|
| **Proxy-Only Mode** | `docker-compose.gateway.yml` | 1 (gateway) | Single-user, no DB/SSO needed |
| **Full Deployment** | `docker-compose.yml` | 4 (db, backend, frontend, certbot) | Multi-user with Web UI, Google SSO, TLS |
| **+ Datadog APM** | either + `--profile datadog` | +1 (datadog-agent) | Optional observability sidecar |

---

## Proxy-Only Mode

A lightweight single-container deployment. No PostgreSQL, no nginx, no Google SSO — just the gateway proxying requests to Kiro with a shared API key.

### Prerequisites

- Docker and Docker Compose installed
- No domain or TLS certificates required

### 1. Configure environment variables

Create `.env.proxy`:

```env
PROXY_API_KEY=your-secret-api-key
KIRO_REGION=us-east-1
# For Identity Center (pro): set your SSO URL and region
# KIRO_SSO_URL=https://your-org.awsapps.com/start
# KIRO_SSO_REGION=us-east-1
```

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `PROXY_API_KEY` | Yes | | API key clients use to authenticate |
| `KIRO_REGION` | No | `us-east-1` | Kiro API region |
| `KIRO_SSO_URL` | No | | Identity Center URL (omit for Builder ID) |
| `KIRO_SSO_REGION` | No | same as `KIRO_REGION` | AWS SSO OIDC region |
| `SERVER_PORT` | No | `8000` | Listen port |
| `LOG_LEVEL` | No | `info` | `debug`, `info`, `warn`, `error` |
| `DEBUG_MODE` | No | `off` | `off`, `errors`, `all` |

**SSO modes:**
- **Builder ID (free):** Omit `KIRO_SSO_URL`. Uses the default AWS Builder ID flow.
- **Identity Center (pro):** Set `KIRO_SSO_URL` to your organization's AWS SSO start URL.

### 2. Start the gateway

```bash
docker compose -f docker-compose.gateway.yml --env-file .env.proxy up -d
```

### 3. Authorize on first boot

On first boot (or when cached credentials expire), the container runs an AWS SSO OIDC device code flow. Check the logs for a URL to open in your browser:

```bash
docker compose -f docker-compose.gateway.yml logs -f
```

```
╔═══════════════════════════════════════════════════════════╗
║  Open this URL in your browser to authorize:             ║
║  https://device.sso.us-east-1.amazonaws.com/?user_code=… ║
╚═══════════════════════════════════════════════════════════╝
```

The entrypoint script (`backend/entrypoint.sh`) handles the full flow:

1. **Register OIDC client** — registers a public client with Kiro scopes at `oidc.{region}.amazonaws.com`
2. **Device authorization** — requests a device code and displays the verification URL
3. **Poll for token** — polls until you authorize in the browser (or the code expires)
4. **Cache credentials** — saves the refresh token, client ID, and client secret to `/data/tokens.json`

### 4. Verify

```bash
curl http://localhost:8000/health
# → {"status":"ok"}

curl http://localhost:8000/v1/chat/completions \
  -H "Authorization: Bearer your-secret-api-key" \
  -H "Content-Type: application/json" \
  -d '{"model": "claude-sonnet-4-6", "messages": [{"role": "user", "content": "Hello!"}]}'
```

### Token caching and reuse

Credentials are cached in the `gateway-data` Docker volume at `/data/tokens.json`. On subsequent restarts:

1. The entrypoint loads cached tokens
2. Validates them with a test refresh against AWS SSO OIDC
3. If valid, starts the gateway immediately (no browser authorization needed)
4. If expired or invalid, runs the device code flow again

To force re-authorization, remove the volume:

```bash
docker compose -f docker-compose.gateway.yml down -v
docker compose -f docker-compose.gateway.yml --env-file .env.proxy up -d
```

---

## Full Deployment

Multi-user deployment with PostgreSQL, Google SSO, Web UI, and automated TLS via Let's Encrypt.

### Prerequisites

- Docker and Docker Compose installed on the VPS
- A domain name pointing to your server (for Let's Encrypt TLS)
- [Google OAuth credentials](https://console.cloud.google.com/apis/credentials) (Client ID + Secret)
- The repository cloned on the server at `/path/to/rkgw`

### 1. Configure environment variables

```bash
cp .env.example .env
# Edit .env — set all required values
```

| Variable | Required | Description |
|----------|----------|-------------|
| `DOMAIN` | Yes | Domain for Let's Encrypt TLS certs |
| `EMAIL` | Yes | Let's Encrypt notification email |
| `POSTGRES_PASSWORD` | Yes | PostgreSQL password |
| `GOOGLE_CLIENT_ID` | Yes | Google OAuth Client ID |
| `GOOGLE_CLIENT_SECRET` | Yes | Google OAuth Client Secret |
| `GOOGLE_CALLBACK_URL` | Yes | OAuth callback (e.g. `https://$DOMAIN/_ui/api/auth/google/callback`) |

Do **not** set `SERVER_HOST`, `DATABASE_URL`, or `SERVER_PORT` in `.env` — these are managed by `docker-compose.yml`.

### 2. Provision TLS certificates

```bash
chmod +x init-certs.sh
DOMAIN=gateway.example.com EMAIL=admin@example.com ./init-certs.sh
```

This creates a temporary self-signed cert so nginx can start, then obtains a real Let's Encrypt certificate via certbot.

### 3. Build and start

```bash
docker compose up -d --build
docker compose logs -f
```

The first build takes a few minutes (compiles Rust + React). Subsequent builds are fast unless `Cargo.toml` or `package.json` dependencies change.

Docker Compose starts four services: PostgreSQL, backend, nginx (frontend), and certbot. On first launch, the gateway starts in **setup-only mode**.

### 4. Complete Web UI setup

Open `https://your-domain/_ui/` in your browser. Sign in with Google — the first user automatically gets the **admin** role. From the Web UI you can:

- Add your Kiro refresh token (run `kiro login` first)
- Generate per-user API keys for programmatic access
- Configure model settings, timeouts, and debug options
- Manage additional users and their roles

> **Setup-only mode:** Until the first admin completes setup, the gateway returns 503 on all `/v1/*` proxy endpoints.

### 5. Verify

```bash
# Health check
curl https://your-domain/health
# → {"status":"ok"}

# Model list
curl -H "Authorization: Bearer <YOUR_API_KEY>" \
  https://your-domain/v1/models

# Web dashboard
open https://your-domain/_ui/
```

### Token Refresh Workflow

The gateway stores per-user Kiro refresh tokens in PostgreSQL and automatically refreshes access tokens before expiry. If a refresh token eventually expires, the user can update it via the Web UI profile page at `/_ui/profile`.

---

## Datadog APM (Optional)

Both deployment modes support an optional Datadog Agent sidecar for distributed tracing, metrics, log forwarding, and frontend RUM. The integration is zero-overhead when not configured.

### 1. Configure environment variables

Add to your `.env` (full deployment) or `.env.proxy` (proxy-only):

```env
DD_API_KEY=your-datadog-api-key
DD_SITE=datadoghq.com   # or datadoghq.eu, us3.datadoghq.com, etc.
DD_ENV=production
```

For frontend Real User Monitoring (RUM), set these **before building** the frontend image:

```env
VITE_DD_CLIENT_TOKEN=your-rum-client-token
VITE_DD_APPLICATION_ID=your-rum-application-id
VITE_DD_ENV=production
```

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `DD_API_KEY` | Yes | | Datadog API key |
| `DD_SITE` | No | `datadoghq.com` | Datadog intake site |
| `DD_ENV` | No | | Environment tag (e.g. `production`, `staging`) |
| `VITE_DD_CLIENT_TOKEN` | No | | RUM client token (baked into frontend bundle) |
| `VITE_DD_APPLICATION_ID` | No | | RUM application ID (baked into frontend bundle) |

### 2. Start with Datadog Agent

Add `--profile datadog` to your compose command:

```bash
# Full deployment
docker compose --profile datadog up -d

# Proxy-only
docker compose -f docker-compose.gateway.yml --profile datadog --env-file .env.proxy up -d
```

The `datadog-agent` service starts alongside the gateway and receives traces via OTLP on port 4317. `DD_AGENT_HOST` is set automatically by docker-compose.

### 3. Verify

After startup, check that traces are flowing:

```bash
# Check agent is running
docker compose ps datadog-agent

# Check agent logs for connectivity
docker compose logs datadog-agent | grep -i "connected\|error"
```

Then open your Datadog APM dashboard — traces should appear within ~30 seconds of the first request.

**What you'll see in Datadog:**
- Distributed traces for every `/v1/*` request with model, user, and latency breakdown
- Metrics: request rate, error rate, latency percentiles, token usage (per model and user)
- Logs correlated to traces via injected `dd.trace_id` / `dd.span_id` fields
- Frontend RUM sessions linked to backend traces (if `VITE_DD_*` vars are set)

---

## Day-to-Day Operations

### Proxy-Only Mode

```bash
# View live logs
docker compose -f docker-compose.gateway.yml logs -f

# Check container status
docker compose -f docker-compose.gateway.yml ps

# Stop the gateway
docker compose -f docker-compose.gateway.yml down

# Rebuild after code changes
docker compose -f docker-compose.gateway.yml --env-file .env.proxy up -d --build

# Restart without rebuild
docker compose -f docker-compose.gateway.yml restart gateway
```

### Full Deployment

```bash
# View live logs
docker compose logs -f

# Check container status (should show "healthy" after ~30s)
docker compose ps

# Stop all services
docker compose down

# Rebuild after code changes
docker compose up -d --build

# Restart backend only
docker compose restart backend

# Update TLS cert (no rebuild needed — certs are bind-mounted)
cp new-cert.pem certs/cert.pem && cp new-key.pem certs/key.pem
docker compose restart frontend
```

---

## Volume Layout

| Mount | Type | Mode | Purpose |
|-------|------|------|---------|
| `pgdata` | named volume | Full | PostgreSQL data (config, credentials, users, guardrails, MCP) |
| `gateway-data` | named volume | Proxy-Only | Cached device code credentials (`/data/tokens.json`) |
| `./certs:/etc/letsencrypt` | bind | Full | TLS certificates (Let's Encrypt managed) |
| `./certbot/www:/var/www/certbot` | bind | Full | Certbot webroot challenge files |

**Backup (Full Deployment):**

```bash
docker compose exec db pg_dump -U kiro kiro_gateway > backup.sql
```

---

## Troubleshooting

### Proxy-Only Mode

| Symptom | Likely Cause | Fix |
|---------|-------------|-----|
| `ERROR: PROXY_API_KEY is required` | Missing env var | Add `PROXY_API_KEY` to `.env.proxy` |
| Device code URL never appears | OIDC registration failed | Check internet connectivity and `KIRO_REGION` value |
| `Device authorization timed out` | Code not authorized in browser within expiry window | Restart container and authorize promptly |
| Gateway starts but 401 on requests | Wrong API key | Verify `PROXY_API_KEY` matches the `Authorization: Bearer` value |
| Cached credentials stop working | Refresh token expired | Remove volume (`docker compose -f docker-compose.gateway.yml down -v`) and re-authorize |
| `ERROR: OIDC client registration failed` | Wrong SSO URL or region | Verify `KIRO_SSO_URL` and `KIRO_SSO_REGION` values |

### Full Deployment

| Symptom | Likely Cause | Fix |
|---------|-------------|-----|
| `Failed to connect to database` | PostgreSQL not ready | Check `docker compose ps` — `db` should be healthy |
| Container exits immediately | Bad env var or DB connection | `docker compose logs backend` for details |
| 503 on `/v1/*` endpoints | Setup not complete | Open `/_ui/` and complete first-user setup |
| TLS certificate errors | Certs not provisioned | Run `init-certs.sh` (see step 2) |
| Google SSO callback fails | Wrong callback URL | Verify `GOOGLE_CALLBACK_URL` matches your domain |
