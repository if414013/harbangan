---
layout: default
title: Troubleshooting
nav_order: 9
---

# Troubleshooting
{: .no_toc }

Common issues, error messages, and their solutions when running Kiro Gateway.
{: .fs-6 .fw-300 }

<details open markdown="block">
  <summary>Table of contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Quick Diagnostic Checklist

Before diving into specific issues, run through this checklist:

1. **Is the gateway running?** — `docker compose ps` or `systemctl status kiro-gateway`
2. **Is it healthy?** — `curl -k https://localhost:9001/health`
3. **Can you reach the Web UI?** — Open `https://your-server:9001/_ui/` in a browser
4. **Is setup complete?** — Check the Web UI; if you see the setup wizard, complete it first
5. **Check the logs** — `docker compose logs -f gateway` or `journalctl -u kiro-gateway -f`

---

## Startup Errors

### "TLS is required when binding to non-localhost"

**Cause:** The gateway enforces TLS when `SERVER_HOST` is set to anything other than `127.0.0.1` or `::1`. This is a security measure to prevent unencrypted traffic on public interfaces.

**Solution:** Generate a TLS certificate:

```bash
mkdir -p certs
openssl req -x509 -newkey rsa:4096 \
  -keyout certs/key.pem -out certs/cert.pem \
  -days 3650 -nodes \
  -subj "/CN=$(hostname)"
```

If using Docker Compose, the `docker-compose.yml` already sets `TLS_ENABLED: "true"` and expects certs in `./certs/`.

### "TLS certificate file not found"

**Cause:** The `TLS_CERT` or `TLS_KEY` environment variable points to a file that doesn't exist.

**Solution:**
- Verify the cert files exist: `ls -la certs/`
- If using Docker, ensure the certs directory is bind-mounted in `docker-compose.yml`
- Generate certs if they're missing (see above)

### "--tls-cert was provided without --tls-key"

**Cause:** You provided a TLS certificate but not the corresponding private key (or vice versa). Both must be provided together.

**Solution:** Provide both `TLS_CERT` and `TLS_KEY`:

```bash
export TLS_CERT=./certs/cert.pem
export TLS_KEY=./certs/key.pem
```

### "Failed to connect to PostgreSQL"

**Cause:** The gateway cannot reach the PostgreSQL database at the configured `DATABASE_URL`.

**Solutions:**
- **Docker Compose:** Check that the `db` service is healthy: `docker compose ps`. The gateway depends on `db` with `condition: service_healthy`, so it should wait. If the db container is unhealthy, check its logs: `docker compose logs db`
- **Manual deployment:** Verify PostgreSQL is running: `pg_isready -h localhost -p 5432`
- **Connection string:** Double-check `DATABASE_URL` format: `postgres://user:password@host:port/database`
- **Authentication:** Verify the PostgreSQL user and password are correct: `psql -U kiro -d kiro_gateway -h localhost`

### "--dashboard requires a terminal (TTY)"

**Cause:** The `--dashboard` flag was used but stdout is not a terminal (e.g. running in Docker or piped output).

**Solution:** The TUI dashboard requires an interactive terminal. Either:
- Remove the `--dashboard` flag and use the Web UI at `/_ui/` instead
- Run the gateway in an interactive terminal session

### Container exits immediately

**Cause:** Usually a configuration error or failed database connection.

**Solution:** Check the gateway logs for the specific error:

```bash
docker compose logs gateway
```

Common causes:
- Invalid environment variables in `.env`
- PostgreSQL not ready (check `docker compose ps` — `db` should be healthy)
- Port already in use (change `SERVER_PORT` in `.env`)

---

## Authentication Errors

### "Invalid or missing API Key" (401)

**Cause:** The request doesn't include a valid API key, or the key doesn't match the configured `PROXY_API_KEY`.

**Solutions:**
- Verify you're sending the key in the correct header:
  - OpenAI-style: `Authorization: Bearer YOUR_KEY`
  - Anthropic-style: `x-api-key: YOUR_KEY`
- The `Authorization` header must include the `Bearer ` prefix (with a space)
- Check the configured key in the Web UI at `/_ui/` (it's shown masked)
- If you forgot the key, you can update it via PostgreSQL directly:

```bash
docker compose exec db psql -U kiro kiro_gateway -c \
  "UPDATE config SET value = 'new-key-here' WHERE key = 'proxy_api_key';"
docker compose restart gateway
```

### "Failed to get access token"

**Cause:** The gateway couldn't obtain a valid Kiro API access token. This usually means the refresh token has expired or is invalid.

**Solutions:**
- Open the Web UI at `/_ui/` and check the configuration page
- Re-run the OAuth device code flow from the setup page
- If using a manual refresh token, update it via the Web UI config page
- Check that `KIRO_REGION` is set correctly (default: `us-east-1`)

### "Setup required. Please complete setup at /_ui/" (503)

**Cause:** The gateway is in setup-only mode because initial configuration hasn't been completed. All `/v1/*` endpoints return 503 until setup is done.

**Solution:** Open `https://your-server:9001/_ui/` and complete the setup wizard.

---

## Connection Problems

### Cannot connect to the gateway

**Possible causes and solutions:**

1. **Firewall:** Ensure the gateway port (default 9001) is open:
   ```bash
   # Check if port is listening
   ss -tlnp | grep 9001

   # Open firewall (Ubuntu/Debian)
   sudo ufw allow 9001/tcp
   ```

2. **Bind address:** If `SERVER_HOST=127.0.0.1`, the gateway only accepts local connections. Set to `0.0.0.0` for remote access.

3. **TLS rejection:** If using a self-signed certificate, clients will reject the connection by default. Use `-k` with curl or disable verification in your client library.

4. **Docker networking:** If running in Docker, ensure the port is published:
   ```bash
   docker compose ps
   # Should show: 0.0.0.0:9001->9001/tcp
   ```

### "Connection refused" when calling the API

**Cause:** The gateway is not running or not listening on the expected port.

**Solutions:**
- Check if the process is running: `docker compose ps` or `ps aux | grep kiro-gateway`
- Verify the port: `curl -k https://localhost:9001/health`
- Check for port conflicts: `ss -tlnp | grep 9001`

### Streaming responses hang or disconnect

**Possible causes:**

1. **Reverse proxy buffering:** If using nginx, disable buffering for SSE:
   ```nginx
   proxy_buffering off;
   proxy_cache off;
   proxy_read_timeout 300s;
   ```

2. **First token timeout:** The gateway has a configurable timeout for the first token (default: 15 seconds). If the model takes longer to start responding, increase `first_token_timeout` in the Web UI.

3. **Network timeout:** Some cloud load balancers have idle connection timeouts. Ensure your load balancer timeout exceeds the expected response time (300+ seconds for long completions).

---

## API Errors

### "messages cannot be empty" (400)

**Cause:** The `messages` array in the request body is empty.

**Solution:** Include at least one message:

```json
{
  "model": "claude-sonnet-4-20250514",
  "messages": [
    {"role": "user", "content": "Hello!"}
  ]
}
```

### "max_tokens must be positive" (400)

**Cause:** The `max_tokens` field in an Anthropic-format request is zero or negative.

**Solution:** Set `max_tokens` to a positive integer:

```json
{
  "model": "claude-sonnet-4-20250514",
  "max_tokens": 1024,
  "messages": [...]
}
```

### "Kiro API error: 429 - Rate limit exceeded"

**Cause:** The upstream Kiro API is rate-limiting your requests.

**Solutions:**
- Reduce request frequency
- The gateway automatically retries with backoff (configurable via `http_max_retries`, default: 3)
- Check if you have multiple clients sharing the same Kiro account

### "Kiro API error: 403 - Forbidden"

**Cause:** The Kiro API rejected the request, usually due to an expired or invalid access token.

**Solutions:**
- The gateway auto-refreshes tokens, but if the refresh token itself has expired, you need to re-authenticate
- Open the Web UI and re-run the OAuth setup flow
- Check that `KIRO_REGION` matches your AWS account's region

### Model not found or unexpected model behavior

**Cause:** The model name you're using doesn't match any known model in the Kiro API.

**Solutions:**
- List available models: `curl -k -H "Authorization: Bearer KEY" https://localhost:9001/v1/models`
- Use the exact model ID from the list
- The resolver supports common aliases (e.g. `claude-sonnet-4.5`), but if your alias isn't recognized, use the canonical ID

---

## Docker-Specific Issues

### Build fails with "cargo build" errors

**Possible causes:**
- **Out of memory:** Rust compilation is memory-intensive. Ensure at least 2 GB RAM is available. On low-memory VPS, add swap:
  ```bash
  sudo fallocate -l 2G /swapfile
  sudo chmod 600 /swapfile
  sudo mkswap /swapfile
  sudo swapon /swapfile
  ```
- **Network issues:** Cargo needs to download dependencies. Check internet connectivity from the Docker build context.

### "healthy" status never reached

**Cause:** The Docker health check uses `curl -fsk` to hit the health endpoint. If this fails, the container stays "unhealthy".

**Solutions:**
- Check if the gateway is actually running: `docker compose logs gateway`
- Verify the port matches: the health check uses `${SERVER_PORT:-9001}`
- If using a custom port, ensure it's set in `.env`
- The `start_period: 20s` gives the gateway time to start; if your system is slow, increase it

### PostgreSQL data persistence

**Cause:** PostgreSQL data is stored in a Docker named volume (`pgdata`). If you remove volumes, you lose all configuration.

**Solutions:**
- **Never** use `docker compose down -v` unless you want to reset everything
- Back up regularly:
  ```bash
  docker compose exec db pg_dump -U kiro kiro_gateway > backup.sql
  ```
- Restore from backup:
  ```bash
  cat backup.sql | docker compose exec -T db psql -U kiro kiro_gateway
  ```

### Port conflicts

**Cause:** Another service is already using the configured port.

**Solution:** Change the port in `.env`:

```bash
SERVER_PORT=9002
```

Then restart: `docker compose up -d`

---

## TLS Certificate Issues

### "certificate verify failed" in client

**Cause:** The client is rejecting the gateway's self-signed certificate.

**Solutions by client:**

| Client | Solution |
|--------|----------|
| curl | Add `-k` or `--insecure` flag |
| Python (httpx) | `httpx.Client(verify=False)` |
| Python (requests) | `requests.get(url, verify=False)` |
| Node.js | Set `NODE_TLS_REJECT_UNAUTHORIZED=0` env var |
| OpenAI Python | Pass `http_client=httpx.Client(verify=False)` |
| Anthropic Python | Pass `http_client=httpx.Client(verify=False)` |

For production, use a real certificate from Let's Encrypt (see [Deployment Guide](deployment.html#lets-encrypt-production)).

### Certificate expired

**Cause:** The TLS certificate has passed its expiration date.

**Solution:** Generate a new certificate or renew via Let's Encrypt:

```bash
# Self-signed: regenerate
openssl req -x509 -newkey rsa:4096 \
  -keyout certs/key.pem -out certs/cert.pem \
  -days 3650 -nodes \
  -subj "/CN=$(hostname)"

# Let's Encrypt: renew
sudo certbot renew

# Restart to pick up new cert
docker compose restart gateway
```

### Certificate hostname mismatch

**Cause:** The certificate's Common Name (CN) or Subject Alternative Name (SAN) doesn't match the hostname you're connecting to.

**Solution:** Regenerate the certificate with the correct hostname:

```bash
openssl req -x509 -newkey rsa:4096 \
  -keyout certs/key.pem -out certs/cert.pem \
  -days 3650 -nodes \
  -subj "/CN=your-actual-hostname.com"
```

---

## Log Analysis Tips

### Enable Debug Logging

For detailed request/response logging:

```bash
# Via environment variable
export LOG_LEVEL=debug
export DEBUG_MODE=all

# Or via Web UI: change log_level to "debug" and debug_mode to "all"
```

Debug mode options:
- `off` — no debug output (default)
- `errors` — log request/response bodies only for failed requests
- `all` — log all request/response bodies (verbose, use temporarily)

### Key Log Messages to Watch For

| Log Message | Meaning |
|-------------|---------|
| `Request to /v1/chat/completions: model=X, stream=Y, messages=Z` | Incoming request received |
| `Model resolution: X -> Y (source: Z, verified: true)` | Model name was resolved successfully |
| `Handling streaming response` | Streaming mode activated |
| `Handling non-streaming response (collecting stream)` | Non-streaming mode (Kiro always streams internally) |
| `Access attempt with invalid or missing API key` | Authentication failure |
| `Failed to get access token` | Kiro token refresh failed |
| `Internal error: ...` | Unexpected server error (check full stack trace) |

### Filtering Logs

```bash
# Docker: filter by level
docker compose logs gateway 2>&1 | grep -i error

# Docker: follow with timestamp
docker compose logs -f --timestamps gateway

# systemd: filter by priority
journalctl -u kiro-gateway -p err -f

# Web UI: use the log search feature at /_ui/
# Supports text search with pagination
```

### Metrics for Debugging

The Web UI metrics endpoint provides useful debugging data:

```bash
curl -k -H "Authorization: Bearer KEY" \
  https://localhost:9001/_ui/api/metrics
```

Response includes:
- `active_connections` — currently in-flight requests
- `total_requests` / `total_errors` — lifetime counters
- `latency.p50/p95/p99` — latency percentiles
- `errors_by_type` — breakdown of error categories (`auth`, `validation`, `upstream`, `internal`)
- `models` — per-model request counts and token usage

---

## Getting Help

If you can't resolve an issue:

1. Check the [GitHub Issues](https://github.com/if414013/rkgw/issues) for known problems
2. Collect diagnostic information:
   ```bash
   # Gateway version
   curl -k https://localhost:9001/health | jq .version

   # Container status
   docker compose ps

   # Recent logs (last 100 lines)
   docker compose logs --tail=100 gateway

   # System info
   uname -a
   docker --version
   ```
3. Open a new issue with the diagnostic information and steps to reproduce
