# Docker Deployment Runbook

## Prerequisites

- Docker and Docker Compose installed on the VPS
- The repository cloned on the server at `/path/to/rkgw`

---

## First-Time Setup

### 1. Generate a TLS certificate (once)

The gateway requires TLS when binding to `0.0.0.0` (enforced at startup). Run this on
the server to generate a 10-year self-signed certificate:

```bash
mkdir -p certs
openssl req -x509 -newkey rsa:4096 \
  -keyout certs/key.pem -out certs/cert.pem \
  -days 3650 -nodes \
  -subj "/CN=$(hostname)"
```

> **Tip:** To use a real certificate (e.g. from Let's Encrypt), place `fullchain.pem` as
> `certs/cert.pem` and `privkey.pem` as `certs/key.pem`. No other changes are needed.

### 2. Configure environment variables

```bash
cp .env.example .env
# Edit .env — at minimum set POSTGRES_PASSWORD
```

Do **not** set `SERVER_HOST`, `TLS_ENABLED`, `DATABASE_URL`, `TLS_CERT`,
or `TLS_KEY` in `.env` — these are managed by `docker-compose.yml`.

### 3. Build and start

```bash
docker compose up -d --build
docker compose logs -f
```

The first build takes a few minutes (compiles Rust + React). Subsequent builds are fast
unless `Cargo.toml` or `package.json` dependencies change.

Docker Compose starts a PostgreSQL database and the gateway. On first launch, the gateway
starts in setup-only mode.

### 4. Complete Web UI setup

Open `https://your-server:8000/_ui/` in a browser. The setup wizard will ask for:

- **Gateway password** — protects API endpoints
- **Kiro refresh token** — run `kiro login` locally first, then provide the token
- **AWS region** — defaults to `us-east-1`

### 5. Verify

```bash
# Health check (self-signed cert → use -k)
curl -k https://your-server:8000/health
# → {"status":"ok"}

# Model list
curl -k -H "Authorization: Bearer <PROXY_API_KEY>" \
  https://your-server:8000/v1/models

# Web dashboard
open https://your-server:8000/_ui/
```

---

## Token Refresh Workflow

The gateway stores the Kiro refresh token in PostgreSQL and automatically refreshes
access tokens before expiry. If your refresh token eventually expires, update it via
the Web UI configuration page at `/_ui/config`.

---

## Day-to-Day Operations

```bash
# View live logs
docker compose logs -f

# Check container status (should show "healthy" after ~30s)
docker compose ps

# Stop the gateway and database
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

---

## Volume Layout

| Mount | Type | Purpose |
|-------|------|---------|
| `pgdata` | named volume | PostgreSQL data (config, credentials, history) |
| `./certs:/certs:ro` | bind (read-only) | TLS cert + key (operator-managed) |

The `pgdata` named volume is managed by Docker. To back it up:

```bash
docker compose exec db pg_dump -U kiro kiro_gateway > backup.sql
```

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---------|-------------|-----|
| `TLS is required when binding to non-localhost` | `TLS_ENABLED` not set | Ensure `docker-compose.yml` has `TLS_ENABLED: "true"` in `environment:` |
| `TLS certificate file not found` | Certs not in `./certs/` | Run step 1 (generate certs) |
| `Failed to connect to database` | PostgreSQL not ready | Check `docker compose ps` — `db` should be healthy |
| Container exits immediately | Bad env var or DB connection | `docker compose logs gateway` for details |
| `healthy` never reached | TLS cert untrusted by curl | Healthcheck uses `-k` (insecure); if still failing, check port binding |
