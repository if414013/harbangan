---
layout: default
title: Troubleshooting
nav_order: 9
---

# Troubleshooting
{: .no_toc }

Common issues, error messages, and their solutions when running Harbangan.
{: .fs-6 .fw-300 }

<details open markdown="block">
  <summary>Table of contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Quick Diagnostic Checklist

### Proxy-Only Mode

1. **Is the gateway running?** — `docker compose -f docker-compose.gateway.yml ps` (expect: gateway)
2. **Is the gateway healthy?** — `curl http://localhost:8000/health` (expect: `{"status":"ok"}`)
3. **Check the logs** — `docker compose -f docker-compose.gateway.yml logs -f gateway`
4. **Was device code flow completed?** — Look for "Authorization successful!" or "Cached credentials valid" in logs

### Full Deployment

1. **Are all services running?** — `docker compose ps` (expect: db, backend, frontend)
2. **Is the backend healthy?** — `docker compose logs backend` (look for startup messages)
3. **Can you reach the Web UI?** — Open `http://localhost:5173/_ui/` in a browser
4. **Is setup complete?** — If you see the setup wizard, complete it first (sign in with Google)
5. **Check the logs** — `docker compose logs -f backend` for backend, `docker compose logs -f frontend` for frontend

---

## Startup Errors

### "Failed to connect to PostgreSQL"

**Cause:** The backend cannot reach the PostgreSQL database at the configured `DATABASE_URL`.

**Solutions:**
- **Check service health:** `docker compose ps` — the `db` service should show "healthy". The backend depends on `db` with `condition: service_healthy`.
- **View db logs:** `docker compose logs db` — check for PostgreSQL startup errors
- **Verify credentials:** Ensure `POSTGRES_PASSWORD` in `.env` matches what the db container expects
- **Connection string:** `DATABASE_URL` is auto-set by docker-compose to `postgres://kiro:$POSTGRES_PASSWORD@db:5432/kiro_gateway`

### Backend container exits immediately

**Cause:** Usually a configuration error or failed database connection.

**Solution:** Check the backend logs for the specific error:

```bash
docker compose logs backend
```

Common causes:
- Invalid environment variables in `.env`
- PostgreSQL not ready (check `docker compose ps` — `db` should be healthy)
- Missing `POSTGRES_PASSWORD` in `.env`

### Frontend container won't start

**Cause:** Port conflict or dependency issue.

**Solutions:**
- **Check logs:** `docker compose logs frontend`
- **Port conflict:** Ensure port 5173 is not in use by another service

---

## Google SSO Issues

### "OAuth callback URL mismatch"

**Cause:** The Google OAuth callback URL configured in the Admin UI doesn't match the authorized redirect URI in your Google Cloud Console.

**Solutions:**
- **Check Admin UI:** Go to `/_ui/` > Admin > Configuration and verify the Google OAuth callback URL is `https://your-domain/_ui/api/auth/google/callback`
- **Check Google Cloud Console:** Go to APIs & Services > Credentials > Your OAuth Client. The "Authorized redirect URIs" must include the exact same URL.
- **Protocol matters:** The URL must use `https://`, not `http://`
- **No trailing slash:** Ensure there's no trailing slash mismatch

### "Sign in with Google" fails silently

**Cause:** Missing or incorrect Google OAuth configuration.

**Solutions:**
- Verify Google OAuth is configured in the Admin UI (Configuration page):
  - Google Client ID
  - Google Client Secret
  - Google Callback URL
- Check that the OAuth consent screen is configured in Google Cloud Console
- For development, ensure "Test users" are added if the app is in testing mode
- Check backend logs for OAuth errors: `docker compose logs backend | grep -i oauth`

### "Access denied" after Google login

**Cause:** The user's Google account domain may not be in the allowed domain list.

**Solutions:**
- If domain allowlisting is enabled, the admin must add the user's email domain via the web UI
- The first user (admin) bypasses domain restrictions during initial setup
- Check the domain allowlist under Configuration → Authentication at `/_ui/config`

---

## Authentication Errors

### "Invalid or missing API Key" (401) on /v1/* endpoints

**Cause:** The request doesn't include a valid per-user API key.

**Solutions:**
- Create an API key in the web dashboard at `/_ui/` (API Keys section)
- Verify the key is sent in the correct header:
  - OpenAI-style: `Authorization: Bearer YOUR_API_KEY`
  - Anthropic-style: `x-api-key: YOUR_API_KEY`
- The `Authorization` header must include the `Bearer ` prefix (with a space)
- API keys are per-user — each user must create their own

### "Failed to get access token"

**Cause:** The gateway couldn't obtain a valid Kiro API access token for the user. The user's Kiro refresh token may have expired or not been configured.

**Solutions:**
- Sign in to the web UI and check the Kiro token management section
- Re-configure Kiro credentials for the affected user
- Each user manages their own Kiro tokens — verify the specific user's configuration

### "Setup required. Please complete setup at /_ui/" (503)

**Cause:** The gateway is in setup-only mode because no admin user exists in the database. All `/v1/*` endpoints return 503 until setup is done.

**Solution:** Open `https://your-domain/_ui/` and complete setup by signing in with Google. The first user gets the admin role.

---

## Connection Problems

### Cannot connect to the gateway

**Possible causes and solutions:**

1. **Services not running:** Check all four services are up:
   ```bash
   docker compose ps
   ```

2. **Firewall:** Ensure ports 80 and 443 are open:
   ```bash
   # Check if ports are listening
   ss -tlnp | grep -E ':(80|443)\s'

   # Open firewall (Ubuntu/Debian)
   sudo ufw allow 80/tcp
   sudo ufw allow 443/tcp
   ```

3. **DNS:** Ensure your domain resolves to the server's IP:
   ```bash
   dig your-domain +short
   ```

4. **Frontend not proxying:** Check frontend logs:
   ```bash
   docker compose logs frontend
   ```

### Cannot reach the backend

**Cause:** Backend is not running or port mismatch.

**Solutions:**
- Check that the backend container is running: `docker compose ps backend`
- Check backend logs for errors: `docker compose logs backend`
- Verify the backend is listening on port 9999: `docker compose exec backend curl -s http://localhost:9999/health`
- Ensure both services are on the same Docker network

### Streaming responses hang or disconnect

**Possible causes:**

1. **Proxy timeout:** If using a reverse proxy, ensure timeouts are set high enough for SSE streaming (300s recommended for long completions).

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

**Solution:** Set `max_tokens` to a positive integer.

### "Kiro API error: 429 - Rate limit exceeded"

**Cause:** The upstream Kiro API is rate-limiting your requests.

**Solutions:**
- Reduce request frequency
- The gateway automatically retries with backoff (configurable via `http_max_retries`, default: 3)
- Check if multiple users are sharing the same Kiro credentials

### "Kiro API error: 403 - Forbidden"

**Cause:** The Kiro API rejected the request, usually due to an expired or invalid access token.

**Solutions:**
- The gateway auto-refreshes tokens, but if the refresh token itself has expired, the user needs to re-configure their Kiro credentials via the web UI
- Each user manages their own Kiro tokens — check the specific user's token status

### Model not found or unexpected model behavior

**Cause:** The model name doesn't match any known model in the Kiro API.

**Solutions:**
- List available models: `curl -H "Authorization: Bearer YOUR_KEY" https://your-domain/v1/models`
- Use the exact model ID from the list
- The resolver supports common aliases (e.g. `claude-sonnet-4.5`), but if your alias isn't recognized, use the canonical ID

---

## Docker-Specific Issues

### Build fails during `docker compose build`

**Possible causes:**

- **Out of memory:** Rust compilation is memory-intensive. Ensure at least 2 GB RAM is available. On low-memory VPS, add swap:
  ```bash
  sudo fallocate -l 2G /swapfile
  sudo chmod 600 /swapfile
  sudo mkswap /swapfile
  sudo swapon /swapfile
  ```
- **Network issues:** Both Cargo (Rust dependencies) and npm (frontend dependencies) need internet access during the build.

### PostgreSQL data persistence

PostgreSQL data is stored in a Docker named volume (`pgdata`). If you remove volumes, you lose all configuration, users, and API keys.

**Best practices:**
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

**Cause:** Another service is already using port 80 or 443.

**Solution:** Stop the conflicting service or adjust your configuration. The gateway uses ports 9999 (backend) and 5173 (frontend).

```bash
# Find what's using the ports
ss -tlnp | grep -E ':(9999|5173)\s'
```

---

## Guardrails Issues

### Guardrails not blocking content

**Cause:** Rules or profiles may not be configured correctly.

**Solutions:**
- Verify `guardrails_enabled` is set to `true` in the configuration
- Check that the guardrail profile is enabled
- Check the rule's CEL expression matches your request — use the **Validate CEL** endpoint to check syntax
- Check the rule's sampling rate — a rate of 50 means only half of matching requests are checked
- Verify the rule's `apply_to` setting matches the direction (input, output, or both)
- Use the **Test Profile** feature to verify your AWS Bedrock guardrail responds as expected

### Bedrock API errors

**Cause:** AWS credentials or guardrail configuration is incorrect.

**Solutions:**
- Verify the AWS access key and secret key are valid and have `bedrock:ApplyGuardrail` permissions
- Check the AWS region matches where your guardrail is deployed
- Verify the guardrail ID and version exist in your AWS account
- Test the profile using the **Test Profile** feature to see detailed error messages

### High latency from guardrails

**Cause:** Guardrail validation adds latency to every matching request.

**Solutions:**
- Reduce the sampling rate (lower percentage = fewer requests checked)
- Increase the per-rule `timeout_ms` if Bedrock calls are timing out
- Use CEL expressions to target only specific request types rather than checking everything
- Consider enabling guardrails only for input validation on high-traffic endpoints

### Guardrails only working for input, not output

**Cause:** Output validation is only supported for non-streaming requests.

**Solution:** This is by design. Streaming responses bypass output guardrail checks because the response is delivered incrementally. If output validation is important, use non-streaming mode for those requests.

---

## Proxy-Only Mode Issues

{: .note }
> Proxy-Only Mode (`docker-compose.gateway.yml`) supports **all providers** (Kiro, Anthropic, OpenAI Codex, Copilot, Custom) via environment variables. Kiro credentials are cached in the `gateway-data` Docker volume at `/data/tokens.json`. Other providers are configured via API key env vars in `.env.proxy`.

### "ERROR: PROXY_API_KEY is required"

**Cause:** The `PROXY_API_KEY` environment variable is not set.

**Solution:** Set `PROXY_API_KEY` in your `.env.proxy` file and pass it via `--env-file`:

```bash
docker compose -f docker-compose.gateway.yml --env-file .env.proxy up
```

### Device code flow URL not appearing

**Cause:** The container may have cached credentials from a previous run.

**Solutions:**
- Check logs for "Cached credentials valid" — if present, the gateway is reusing existing credentials and no device code flow is needed
- If credentials are stale, clear them: `docker volume rm harbangan_gateway-data`
- Then restart: `docker compose -f docker-compose.gateway.yml --env-file .env.proxy up`

### "OIDC client registration failed"

**Cause:** The AWS SSO OIDC endpoint is unreachable or returned an error.

**Solutions:**
- Check that the container has internet access
- Verify `KIRO_REGION` is a valid AWS region (default: `us-east-1`)
- If using Identity Center (pro), verify `KIRO_SSO_URL` is correct
- Check `KIRO_SSO_REGION` if your SSO endpoint is in a different region than `KIRO_REGION`

### "Device authorization timed out"

**Cause:** The device code flow expired before you authorized in the browser (default expiry is ~600 seconds).

**Solution:** Restart the container and complete the browser authorization promptly:

```bash
docker compose -f docker-compose.gateway.yml --env-file .env.proxy restart gateway
```

### Cached credentials expired

**Cause:** The refresh token in the cached credentials has expired (typically after extended inactivity).

**Solutions:**
1. Clear the credential cache:
   ```bash
   docker volume rm harbangan_gateway-data
   ```
2. Restart the gateway to trigger a fresh device code flow:
   ```bash
   docker compose -f docker-compose.gateway.yml --env-file .env.proxy up
   ```

### "Invalid or missing API Key" (401) in Proxy-Only Mode

**Cause:** The `Authorization: Bearer` or `x-api-key` value doesn't match `PROXY_API_KEY`.

**Solutions:**
- Verify the key in your `.env.proxy` file matches what you're sending in the request
- The `Authorization` header must include the `Bearer ` prefix (with a space)
- Both `Authorization: Bearer {key}` and `x-api-key: {key}` headers are supported

### Wrong SSO mode (Builder ID vs Identity Center)

**Cause:** You're trying to use Identity Center but `KIRO_SSO_URL` is not set, or vice versa.

**Solutions:**
- **Builder ID (free):** Leave `KIRO_SSO_URL` unset in `.env.proxy`
- **Identity Center (pro):** Set `KIRO_SSO_URL=https://your-org.awsapps.com/start`
- After changing, clear credentials and restart:
  ```bash
  docker volume rm harbangan_gateway-data
  docker compose -f docker-compose.gateway.yml --env-file .env.proxy up
  ```

### Permission denied writing to `/data`

**Cause:** The `gateway-data` volume is owned by root but the container runs as `appuser` (non-root).

**Solutions:**
- This should not occur with a fresh volume — Docker creates the volume with correct ownership.
- If it does occur (e.g., after manually copying files into the volume), reset the volume:
  ```bash
  docker compose -f docker-compose.gateway.yml down
  docker volume rm harbangan_gateway-data
  docker compose -f docker-compose.gateway.yml --env-file .env.proxy up
  ```

---

## Provider-Specific Issues

{: .note }
> The OAuth-based provider flows (Copilot connect button, Anthropic OAuth relay) below apply to **Full Deployment only** (`docker-compose.yml`). In Proxy-Only Mode, providers are configured via environment variables instead of OAuth flows.

### GitHub Copilot

#### "Copilot not available" or connect button missing

**Cause:** The Copilot provider is not configured on the server.

**Solutions:**
- Verify `GITHUB_COPILOT_CLIENT_ID`, `GITHUB_COPILOT_CLIENT_SECRET`, and `GITHUB_COPILOT_CALLBACK_URL` are set in `.env`
- The callback URL must be `https://{DOMAIN}/_ui/api/copilot/callback`
- Register a GitHub OAuth App at [GitHub Developer Settings](https://github.com/settings/developers) with the matching callback URL
- Restart the backend after changing environment variables

#### Copilot token refresh fails

**Cause:** The GitHub OAuth refresh token has expired or been revoked.

**Solutions:**
- Go to the Profile page in the Web UI and disconnect Copilot, then reconnect
- Check that the GitHub OAuth App is still active and not suspended
- Verify the user hasn't revoked the app's access in their GitHub settings (Settings > Applications > Authorized OAuth Apps)

#### "Failed to exchange code" during Copilot connect

**Cause:** The OAuth code exchange with GitHub failed.

**Solutions:**
- Verify `GITHUB_COPILOT_CLIENT_SECRET` is correct
- Check that the callback URL in `.env` exactly matches the one registered in the GitHub OAuth App
- Check backend logs for the specific error: `docker compose logs backend | grep -i copilot`

### Provider Priority and Fallback

#### Requests using wrong provider

**Cause:** Provider priority order may not be set as expected.

**Solutions:**
- Check the Profile page to verify your provider priority order
- The gateway uses the first available provider in priority order — if a higher-priority provider's credentials are expired, it falls back to the next
- Reconnect any providers showing as disconnected

#### "No provider credentials available"

**Cause:** None of the user's configured providers have valid credentials.

**Solutions:**
- Go to the Profile page and check the status of each connected provider
- Reconnect any providers with expired tokens
- Ensure at least one provider (Kiro or Copilot) is connected and active

---

## Datadog APM Issues

### No traces appearing in Datadog

**Possible causes:**

1. **Agent not running:** Check the Datadog Agent container is up:
   ```bash
   docker compose ps datadog-agent
   docker compose logs datadog-agent | tail -20
   ```

2. **Missing `DD_API_KEY`:** The agent won't forward data without a valid API key. Verify it's set in `.env` and the agent container has it:
   ```bash
   docker compose exec datadog-agent env | grep DD_API_KEY
   ```

3. **Wrong `DD_SITE`:** If your Datadog account is on a non-US site (e.g. EU), set `DD_SITE=datadoghq.eu`. The default is `datadoghq.com`.

4. **`DD_AGENT_HOST` not set:** The backend only activates tracing when `DD_AGENT_HOST` is set. In docker-compose this is set automatically when using `--profile datadog`. If running outside docker-compose, set it manually to the agent's hostname.

5. **Profile not activated:** Ensure you started with `--profile datadog`:
   ```bash
   docker compose --profile datadog up -d
   ```

### No frontend RUM data

**Cause:** The `VITE_DD_*` variables are build-time — they must be set before building the frontend image.

**Solutions:**
- Set `VITE_DD_CLIENT_TOKEN` and `VITE_DD_APPLICATION_ID` in `.env` before building
- Rebuild the frontend image: `docker compose build frontend`
- Verify the variables were baked in: check the browser console for Datadog RUM initialization messages
- If the variables are empty at build time, the RUM SDK is not initialized and no data is sent

### Log correlation not working (no `dd.trace_id` in logs)

**Cause:** JSON log formatting with trace ID injection is only active when `DD_AGENT_HOST` is set.

**Solutions:**
- Verify `DD_AGENT_HOST` is set in the backend container's environment
- Check that the Datadog Agent is running and reachable
- Confirm you're looking at backend logs — only the Rust backend injects trace IDs

### Datadog Agent exits immediately

**Cause:** Usually a missing or invalid `DD_API_KEY`.

**Solution:** Check the agent logs for the specific error:
```bash
docker compose logs datadog-agent
```

Common messages:
- `API key is missing` — set `DD_API_KEY` in `.env`
- `API key is invalid` — verify the key is correct in your Datadog account settings

---

## Log Analysis Tips

### Enable Debug Logging

For detailed request/response logging, change settings in the Web UI (admin only):

- Set `log_level` to `debug`
- Set `debug_mode` to `all` (logs all request/response bodies — use temporarily)

Debug mode options:
- `off` — no debug output (default)
- `errors` — log request/response bodies only for failed requests
- `all` — log all request/response bodies (verbose)

### Viewing Logs

```bash
# Proxy-Only Mode
docker compose -f docker-compose.gateway.yml logs -f gateway

# Full Deployment — all services
docker compose logs -f

# Full Deployment — backend only
docker compose logs -f backend

# Full Deployment — frontend only
docker compose logs -f frontend

# Filter by level
docker compose logs backend 2>&1 | grep -i error

# Last 100 lines with timestamps
docker compose logs -f --timestamps --tail=100 backend

# Web UI: use the log viewer at /_ui/ (Full Deployment only, requires login)
```

### Key Log Messages to Watch For

| Log Message | Meaning |
|-------------|---------|
| `Request to /v1/chat/completions: model=X, stream=Y, messages=Z` | Incoming request received |
| `Model resolution: X -> Y (source: Z, verified: true)` | Model name resolved successfully |
| `Handling streaming response` | Streaming mode activated |
| `Access attempt with invalid or missing API key` | Authentication failure |
| `Failed to get access token` | Kiro token refresh failed |
| `Internal error: ...` | Unexpected server error (check full trace) |

---

## Getting Help

If you can't resolve an issue:

1. Check the [GitHub Issues](https://github.com/if414013/harbangan/issues) for known problems
2. Collect diagnostic information:
   ```bash
   # Proxy-Only Mode
   docker compose -f docker-compose.gateway.yml ps
   docker compose -f docker-compose.gateway.yml logs --tail=100 gateway

   # Full Deployment
   docker compose ps
   docker compose logs --tail=100 backend
   docker compose logs --tail=100 frontend
   # System info
   uname -a
   docker --version
   docker compose version
   ```
3. Open a new issue with the diagnostic information and steps to reproduce
