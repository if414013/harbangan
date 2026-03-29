# Debug Report: Model Registry Populate Failure

**Date**: 2026-03-29
**Team**: debug-models (ACH methodology)
**Status**: Investigation complete

## Symptom

The providers page (`/_ui/providers` → Models tab) only shows Copilot models (30). Kiro, Anthropic, and OpenAI Codex show 0 models despite all 4 providers showing "Connected" status.

## Root Cause Summary

Three independent failures, all silently swallowed by `.ok()` in `model_registry.rs`:

| Provider | Token Type | API Response | Root Cause |
|----------|-----------|--------------|------------|
| **Kiro** | SSO access token | DNS failure | Docker container cannot resolve `q.ap-southeast-1.amazonaws.com` |
| **Anthropic** | OAuth Access Token (`sk-ant-oat01-...`) | 401 `invalid x-api-key` | OAT tokens from OAuth relay are not accepted by `api.anthropic.com/v1/models` |
| **OpenAI Codex** | JWT (OAuth relay) | 403 `missing api.model.read scope` | OAuth relay JWT lacks the `api.model.read` scope required for `/v1/models` |
| **Copilot** | Device code token | 200 OK, 32 models | Works correctly |

## Evidence

### Verified via curl from Docker container

```
# Anthropic — 401 (OAT token rejected)
curl https://api.anthropic.com/v1/models -H "x-api-key: <oat-token>"
→ {"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}

# OpenAI Codex — 403 (JWT missing model.read scope)
curl https://api.openai.com/v1/models -H "Authorization: Bearer <jwt-token>"
→ {"error":"Missing scopes: api.model.read"}

# Kiro — DNS failure
curl https://q.ap-southeast-1.amazonaws.com/ListAvailableModels
→ Could not resolve host
```

### DB state confirms tokens exist and are not expired

```sql
SELECT provider_id, expires_at, expires_at > NOW() as valid FROM user_provider_tokens;
-- anthropic    | 2026-03-29 16:12:55 | t (valid)
-- openai_codex | 2026-04-08 08:13:44 | t (valid)
```

### Error swallowing in code

All failures are silently consumed at these locations in `backend/src/web_ui/model_registry.rs`:

- **Line 334**: `.ok()` swallows Anthropic admin pool fetch errors
- **Line 347**: `.ok()` swallows Anthropic user token fetch errors
- **Line 365**: `.ok()` swallows OpenAI Codex admin pool fetch errors
- **Line 382**: `.ok()` swallows OpenAI Codex user token fetch errors
- **Line 297**: `.ok()` swallows Kiro global auth fetch errors
- **Line 312**: `.ok()` swallows Kiro user token fetch errors

The "keep-last-successful" guard at line 392 then logs a generic warning with no detail:
```
WARN API returned no models, keeping existing registry provider="anthropic"
```

## Why Copilot Works But Others Don't

Copilot uses a **different token lifecycle**:
- Separate table (`user_copilot_tokens`) with `copilot_token` field
- Token obtained via GitHub device code flow (not OAuth relay)
- Background refresh task (`spawn_copilot_token_refresh_task`) keeps tokens fresh
- The Copilot models API (`api.github.com`) accepts these tokens directly

The other providers use OAuth relay tokens that were designed for **proxying requests** (where the gateway adds auth), not for **direct API calls** like model listing.

## Additional Finding: Runtime Drift (Codex)

Codex's investigation identified that the Docker container was running a stale binary from March 26, missing code fixes from March 29. This was resolved by rebuilding with `docker compose build --no-cache && docker compose up -d`.

## Recommended Fixes

### 1. Anthropic & OpenAI Codex: Hardcoded model fallback lists
Since OAuth relay tokens cannot list models, provide built-in model definitions as fallback when no admin pool API key is configured and the API call fails.

### 2. Kiro: Fix Docker DNS resolution
The Docker container cannot resolve AWS service endpoints. Options:
- Add `dns` configuration to `docker-compose.yml`
- Use `network_mode: host` for the backend container
- Configure Docker's DNS to forward to the host resolver

### 3. Error visibility: Replace `.ok()` with logged errors
Replace silent `.ok()` calls with explicit error logging so failures are visible:
```rust
// Before:
fetch_anthropic_models(...).await.ok()

// After:
match fetch_anthropic_models(...).await {
    Ok(models) => Some(models),
    Err(e) => {
        tracing::warn!(provider = "anthropic", error = ?e, "Failed to fetch models");
        None
    }
}
```

### 4. UI: Surface per-provider populate errors
Return per-provider error details from the populate endpoint so the UI can show why a specific provider failed instead of just "0 models".

## Investigation Methodology

Used Analysis of Competing Hypotheses (ACH) with 3 parallel investigators:

| Hypothesis | Verdict | Notes |
|-----------|---------|-------|
| H1: Logic Error — populate code broken for non-Copilot | **RULED_OUT** | Code handles all 4 providers correctly |
| H2: Config Error — token injection fails | **PARTIALLY CONFIRMED** | Tokens exist but are wrong type for model listing |
| H3: Integration Failure — PR #177 query bug | **PARTIALLY CONFIRMED** | Query works, but returned tokens can't authenticate |

Final root cause is a **credential type mismatch**: OAuth relay tokens ≠ direct API keys.
