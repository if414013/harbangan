---
layout: default
title: Deployment
nav_order: 8
---

# Deployment Guide
{: .no_toc }

Production deployment instructions for Kiro Gateway, covering Docker Compose, manual VPS deployment, TLS configuration, and PostgreSQL setup.
{: .fs-6 .fw-300 }

<details open markdown="block">
  <summary>Table of contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Deployment Architecture

```mermaid
graph LR
    subgraph Internet
        client1["OpenAI Client<br/>(Python/Node.js)"]
        client2["Anthropic Client"]
        client3["Web Browser"]
    end

    subgraph Docker["Docker Compose Stack"]
        subgraph gateway_container["gateway container"]
            gw["Kiro Gateway<br/>:9001 (HTTPS)"]
            spa["React SPA<br/>(embedded)"]
        end
        subgraph db_container["db container"]
            pg["PostgreSQL 16<br/>:5432"]
        end
    end

    subgraph AWS["AWS"]
        kiro["Kiro API<br/>(CodeWhisperer)"]
        sso["AWS SSO OIDC"]
    end

    client1 -->|"HTTPS /v1/chat/completions"| gw
    client2 -->|"HTTPS /v1/messages"| gw
    client3 -->|"HTTPS /_ui/"| spa

    gw -->|"HTTPS Event Stream"| kiro
    gw -->|"OAuth token refresh"| sso
    gw -->|"Config persistence"| pg

    style gw fill:#4a9eff,color:#fff
    style pg fill:#336791,color:#fff
    style kiro fill:#ff9900,color:#fff
    style sso fill:#ff9900,color:#fff
```

---

## Docker Compose Deployment (Recommended)

Docker Compose is the recommended deployment method. It manages both the gateway and PostgreSQL database as a single stack.

### Prerequisites

- Docker Engine 20.10+ and Docker Compose v2
- Git (to clone the repository)
- At least 1 GB RAM and 2 GB disk space

### Step 1: Clone the Repository

```bash
git clone https://github.com/if414013/rkgw.git
cd rkgw
```

### Step 2: Generate TLS Certificates

The gateway requires TLS when binding to `0.0.0.0` (enforced at startup). Generate a self-signed certificate:

```bash
mkdir -p certs
openssl req -x509 -newkey rsa:4096 \
  -keyout certs/key.pem -out certs/cert.pem \
  -days 3650 -nodes \
  -subj "/CN=$(hostname)"
```

To use a real certificate (e.g. from Let's Encrypt), place `fullchain.pem` as `certs/cert.pem` and `privkey.pem` as `certs/key.pem`. No other changes are needed.

### Step 3: Configure Environment Variables

```bash
cp .env.example .env
```

Edit `.env` and set at minimum:

```bash
# Required: PostgreSQL password
POSTGRES_PASSWORD=your_secure_password_here

# Optional: Change the default port (default: 9001)
SERVER_PORT=9001
```

Do **not** set `SERVER_HOST`, `TLS_ENABLED`, `DATABASE_URL`, `TLS_CERT`, or `TLS_KEY` in `.env` — these are managed by `docker-compose.yml`.

### Step 4: Build and Start

```bash
docker compose up -d --build
```

The first build compiles the Rust binary and React frontend, which takes a few minutes. Subsequent builds are fast unless `Cargo.toml` or `package.json` dependencies change.

Watch the logs to confirm startup:

```bash
docker compose logs -f
```

### Step 5: Complete Web UI Setup

On first launch, the gateway starts in setup-only mode. Open the Web UI in your browser:

```
https://your-server:9001/_ui/
```

The setup wizard will guide you through:

1. **Gateway password** — this becomes your `PROXY_API_KEY` for API authentication
2. **Authentication** — either paste a Kiro refresh token or use the OAuth device code flow
3. **AWS region** — defaults to `us-east-1`

### Step 6: Verify

```bash
# Health check (use -k for self-signed certs)
curl -k https://your-server:9001/health
# Expected: {"status":"healthy","timestamp":"...","version":"1.0.8"}

# List models
curl -k -H "Authorization: Bearer YOUR_PROXY_API_KEY" \
  https://your-server:9001/v1/models

# Test a chat completion
curl -k -X POST https://your-server:9001/v1/chat/completions \
  -H "Authorization: Bearer YOUR_PROXY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-20250514","messages":[{"role":"user","content":"Hello!"}],"max_tokens":50}'
```

### Docker Compose File Reference

The `docker-compose.yml` defines two services:

```yaml
services:
  db:
    image: postgres:16-alpine
    restart: unless-stopped
    environment:
      POSTGRES_DB: kiro_gateway
      POSTGRES_USER: kiro
      POSTGRES_PASSWORD: ${POSTGRES_PASSWORD:-kiro_secret}
    volumes:
      - pgdata:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U kiro -d kiro_gateway"]
      interval: 10s
      timeout: 5s
      retries: 5
      start_period: 10s

  gateway:
    build:
      context: .
      dockerfile: Dockerfile
    image: kiro-gateway:latest
    restart: on-failure:5
    ports:
      - "${SERVER_PORT:-9001}:${SERVER_PORT:-9001}"
    env_file:
      - .env
    environment:
      SERVER_HOST: "0.0.0.0"
      DATABASE_URL: postgres://kiro:${POSTGRES_PASSWORD:-kiro_secret}@db:5432/kiro_gateway
    depends_on:
      db:
        condition: service_healthy
    healthcheck:
      test: ["CMD", "curl", "-fsk", "https://localhost:${SERVER_PORT:-9001}/health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 20s

volumes:
  pgdata:
```

### Volume Layout

| Mount | Type | Purpose |
|-------|------|---------|
| `pgdata` | Named volume | PostgreSQL data (config, credentials, history) |
| `./certs:/certs:ro` | Bind mount (read-only) | TLS certificate + key (operator-managed) |

### Day-to-Day Operations

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
docker compose restart gateway

# Update TLS cert (no rebuild needed — certs are bind-mounted)
cp new-cert.pem certs/cert.pem
cp new-key.pem certs/key.pem
docker compose restart gateway
```

### Database Backup

```bash
# Dump the database
docker compose exec db pg_dump -U kiro kiro_gateway > backup.sql

# Restore from backup
cat backup.sql | docker compose exec -T db psql -U kiro kiro_gateway
```

---

## Manual VPS Deployment

For deployments without Docker, you can build and run the gateway binary directly.

### Prerequisites

- Rust toolchain (1.75+): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- Node.js 18+ and npm (for building the React frontend)
- PostgreSQL 14+ server
- OpenSSL (for TLS certificate generation)

### Step 1: Build the Frontend

```bash
cd web-ui
npm install
npm run build
cd ..
```

The built assets in `web-ui/dist/` are embedded into the Rust binary at compile time via `rust-embed`.

### Step 2: Build the Gateway

```bash
cargo build --release
```

The binary is at `target/release/kiro-gateway`.

### Step 3: Set Up PostgreSQL

```bash
# Create database and user
sudo -u postgres psql <<EOF
CREATE USER kiro WITH PASSWORD 'your_secure_password';
CREATE DATABASE kiro_gateway OWNER kiro;
EOF
```

The gateway automatically creates its tables (`config`, `config_history`, `schema_version`) on first connection.

### Step 4: Generate TLS Certificates

```bash
mkdir -p certs
openssl req -x509 -newkey rsa:4096 \
  -keyout certs/key.pem -out certs/cert.pem \
  -days 3650 -nodes \
  -subj "/CN=$(hostname)"
```

### Step 5: Configure and Run

Create a `.env` file or export environment variables:

```bash
export SERVER_HOST=0.0.0.0
export SERVER_PORT=9001
export DATABASE_URL=postgres://kiro:your_secure_password@localhost:5432/kiro_gateway
export TLS_CERT=./certs/cert.pem
export TLS_KEY=./certs/key.pem
export LOG_LEVEL=info
```

Run the gateway:

```bash
./target/release/kiro-gateway
```

Or with CLI arguments:

```bash
./target/release/kiro-gateway \
  --host 0.0.0.0 \
  --port 9001 \
  --database-url postgres://kiro:your_secure_password@localhost:5432/kiro_gateway \
  --tls-cert ./certs/cert.pem \
  --tls-key ./certs/key.pem \
  --log-level info
```

### Step 6: Set Up as a systemd Service

Create `/etc/systemd/system/kiro-gateway.service`:

```ini
[Unit]
Description=Kiro Gateway
After=network.target postgresql.service
Requires=postgresql.service

[Service]
Type=simple
User=kiro
Group=kiro
WorkingDirectory=/opt/kiro-gateway
ExecStart=/opt/kiro-gateway/kiro-gateway
EnvironmentFile=/opt/kiro-gateway/.env
Restart=on-failure
RestartSec=5
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
```

Enable and start:

```bash
sudo systemctl daemon-reload
sudo systemctl enable kiro-gateway
sudo systemctl start kiro-gateway
sudo journalctl -u kiro-gateway -f
```

---

## TLS Certificate Setup

TLS is always enabled on the gateway. When binding to a non-localhost address (`0.0.0.0`, a public IP, etc.), TLS is enforced at startup.

### Self-Signed Certificate (Development / Internal)

```bash
mkdir -p certs
openssl req -x509 -newkey rsa:4096 \
  -keyout certs/key.pem -out certs/cert.pem \
  -days 3650 -nodes \
  -subj "/CN=kiro-gateway"
```

Clients connecting to a self-signed gateway need to disable certificate verification:

- **curl:** use `-k` or `--insecure`
- **Python httpx:** `httpx.Client(verify=False)`
- **Node.js:** set `NODE_TLS_REJECT_UNAUTHORIZED=0`

### Let's Encrypt (Production)

Use `certbot` to obtain a free certificate:

```bash
# Install certbot
sudo apt install certbot

# Obtain certificate (standalone mode — stop the gateway first)
sudo certbot certonly --standalone -d your-domain.com

# Copy certificates
sudo cp /etc/letsencrypt/live/your-domain.com/fullchain.pem certs/cert.pem
sudo cp /etc/letsencrypt/live/your-domain.com/privkey.pem certs/key.pem
sudo chown kiro:kiro certs/*.pem
```

Set up automatic renewal:

```bash
# Add to crontab
0 3 * * * certbot renew --quiet --deploy-hook "cp /etc/letsencrypt/live/your-domain.com/fullchain.pem /opt/kiro-gateway/certs/cert.pem && cp /etc/letsencrypt/live/your-domain.com/privkey.pem /opt/kiro-gateway/certs/key.pem && systemctl restart kiro-gateway"
```

For Docker deployments, the certs directory is bind-mounted, so just replace the files and restart:

```bash
docker compose restart gateway
```

### Reverse Proxy with nginx

If you prefer to terminate TLS at a reverse proxy:

```nginx
server {
    listen 443 ssl http2;
    server_name your-domain.com;

    ssl_certificate /etc/letsencrypt/live/your-domain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/your-domain.com/privkey.pem;

    location / {
        proxy_pass https://127.0.0.1:9001;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # SSE support
        proxy_buffering off;
        proxy_cache off;
        proxy_read_timeout 300s;
    }
}
```

Note: The gateway still requires its own TLS certificate even behind a reverse proxy, since TLS is always enabled. Use a self-signed cert for the internal connection.

---

## PostgreSQL Setup and Configuration

The gateway uses PostgreSQL for persistent configuration storage, including:

- Gateway password (`proxy_api_key`)
- Kiro refresh tokens
- OAuth client credentials
- Runtime configuration overrides
- Configuration change history

### Database Schema

The gateway automatically creates three tables on first connection:

| Table | Purpose |
|-------|---------|
| `config` | Key-value configuration store. Primary key on `key`, stores `value`, `value_type`, `updated_at`, and optional `description`. |
| `config_history` | Audit log of all configuration changes. Records `key`, `old_value`, `new_value`, `changed_at`, and `source` (e.g. `web_ui`, `setup`, `oauth_setup`). Auto-pruned to 1000 entries. |
| `schema_version` | Tracks database schema version for future migrations. |

### Connection String Format

```
postgres://USER:PASSWORD@HOST:PORT/DATABASE
```

Examples:

```bash
# Local PostgreSQL
DATABASE_URL=postgres://kiro:secret@localhost:5432/kiro_gateway

# Docker Compose (internal network)
DATABASE_URL=postgres://kiro:secret@db:5432/kiro_gateway

# Remote PostgreSQL with SSL
DATABASE_URL=postgres://kiro:secret@db.example.com:5432/kiro_gateway?sslmode=require
```

### PostgreSQL Tuning for Production

For a gateway handling moderate traffic, these PostgreSQL settings are recommended:

```
# postgresql.conf
shared_buffers = 128MB
max_connections = 50
work_mem = 4MB
```

The gateway uses a connection pool (via `sqlx`), so it won't open more than a handful of connections.

---

## Environment Variable Reference

All configuration can be set via environment variables, CLI arguments, or the Web UI (persisted to PostgreSQL).

### Bootstrap Variables (CLI / .env)

These are read at startup and cannot be changed via the Web UI:

| Variable | CLI Flag | Default | Description |
|----------|----------|---------|-------------|
| `SERVER_HOST` | `--host` | `127.0.0.1` | Bind address. Set to `0.0.0.0` for Docker/remote access. |
| `SERVER_PORT` | `--port` | `8000` | Listen port. |
| `DATABASE_URL` | `--database-url` | — | PostgreSQL connection string. Required for config persistence and Web UI. |
| `TLS_CERT` | `--tls-cert` | — | Path to TLS certificate (PEM). Auto-generated if not set. |
| `TLS_KEY` | `--tls-key` | — | Path to TLS private key (PEM). Auto-generated if not set. |
| `WEB_UI` | `--web-ui` | `true` | Enable the Web UI dashboard at `/_ui/`. |
| `LOG_LEVEL` | `--log-level` | `info` | Log verbosity: `trace`, `debug`, `info`, `warn`, `error`. |
| `DEBUG_MODE` | `--debug-mode` | `off` | Debug output: `off`, `errors`, `all`. |

### Runtime Variables (Web UI / PostgreSQL)

These can be changed at runtime via the Web UI without restarting:

| Variable | Default | Description |
|----------|---------|-------------|
| `PROXY_API_KEY` | — | API key for client authentication. Set during initial setup. |
| `KIRO_REGION` | `us-east-1` | AWS region for the Kiro API. |
| `fake_reasoning_enabled` | `true` | Enable extended thinking / reasoning extraction. |
| `fake_reasoning_max_tokens` | `4000` | Maximum tokens for reasoning output. |
| `truncation_recovery` | `true` | Detect and recover from truncated responses. |
| `tool_description_max_length` | `10000` | Maximum character length for tool descriptions. |
| `first_token_timeout` | `15` | Seconds to wait for the first token before timing out. |

---

## Health Monitoring

### Health Check Endpoint

```bash
curl -k https://your-server:9001/health
```

Returns `200 OK` with:

```json
{
  "status": "healthy",
  "timestamp": "2025-03-01T12:00:00.000Z",
  "version": "1.0.8"
}
```

### Docker Health Check

The Docker Compose configuration includes a built-in health check that runs every 30 seconds:

```yaml
healthcheck:
  test: ["CMD", "curl", "-fsk", "https://localhost:${SERVER_PORT:-9001}/health"]
  interval: 30s
  timeout: 10s
  retries: 3
  start_period: 20s
```

Check container health:

```bash
docker compose ps
# NAME       SERVICE   STATUS          PORTS
# rkgw-db-1  db        Up (healthy)    5432/tcp
# rkgw-gw-1  gateway   Up (healthy)    0.0.0.0:9001->9001/tcp
```

### Web UI Metrics

The Web UI at `/_ui/` provides real-time monitoring:

- Active connections
- Total requests and errors
- Latency percentiles (p50, p95, p99)
- Per-model statistics
- Error breakdown by type
- Live log streaming

### Log Access

```bash
# Docker logs
docker compose logs -f gateway

# systemd logs
journalctl -u kiro-gateway -f

# Filter by level
docker compose logs gateway 2>&1 | grep ERROR
```

The gateway uses structured logging via `tracing`. Log output includes timestamps, levels, and structured fields:

```
2025-03-01T12:00:00.000Z  INFO kiro_gateway::routes: Request to /v1/chat/completions: model=claude-sonnet-4-20250514, stream=true, messages=3
```
