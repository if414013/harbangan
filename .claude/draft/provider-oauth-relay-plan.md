# Plan: Provider OAuth (Relay Pattern) + Profile Page Merge

## Context

Two changes to the multi-provider implementation:
1. **Merge `/providers` into `/profile`** — per-user config belongs alongside Kiro token and API keys.
2. **Replace API key input with OAuth browser flow** — no static API keys stored or entered anywhere.

---

## Key Insight: Relay Pattern Unlocks All Three Providers

The CLI tool OAuth client IDs (from CLIProxyAPI) are registered with `localhost` redirect URIs. A server-side callback would fail. But a **relay helper script** runs on the user's machine, captures the localhost callback, and POSTs the authorization code to rkgw. This means:

- **No app registration needed** — reuse the same public client IDs as CLIProxyAPI
- **Full per-user isolation** — each user's tokens stored separately in PostgreSQL
- **Works for all three providers** — Anthropic, Gemini, OpenAI

### OAuth Client IDs (from CLIProxyAPI source, already public)

| Provider | Client ID | Client Secret | Callback Port |
|----------|-----------|---------------|---------------|
| Anthropic | `9d1c250a-e61b-44d9-88ed-5944d1962f5e` | none (public client) | 54545 |
| Gemini | `<GEMINI_OAUTH_CLIENT_ID>` | `<GEMINI_OAUTH_CLIENT_SECRET>` | 8085 |
| OpenAI | `app_EMoamEEZ73f0CkXaXp7hrann` | none (public client) | 1455 |

Hardcode as defaults; allow override via env vars (`ANTHROPIC_OAUTH_CLIENT_ID`, `GEMINI_OAUTH_CLIENT_ID`, `GEMINI_OAUTH_CLIENT_SECRET`, `OPENAI_OAUTH_CLIENT_ID`).

---

## Relay Flow (per provider, same pattern)

```
1. User clicks "connect" on profile page
2. rkgw: generate PKCE verifier+challenge, state, relay_token (UUID, 10-min TTL)
          invalidate any existing pending relay for this (user_id, provider) pair
          store { pkce_verifier, state, user_id, provider } in provider_oauth_pending DashMap
          return { relay_script_url }
3. Frontend: show modal with command:
          curl -fsSL https://{DOMAIN}/_ui/api/providers/anthropic/relay-script?token=... | sh
4. User runs script on their machine:
   a. Script starts local HTTP server on localhost:54545
   b. Opens browser to https://claude.ai/oauth/authorize?...
   c. User authorizes → provider redirects to http://localhost:54545/callback?code=...&state=...
   d. Script captures code+state, POSTs { relay_token, code, state } to rkgw relay endpoint
   e. Script checks HTTP response status — prints error if not 200
5. rkgw relay endpoint:
   a. Validate relay_token → look up { pkce_verifier, user_id, provider } in provider_oauth_pending (single-use: consumed on first POST)
   b. Exchange code: POST to provider token URL with code + verifier + redirect_uri=http://localhost:{port}/callback
   c. Fetch user email from provider userinfo endpoint
   d. Store tokens in user_provider_tokens table (retry once on DB failure)
   e. Invalidate provider cache for user
6. Frontend polls GET /providers/status every 2s → detects connected → shows success
   - Polling stops on modal close or component unmount (useEffect cleanup)
   - 10-min frontend timeout matching relay_token TTL → shows "Connection timed out, try again"
   - Cancel button in modal stops polling immediately
```

---

## What Changes

### Remove entirely
- `backend/src/web_ui/provider_keys.rs` — API key CRUD endpoints
- `backend/src/providers/key_detection.rs` — key format detection
- `frontend/src/pages/Providers.tsx` — standalone providers page
- `frontend/e2e/specs/providers.spec.ts` — E2E tests for old providers page
- Providers route in `App.tsx` and nav link in `Sidebar.tsx`
- API key types/functions in `api.ts` (`addProviderKey`, `removeProviderKey`, `ProviderInfo.key_prefix`)
- Orphaned CSS in `components.css`: `.providers-page`, `.providers-grid`, `.provider-key-form`, `.provider-key-input`, `.provider-key-prefix`, `.provider-key-value`, `.provider-models`, `.provider-models-label`, `.provider-models-list`, `.provider-model-tag` (keep `.provider-card` and `.provider-actions` if reused in Profile)

### Add
- `backend/src/web_ui/provider_oauth.rs` — relay OAuth flow for all three providers
- DB migration v8: `user_provider_tokens` table
- PROVIDERS section in `Profile.tsx`
- Relay script template (shell script, served dynamically by rkgw)

---

## Implementation Plan

### Phase 1 — Backend: DB Migration v8

File: `backend/src/web_ui/config_db.rs`

Add `migrate_to_v8()`:
```sql
CREATE TABLE IF NOT EXISTS user_provider_tokens (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider_id   TEXT NOT NULL CHECK (provider_id IN ('anthropic', 'gemini', 'openai')),
    access_token  TEXT NOT NULL,
    refresh_token TEXT NOT NULL DEFAULT '',
    expires_at    TIMESTAMPTZ NOT NULL,
    email         TEXT NOT NULL DEFAULT '',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, provider_id)
);
```

New DB methods:
- `upsert_user_provider_token(user_id, provider_id, access_token, refresh_token, expires_at, email)` — only update `refresh_token` if the new value is non-empty (preserve existing refresh token on re-authorization)
- `get_user_provider_token(user_id, provider_id)` → `Option<(access_token, refresh_token, expires_at, email)>`
- `delete_user_provider_token(user_id, provider_id)`
- `get_user_connected_providers(user_id)` → `Vec<(provider_id, email)>`

### Phase 2 — Backend: OAuth Endpoints

New file: `backend/src/web_ui/provider_oauth.rs`

**Pending state:** Create a new `ProviderOAuthPendingState` struct (separate from Google SSO's `OAuthPendingState`):
```rust
struct ProviderOAuthPendingState {
    pkce_verifier: String,
    user_id: Uuid,
    provider: String,
    created_at: DateTime<Utc>,
}
```
Add `provider_oauth_pending: Arc<DashMap<String, ProviderOAuthPendingState>>` to `AppState`. Do NOT reuse the existing `oauth_pending` DashMap — the Google SSO flow has a different struct shape (`nonce` field, no `user_id`/`provider`).

**Token exchange abstraction:** Extract token exchange into a trait for testability:
```rust
#[async_trait]
trait TokenExchanger: Send + Sync {
    async fn exchange_code(&self, provider: &ProviderId, code: &str, pkce_verifier: &str, redirect_uri: &str) -> Result<TokenExchangeResult, ApiError>;
    async fn refresh_token(&self, provider: &ProviderId, refresh_token: &str) -> Result<TokenExchangeResult, ApiError>;
}
```
Production impl makes real HTTP calls. Test impl returns canned responses. Store `Arc<dyn TokenExchanger>` in AppState.

**Provider path validation:** All `{provider}` path params must be validated against `["anthropic", "gemini", "openai"]` early in each handler. Return 400 for unknown providers.

Routes (session-authenticated):
- `GET /_ui/api/providers/status` — returns `{ providers: { anthropic: { connected, email }, gemini: { connected, email }, openai: { connected, email } } }`
- `GET /_ui/api/providers/{provider}/connect` — generates PKCE + relay_token, returns `{ relay_script_url }`
- `GET /_ui/api/providers/{provider}/relay-script?token={relay_token}` — serves shell script (no session auth, relay_token IS the auth)
- `POST /_ui/api/providers/{provider}/relay` — receives `{ relay_token, code, state }`, exchanges tokens, stores (no session auth, relay_token IS the auth)
- `DELETE /_ui/api/providers/{provider}` — disconnect (CSRF-protected)

**Relay script** (served dynamically, token + auth URL baked in):
```sh
#!/bin/sh
# rkgw provider relay helper — runs on your machine, relays OAuth code to rkgw
AUTH_URL="<baked-in auth URL with PKCE challenge>"
RELAY_URL="https://{DOMAIN}/_ui/api/providers/{provider}/relay"
RELAY_TOKEN="<baked-in relay token>"
PORT=<provider port>

# Preflight checks
command -v python3 >/dev/null 2>&1 || { echo "Error: python3 is required but not found."; exit 1; }

python3 -c "
import http.server, urllib.parse, json, urllib.request, sys, os, socket, time

# Check port availability before starting server
try:
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(('localhost', $PORT))
    s.close()
except OSError:
    print('Error: Port $PORT is already in use. Close the conflicting process and try again.')
    sys.exit(1)

class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        p = urllib.parse.urlparse(self.path)
        q = urllib.parse.parse_qs(p.query)
        code = q.get('code',[''])[0]
        state = q.get('state',[''])[0]
        data = json.dumps({'relay_token':'$RELAY_TOKEN','code':code,'state':state}).encode()
        req = urllib.request.Request('$RELAY_URL', data=data, headers={'Content-Type':'application/json'})
        for attempt in range(2):
            try:
                resp = urllib.request.urlopen(req, timeout=10)
                if resp.status == 200:
                    self.send_response(200); self.end_headers()
                    self.wfile.write(b'<h2>Connected! You can close this window.</h2>')
                else:
                    self.send_response(200); self.end_headers()
                    self.wfile.write(b'<h2>Error: server returned ' + str(resp.status).encode() + b'</h2>')
                    print('Error: relay server returned HTTP ' + str(resp.status))
                break
            except Exception as e:
                if attempt == 0:
                    time.sleep(2)
                else:
                    self.send_response(200); self.end_headers()
                    self.wfile.write(b'<h2>Error connecting to server</h2>')
                    print('Error: ' + str(e))
        os._exit(0)
    def log_message(self, *a): pass

http.server.HTTPServer(('localhost', $PORT), H).handle_request()
" &
PY_PID=$!
if command -v open >/dev/null 2>&1; then open "$AUTH_URL"
elif command -v xdg-open >/dev/null 2>&1; then xdg-open "$AUTH_URL"
else echo "Open in browser: $AUTH_URL"; fi
echo "Waiting for authorization..."
wait $PY_PID
echo "Done! Provider connected."
```

**DOMAIN sanitization:** When generating the relay script, validate that `DOMAIN` contains only `[a-zA-Z0-9._:-]` characters to prevent shell injection via misconfigured domain values.

**Token exchange per provider:**

| Provider | Token URL | redirect_uri | Auth header |
|----------|-----------|-------------|-------------|
| Anthropic | `https://api.anthropic.com/v1/oauth/token` | `http://localhost:54545/callback` | none (public client) |
| Gemini | `https://oauth2.googleapis.com/token` | `http://localhost:8085/oauth2callback` | Basic auth (client_id:secret) |
| OpenAI | `https://auth.openai.com/oauth/token` | `http://localhost:1455/auth/callback` | none (public client) |

**Userinfo endpoints:**
- Anthropic: decode JWT from `id_token` field in token response
- Gemini: `GET https://www.googleapis.com/oauth2/v3/userinfo`
- OpenAI: decode JWT from `id_token` field in token response

**Token refresh** (called by `ensure_fresh_token()` before registry lookup):
- POST to provider token URL with `grant_type=refresh_token`
- Update `user_provider_tokens` row (only update `refresh_token` column if response includes one)
- **Refresh locking:** Use `DashMap<(Uuid, String), Arc<tokio::sync::Mutex<()>>>` to prevent concurrent refresh for the same user+provider. First caller acquires lock, refreshes, updates DB+cache. Second caller waits, then reads fresh token from cache.
- On permanent refresh failure (revoked access, expired refresh token): delete the token row, invalidate cache, fall back to Kiro

Update `backend/src/web_ui/mod.rs`: swap `provider_keys` routes for `provider_oauth` routes.

### Phase 3 — Backend: Registry Update

File: `backend/src/providers/registry.rs`

- Load from `user_provider_tokens` instead of `user_provider_keys`
- Keep `resolve_provider` as a pure cache/DB lookup (no side effects)
- Move refresh logic to a separate `ensure_fresh_token(user_id, provider, db, exchanger)` function called at the handler level before `resolve_provider` — keeps the registry testable
- Cache: `(user_id, ProviderId)` → `(access_token, expires_at)` with 5-min TTL

Delete `backend/src/providers/key_detection.rs`.

### Phase 4 — Frontend: Remove Providers Page

- Delete `frontend/src/pages/Providers.tsx`
- Delete `frontend/e2e/specs/providers.spec.ts`
- `frontend/src/App.tsx`: remove `<Route path="providers" element={<Providers />} />` and the import
- `frontend/src/components/Sidebar.tsx`: remove providers NavLink
- `frontend/src/styles/components.css`: remove orphaned CSS classes (`.providers-page`, `.providers-grid`, `.provider-key-form`, `.provider-key-input`, `.provider-key-prefix`, `.provider-key-value`, `.provider-models`, `.provider-models-label`, `.provider-models-list`, `.provider-model-tag`). Keep `.provider-card` and `.provider-actions` if reused in Profile.

### Phase 5 — Frontend: PROVIDERS Section in Profile

File: `frontend/src/pages/Profile.tsx`

Add `PROVIDERS` section after `API KEYS`, following the `KiroSetup` card pattern. Only external providers (anthropic, gemini, openai) — Kiro is handled by the existing KIRO TOKEN section above.

```
PROVIDERS

> anthropic
  [CONNECTED] user@anthropic.com   [$ disconnect]
  — or —
  [NOT CONNECTED]                  [$ connect →]
    ↓ (after clicking connect)
  Modal: "Run this in your terminal:"
  ┌──────────────────────────────────────────────────────────────┐
  │ curl -fsSL https://{domain}/_ui/api/providers/anthropic/... │  [copy]
  └──────────────────────────────────────────────────────────────┘
  [Waiting for authorization... ⠋]  [cancel]
  - Polls /providers/status every 2s
  - useEffect cleanup clears interval on unmount or modal close
  - 10-min timeout → "Connection timed out. Click connect to try again."
  - Copy button with "copied!" feedback (reuse DeviceCodeDisplay pattern)

> gemini   (same pattern)
> openai   (same pattern)
```

File: `frontend/src/lib/api.ts`
- Remove: `addProviderKey`, `removeProviderKey`, `ProviderInfo`, `ProvidersStatusResponse`, `AddProviderKeyResponse`
- Add:
  ```ts
  interface ProviderStatus { connected: boolean; email?: string }
  interface ProvidersStatus { providers: Record<string, ProviderStatus> }
  getProvidersStatus(): Promise<ProvidersStatus>
  getProviderConnectUrl(provider: string): Promise<{ relay_script_url: string }>
  disconnectProvider(provider: string): Promise<{ ok: boolean }>
  ```

---

## Critical Files

| File | Action |
|------|--------|
| `backend/src/web_ui/config_db.rs` | Add migration v8, new token DB methods |
| `backend/src/web_ui/provider_oauth.rs` | New — relay OAuth flow |
| `backend/src/web_ui/provider_keys.rs` | Delete |
| `backend/src/web_ui/mod.rs` | Swap route registration |
| `backend/src/routes/mod.rs` | Add `provider_oauth_pending` DashMap + `token_exchanger` to AppState |
| `backend/src/providers/registry.rs` | Load OAuth tokens, add `ensure_fresh_token()` |
| `backend/src/providers/key_detection.rs` | Delete |
| `frontend/src/pages/Profile.tsx` | Add PROVIDERS section |
| `frontend/src/pages/Providers.tsx` | Delete |
| `frontend/e2e/specs/providers.spec.ts` | Delete |
| `frontend/src/App.tsx` | Remove providers route |
| `frontend/src/components/Sidebar.tsx` | Remove providers nav link |
| `frontend/src/lib/api.ts` | Replace key API with OAuth API |
| `frontend/src/styles/components.css` | Remove orphaned provider-key CSS classes |

## Patterns to Reuse

- `backend/src/web_ui/google_auth.rs` — PKCE generation, DashMap TTL/cap pattern, state validation (but use a **separate** `provider_oauth_pending` DashMap, not the existing `oauth_pending`)
- `backend/src/auth/` — token refresh pattern
- `frontend/src/components/KiroSetup.tsx` — connect/disconnect card UI pattern
- `frontend/src/components/DeviceCodeDisplay` — copy-to-clipboard pattern for the curl command

---

## Verification

### Automated checks
1. `cargo test --lib` — all tests pass (including new relay endpoint tests)
2. `cargo clippy` — no warnings
3. `npm run lint && npm run build` — clean

### Relay endpoint tests (new)
4. Valid relay_token + valid code → tokens stored, 200
5. Expired relay_token (>10 min) → 410 Gone
6. Unknown relay_token (random UUID) → 401
7. Relay_token already consumed (second POST) → 401
8. State parameter mismatch → 400
9. Provider path param doesn't match stored provider → 400
10. Invalid provider path param (e.g., `/providers/foobar/connect`) → 400
11. Token exchange failure → error propagated, no garbage stored
12. DB write failure after token exchange → retry once, then return error

### Token refresh tests (new)
13. Expired access_token → `ensure_fresh_token` refreshes transparently
14. Concurrent refresh for same user+provider → only one HTTP call (mutex)
15. Permanent refresh failure (revoked) → token row deleted, falls back to Kiro

### Manual flow
16. `/profile` shows PROVIDERS section with Anthropic, Gemini, OpenAI cards (not Kiro)
17. Click "connect →" → modal shows relay script command with copy button
18. Copy button copies full curl command, shows "copied!" feedback
19. Run script in terminal → browser opens provider consent screen
20. Authorize → terminal prints "Done! Provider connected."
21. Profile card updates to CONNECTED with email (within 2s poll)
22. Close modal without completing → polling stops, no leaked intervals
23. Wait 10 min without completing → frontend shows timeout message
24. `claude-*` → Anthropic, `gemini-*` → Gemini, `gpt-*` → OpenAI
25. Disconnect → NOT CONNECTED, requests fall back to Kiro
26. `/providers` route returns 404
27. No API key input exists anywhere in the UI

### Script robustness
28. Run relay script without python3 → clear error message, script exits
29. Run relay script with port already in use → clear error message
30. Network failure during relay POST → script retries once, then shows error
