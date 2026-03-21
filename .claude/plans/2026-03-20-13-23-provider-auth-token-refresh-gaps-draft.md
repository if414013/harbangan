# Provider Authentication & Token Refresh Gap Analysis

## Context

Harbangan supports 6 AI providers: **Kiro, Anthropic, OpenAI (Codex), GitHub Copilot, Qwen, Custom**. Kiro has a mature background refresh task, but the other providers were added later with varying levels of token lifecycle management. This analysis identifies gaps where expired tokens would force users to manually reconnect instead of being automatically refreshed.

## Provider Auth Summary

| Provider | Auth Method | Token Storage | Auto-Refresh | On Expiry |
|----------|------------|---------------|-------------|-----------|
| **Kiro** | AWS SSO device flow | `user_kiro_tokens` (DB) + `kiro_token_cache` (mem) | Background task every 5 min + on-demand via `AuthManager` | Marked expired, user must reconnect |
| **Anthropic** | OAuth PKCE relay | `user_provider_tokens` (DB) | On-demand via `ensure_fresh_token()` with 5-min buffer | Token deleted, falls back to Kiro |
| **OpenAI Codex** | OAuth PKCE relay | `user_provider_tokens` (DB) | On-demand via `ensure_fresh_token()` with 5-min buffer | Token deleted, falls back to Kiro |
| **Qwen** | OAuth device flow + PKCE | `user_provider_tokens` (DB) | On-demand via `ensure_fresh_token()` with 5-min buffer | Token deleted, falls back to Kiro |
| **Copilot** | GitHub device flow → internal token | `user_copilot_tokens` (DB) | Background task every 120s only | 401/403 error, no fallback |
| **Custom** | Static API key (env var) | Environment variables only | None (by design) | Request fails permanently |

## Identified Gaps

### Gap 1: Copilot Token Refresh — Not Checked at Request Time (CRITICAL)

**Files:**
- `backend/src/providers/copilot.rs` — `send_request()` uses token without expiry check
- `backend/src/providers/registry.rs` — `ensure_fresh_token()` only handles `user_provider_tokens` table (Anthropic/OpenAI/Qwen), NOT the separate `user_copilot_tokens` table
- `backend/src/routes/pipeline.rs:54-62` — calls `ensure_fresh_token()` which is a no-op for Copilot

**How it works today:**
1. `pipeline.rs` calls `ensure_fresh_token()` before every request
2. `ensure_fresh_token()` maps model name → provider_id (anthropic/openai_codex/qwen)
3. Copilot is NOT in this mapping — it can serve any model name based on user priority
4. So `ensure_fresh_token()` returns immediately without checking Copilot tokens
5. Background task (`spawn_copilot_token_refresh_task`) runs every 120s with 5-min buffer
6. If token expires between background refresh cycles, request hits Copilot API with stale token → 401

**Two-level auth adds complexity:**
- GitHub OAuth token (long-lived) → used to fetch Copilot internal token
- Copilot internal token (short-lived, from `/copilot_internal/v2/token`) → used for API calls
- Background task correctly uses github_token to refresh copilot_token
- But `refresh_in` field from GitHub API is stored but NEVER consulted

**Impact:** Users with Copilot as their primary provider will intermittently get 401 errors with no graceful fallback to Kiro.

### Gap 2: No Fallback on Copilot Auth Failure (MEDIUM)

**Files:**
- `backend/src/providers/copilot.rs` — returns raw 401/403 to caller
- `backend/src/providers/registry.rs` — no fallback logic for Copilot

**What happens:** When Copilot returns 401, the error propagates directly to the user. Unlike Anthropic/OpenAI/Qwen which fall back to Kiro when refresh fails, Copilot has no fallback path.

### Gap 3: Transient Refresh Failures Cause Permanent Disconnection (MEDIUM)

**File:** `backend/src/providers/registry.rs:305-316`

**What happens:** When `refresh_token()` fails for ANY reason (including transient network errors), the token is immediately deleted from the DB and the user falls back to Kiro. No retry. A brief network blip permanently disconnects the provider.

Same applies to Kiro: `mark_kiro_token_expired()` sets access_token=NULL on first failure.

### Gap 4: Custom Provider Has No Token Lifecycle (LOW — by design)

**File:** `backend/src/providers/custom.rs`

Static API keys from env vars. No expiry tracking, no refresh. This is intentional for self-hosted backends (Ollama, vLLM, etc.) that use static keys, but worth documenting.

### Gap 5: Kiro Background Refresh vs On-Demand Inconsistency (LOW)

Kiro has BOTH background refresh (every 5 min) AND on-demand refresh via `AuthManager`. Other OAuth providers (Anthropic, OpenAI, Qwen) only have on-demand refresh via `ensure_fresh_token()`. This means:
- Kiro: tokens refreshed proactively even without traffic
- Others: tokens only refreshed when a request arrives — if token expired during idle period, first request triggers refresh (adds latency but works)

This is acceptable but creates inconsistent behavior.

## Recommended Fixes (Priority Order)

### Fix 1: Add Request-Time Copilot Token Check

Add Copilot token expiry check to the request pipeline, either:
- **Option A:** Extend `ensure_fresh_token()` in `registry.rs` to also check `user_copilot_tokens` when the resolved provider is Copilot
- **Option B:** Add a pre-request check in `copilot.rs::send_request()` that checks `expires_at` and triggers a refresh using the stored `github_token` before sending

### Fix 2: Add Copilot Fallback to Kiro

When Copilot returns 401/403 (auth failure), retry the request via Kiro instead of propagating the error. Similar to how Anthropic/OpenAI/Qwen fall back.

### Fix 3: Add Retry for Transient Refresh Failures

Instead of immediately deleting tokens on refresh failure, implement:
- 1-2 retries with exponential backoff for transient errors (network, 500s)
- Only delete/mark-expired for permanent failures (400 invalid_grant, 401 revoked)

### Fix 4: Use `refresh_in` Field for Copilot

The GitHub API returns `refresh_in` (seconds until recommended refresh) but the background task ignores it. Use this value instead of the hardcoded 5-min window for more accurate refresh timing.

## Key Files Reference

| File | Role |
|------|------|
| `backend/src/providers/registry.rs` | Provider routing, `ensure_fresh_token()`, fallback logic |
| `backend/src/providers/copilot.rs` | Copilot provider implementation |
| `backend/src/providers/traits.rs` | Provider trait definition |
| `backend/src/web_ui/copilot_auth.rs` | Copilot device flow + background refresh task |
| `backend/src/web_ui/provider_oauth.rs` | OAuth relay flow + token exchange for Anthropic/OpenAI/Qwen |
| `backend/src/routes/pipeline.rs` | Request pipeline that calls `ensure_fresh_token()` |
| `backend/src/auth/mod.rs` | Kiro AuthManager with on-demand refresh |
| `backend/src/web_ui/user_kiro.rs` | Kiro device flow + background refresh task |
| `backend/src/web_ui/config_db.rs` | DB schema and token CRUD operations |

## Verification

After implementing fixes:
1. Unit tests: token expiry detection, refresh trigger, fallback behavior
2. Integration: simulate expired Copilot token, verify auto-refresh or fallback
3. E2E: `e2e-tests/specs/api/provider-status.spec.ts` — extend to verify refresh behavior
4. Manual: connect Copilot, wait for token expiry, confirm requests still work
