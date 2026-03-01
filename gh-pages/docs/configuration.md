---
layout: default
title: Configuration
nav_order: 4
---

# Configuration Reference
{: .no_toc }

Complete reference for all Kiro Gateway configuration options. The gateway can be configured through environment variables, CLI arguments, the `.env` file, or the Web UI.

<details open markdown="block">
  <summary>Table of contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Configuration Precedence

Configuration values are resolved in the following order (highest priority first):

1. **CLI arguments** — e.g., `--port 9000`
2. **Environment variables** — e.g., `SERVER_PORT=9000`
3. **`.env` file** — loaded automatically from the working directory via `dotenvy`
4. **PostgreSQL database** — values saved through the Web UI setup wizard or config page
5. **Built-in defaults** — hardcoded safe defaults

The gateway loads CLI args and environment variables at startup (steps 1-3), then overlays any values stored in PostgreSQL (step 4). This means environment variables override database values for bootstrap settings like `SERVER_HOST` and `SERVER_PORT`, while runtime settings like `log_level` and `debug_mode` can be changed live through the Web UI.

---

## Environment Variables

### Required

These must be set before the gateway can serve API requests. If using Docker Compose, `DATABASE_URL` is managed automatically.

| Variable | Description | Example |
|:---|:---|:---|
| `PROXY_API_KEY` | Password that clients must include in `Authorization: Bearer <key>` headers. Protects all API endpoints. Set during the Web UI setup wizard. | `my-strong-api-key` |
| `DATABASE_URL` | PostgreSQL connection string. Required for config persistence and credential storage. | `postgres://kiro:pass@localhost:5432/kiro_gateway` |
| `KIRO_REGION` | AWS region for the Kiro / Amazon Q Developer API endpoint. | `us-east-1` |

### Server

| Variable | CLI flag | Default | Description |
|:---|:---|:---|:---|
| `SERVER_HOST` | `--host` | `127.0.0.1` | Bind address. Use `0.0.0.0` to listen on all interfaces (required for Docker). |
| `SERVER_PORT` | `--port`, `-p` | `8000` | TCP port to listen on. Docker Compose defaults to `9001`. |
| `WEB_UI` | `--web-ui` | `true` | Enable the Web UI dashboard served at `/_ui/`. Set to `false` to disable. |

### TLS / HTTPS

TLS is always enabled. When no custom certificate is provided, the gateway auto-generates a self-signed certificate stored at `~/.kiro-gateway/tls/`.

| Variable | CLI flag | Default | Description |
|:---|:---|:---|:---|
| `TLS_CERT` | `--tls-cert` | *(none — auto-generated)* | Path to a PEM-encoded TLS certificate file. Must be provided together with `TLS_KEY`. |
| `TLS_KEY` | `--tls-key` | *(none — auto-generated)* | Path to a PEM-encoded TLS private key file. Must be provided together with `TLS_CERT`. |

### Logging and Debugging

| Variable | CLI flag | Default | Description |
|:---|:---|:---|:---|
| `LOG_LEVEL` | `--log-level` | `info` | Log verbosity. One of: `trace`, `debug`, `info`, `warn`, `error`. |
| `DEBUG_MODE` | `--debug-mode` | `off` | Request/response debug logging. `off` = no debug output, `errors` = log failed requests, `all` = log all requests. |

### Timeouts and HTTP Client

These control the gateway's outbound connections to the Kiro API.

| Variable | Default | Description |
|:---|:---|:---|
| `HTTP_REQUEST_TIMEOUT` | `300` (seconds) | Maximum time for a complete HTTP request to the Kiro API. |
| `HTTP_CONNECT_TIMEOUT` | `30` (seconds) | TCP connection timeout for outbound requests. |
| `HTTP_MAX_RETRIES` | `3` | Number of retry attempts for failed upstream requests. |
| `STREAMING_READ_TIMEOUT` | `300` (seconds) | Maximum time to wait for streaming response data. |
| `FIRST_TOKEN_TIMEOUT` | `15` (seconds) | Cancel and retry if no token is received within this time. Helps recover from stalled requests. |
| `TOKEN_REFRESH_THRESHOLD` | `300` (seconds) | Refresh the access token this many seconds before it expires. |

### Converter and Model Settings

These are typically managed through the Web UI config page rather than environment variables.

| Setting | Default | Description |
|:---|:---|:---|
| `fake_reasoning_enabled` | `true` | Enable synthetic reasoning/thinking blocks in responses. |
| `fake_reasoning_max_tokens` | `4000` | Maximum tokens for synthetic reasoning content. |
| `truncation_recovery` | `true` | Automatically detect and retry truncated API responses. |
| `tool_description_max_length` | `10000` | Maximum character length for tool descriptions sent to the Kiro API. |

### Docker-Specific

These variables are used by `docker-compose.yml` and should not be set in `.env` when using Docker:

| Variable | Managed by | Value in Docker |
|:---|:---|:---|
| `SERVER_HOST` | `docker-compose.yml` | `0.0.0.0` |
| `DATABASE_URL` | `docker-compose.yml` | `postgres://kiro:<POSTGRES_PASSWORD>@db:5432/kiro_gateway` |
| `POSTGRES_PASSWORD` | `.env` (optional) | Default: `kiro_secret` |

---

## CLI Arguments Reference

All CLI arguments have corresponding environment variables. The CLI flag takes precedence over the environment variable.

```
kiro-gateway [OPTIONS]

Options:
  --host <HOST>              Server bind address [env: SERVER_HOST] [default: 127.0.0.1]
  -p, --port <PORT>          Server port [env: SERVER_PORT] [default: 8000]
  --proxy-api-key <KEY>      API key for client auth [env: PROXY_API_KEY]
  --kiro-region <REGION>     AWS region [env: KIRO_REGION] [default: us-east-1]
  --log-level <LEVEL>        Log level [env: LOG_LEVEL] [default: info]
  --debug-mode <MODE>        Debug mode [env: DEBUG_MODE] [default: off]
  --database-url <URL>       PostgreSQL URL [env: DATABASE_URL]
  --tls-cert <PATH>          TLS certificate path [env: TLS_CERT]
  --tls-key <PATH>           TLS private key path [env: TLS_KEY]
  --web-ui                   Enable web UI [env: WEB_UI] [default: true]
  --dashboard                Enable TUI dashboard (requires TTY)
  -h, --help                 Print help
  -V, --version              Print version
```

### Examples

Run on a custom port with debug logging:

```bash
kiro-gateway --port 9000 --log-level debug
```

Run with a custom TLS certificate:

```bash
kiro-gateway --tls-cert /etc/ssl/certs/gateway.pem --tls-key /etc/ssl/private/gateway-key.pem
```

Run with the terminal dashboard (requires a TTY):

```bash
kiro-gateway --dashboard
```

---

## PostgreSQL Database Setup

The gateway uses PostgreSQL to persist configuration, credentials, and change history. Tables are created automatically on first connection.

### Local PostgreSQL

```bash
# Install PostgreSQL (macOS)
brew install postgresql@16
brew services start postgresql@16

# Create the database
psql -U postgres -c "CREATE USER kiro WITH PASSWORD 'your_password';"
psql -U postgres -c "CREATE DATABASE kiro_gateway OWNER kiro;"
```

Set the connection string:

```bash
export DATABASE_URL=postgres://kiro:your_password@localhost:5432/kiro_gateway
```

### Docker Compose (automatic)

When using `docker-compose.yml`, PostgreSQL is started automatically as the `db` service. The gateway connects to it using the internal Docker network hostname `db`. No manual setup is needed.

The default credentials are:

| Setting | Value |
|:---|:---|
| Database | `kiro_gateway` |
| User | `kiro` |
| Password | `kiro_secret` (override with `POSTGRES_PASSWORD` in `.env`) |

### Backup and restore

```bash
# Backup
docker compose exec db pg_dump -U kiro kiro_gateway > backup.sql

# Restore
docker compose exec -T db psql -U kiro kiro_gateway < backup.sql
```

### What's stored in the database

| Table | Contents |
|:---|:---|
| `config` | Key-value configuration pairs (proxy_api_key, region, timeouts, etc.) |
| `config_history` | Audit log of all configuration changes with timestamps and source |
| `oauth_credentials` | OAuth client registration and refresh tokens |

---

## TLS / HTTPS Configuration

TLS is always enabled on the gateway. There is no plaintext HTTP mode.

### Auto-generated self-signed certificates (default)

When no `TLS_CERT` / `TLS_KEY` is provided, the gateway automatically generates a self-signed certificate on first startup. The certificate is stored at:

```
~/.kiro-gateway/tls/cert.pem
~/.kiro-gateway/tls/key.pem
```

Properties of the auto-generated certificate:

- Valid for **365 days**
- Automatically regenerated **30 days** before expiry
- SANs include: `localhost`, `127.0.0.1`, `::1`, and the system hostname
- Private key file permissions are restricted to `0600` (Unix)
- Certificate directory permissions are restricted to `0700` (Unix)

Clients connecting to the gateway with a self-signed certificate need to disable certificate verification:

```bash
# curl
curl -k https://localhost:8000/health

# Python (httpx)
httpx.Client(verify=False)

# Node.js
process.env.NODE_TLS_REJECT_UNAUTHORIZED = '0'
```

### Custom certificates

For production deployments, provide your own certificate:

```bash
# Using CLI flags
kiro-gateway --tls-cert /path/to/cert.pem --tls-key /path/to/key.pem

# Using environment variables
export TLS_CERT=/path/to/cert.pem
export TLS_KEY=/path/to/key.pem
```

Both `TLS_CERT` and `TLS_KEY` must be provided together. The files must be in PEM format.

### Using Let's Encrypt

If you have a domain name pointing to your server, you can use Let's Encrypt with certbot:

```bash
# Install certbot
sudo apt install certbot

# Get a certificate
sudo certbot certonly --standalone -d gateway.example.com

# Point the gateway at the certificate
export TLS_CERT=/etc/letsencrypt/live/gateway.example.com/fullchain.pem
export TLS_KEY=/etc/letsencrypt/live/gateway.example.com/privkey.pem
```

For Docker deployments, place the certificate files in a `certs/` directory and bind-mount them:

```bash
# Copy certificates
cp /etc/letsencrypt/live/gateway.example.com/fullchain.pem certs/cert.pem
cp /etc/letsencrypt/live/gateway.example.com/privkey.pem certs/key.pem
```

To update certificates without rebuilding:

```bash
cp new-cert.pem certs/cert.pem
cp new-key.pem certs/key.pem
docker compose restart gateway
```

---

## Configuration via Web UI

The Web UI at `/_ui/` provides a graphical interface for viewing and modifying configuration.

### Setup wizard (`/_ui/` on first launch)

The setup wizard runs once on first launch and collects:

- Gateway password (PROXY_API_KEY)
- AWS SSO Start URL and region
- Initiates the OAuth device code flow for authentication

### Configuration page (`/_ui/config`)

After setup, the config page lets you modify runtime settings. Changes are persisted to PostgreSQL and take effect based on their type:

| Change type | Behavior | Examples |
|:---|:---|:---|
| Hot-reload | Applied immediately, no restart needed | `log_level`, `debug_mode`, `fake_reasoning_enabled`, `truncation_recovery`, `first_token_timeout` |
| Requires restart | Saved to DB but only applied after gateway restart | `server_port`, `proxy_api_key`, `kiro_region` |

### Configuration history (`/_ui/config/history`)

Every configuration change is logged with:

- The key that changed
- Old and new values (sensitive values are masked)
- Timestamp
- Source (e.g., `web_ui`, `setup`)

---

## Complete Configuration Table

Quick reference of every configuration option with its source, default, and whether it can be hot-reloaded.

| Setting | Env var | CLI flag | Default | Hot-reload |
|:---|:---|:---|:---|:---|
| Bind address | `SERVER_HOST` | `--host` | `127.0.0.1` | No |
| Port | `SERVER_PORT` | `--port` | `8000` | No |
| API key | `PROXY_API_KEY` | `--proxy-api-key` | *(empty)* | No |
| AWS region | `KIRO_REGION` | `--kiro-region` | `us-east-1` | No |
| Database URL | `DATABASE_URL` | `--database-url` | *(none)* | No |
| TLS certificate | `TLS_CERT` | `--tls-cert` | *(auto-generated)* | No |
| TLS private key | `TLS_KEY` | `--tls-key` | *(auto-generated)* | No |
| Web UI | `WEB_UI` | `--web-ui` | `true` | No |
| TUI dashboard | — | `--dashboard` | `false` | No |
| Log level | `LOG_LEVEL` | `--log-level` | `info` | Yes |
| Debug mode | `DEBUG_MODE` | `--debug-mode` | `off` | Yes |
| Fake reasoning | — | — | `true` | Yes |
| Fake reasoning max tokens | — | — | `4000` | Yes |
| Truncation recovery | — | — | `true` | Yes |
| Tool description max length | — | — | `10000` | Yes |
| First token timeout | — | — | `15` (sec) | Yes |
| HTTP request timeout | — | — | `300` (sec) | No |
| HTTP connect timeout | — | — | `30` (sec) | No |
| HTTP max retries | — | — | `3` | No |
| HTTP max connections | — | — | `20` | No |
| Streaming timeout | — | — | `300` (sec) | No |
| Token refresh threshold | — | — | `300` (sec) | No |

---

## Example `.env` File

A complete example with all available options:

```bash
# ===========================================
# Required
# ===========================================
PROXY_API_KEY=change-me-to-a-strong-password
KIRO_REGION=us-east-1
DATABASE_URL=postgres://kiro:your_password@localhost:5432/kiro_gateway

# ===========================================
# Server (optional)
# ===========================================
# SERVER_HOST=127.0.0.1
# SERVER_PORT=8000
# WEB_UI=true

# ===========================================
# TLS (optional — omit for auto-generated self-signed cert)
# ===========================================
# TLS_CERT=/path/to/cert.pem
# TLS_KEY=/path/to/key.pem

# ===========================================
# Logging (optional)
# ===========================================
# LOG_LEVEL=info
# DEBUG_MODE=off

# ===========================================
# Timeouts (optional)
# ===========================================
# HTTP_REQUEST_TIMEOUT=300
# HTTP_CONNECT_TIMEOUT=30
# HTTP_MAX_RETRIES=3
# FIRST_TOKEN_TIMEOUT=15
# TOKEN_REFRESH_THRESHOLD=300

# ===========================================
# Docker Compose only (optional)
# ===========================================
# POSTGRES_PASSWORD=kiro_secret
```

---

## Next Steps

- [Getting Started](getting-started.html) — Full installation walkthrough and first-time setup
- [Quickstart](quickstart.html) — Get running in under 5 minutes
