# Google SSO + Multi-User Enterprise Gateway

## Context

The gateway is currently single-user: one `proxy_api_key` protects everything, one Kiro token handles all backend calls. We're converting it to a multi-user enterprise system where users sign in with Google, each user manages their own Kiro token and API keys, and admins control access.

**Google OAuth creds**: via env vars (`GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `GOOGLE_CALLBACK_URL`)
**Domain restriction**: configurable allowlist in Web UI (admin only); empty allowlist = open to all Google accounts (bootstrap mode)
**Roles**: Admin (first user, atomically assigned) + User
**API keys**: per-user, mapped to user's Kiro token
**Breaking change**: This replaces the old single `proxy_api_key` auth entirely. No backward compatibility layer.

---

## 1. Database Schema (inline migrations in `run_migrations()`)

UUIDs generated in Rust via `Uuid::new_v4()` — no `pgcrypto` extension needed. Follow existing `config_db.rs` inline migration pattern (not sqlx CLI).

```sql
-- Users (populated from Google profile on first login)
CREATE TABLE IF NOT EXISTS users (
    id          UUID PRIMARY KEY,              -- generated in Rust
    email       TEXT UNIQUE NOT NULL,
    name        TEXT NOT NULL,
    picture_url TEXT,
    role        TEXT NOT NULL DEFAULT 'user' CHECK (role IN ('admin', 'user')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_login  TIMESTAMPTZ
);

-- Sessions (web UI auth, cookie-based)
CREATE TABLE IF NOT EXISTS sessions (
    id         UUID PRIMARY KEY,               -- generated in Rust
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,            -- 24h from creation, sliding on activity
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);

-- Per-user Kiro tokens (from device code flow)
CREATE TABLE IF NOT EXISTS user_kiro_tokens (
    user_id        UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    refresh_token  TEXT NOT NULL,               -- TODO: encrypt at rest with ENCRYPTION_KEY env var (AES-256-GCM)
    access_token   TEXT,                        -- TODO: encrypt at rest
    token_expiry   TIMESTAMPTZ,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Per-user API keys (for /v1/* endpoints)
CREATE TABLE IF NOT EXISTS api_keys (
    id         UUID PRIMARY KEY,               -- generated in Rust
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    key_hash   TEXT UNIQUE NOT NULL,            -- SHA-256 hash, looked up via DB index
    key_prefix TEXT NOT NULL,                   -- first 8 chars for display
    label      TEXT NOT NULL DEFAULT '',
    last_used  TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);
CREATE INDEX IF NOT EXISTS idx_api_keys_user ON api_keys(user_id);

-- Domain allowlist (admin-managed, empty = allow all Google accounts)
CREATE TABLE IF NOT EXISTS allowed_domains (
    domain     TEXT PRIMARY KEY,                -- stored lowercase, exact match only
    added_by   UUID REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

**Migration from existing schema**: Existing `config`, `config_history`, and `schema_version` tables remain unchanged. The old `proxy_api_key` in the config table is no longer used. Bump `schema_version` to 3. New tables use `CREATE TABLE IF NOT EXISTS` — safe for re-runs.

**`setupComplete` redefinition**: The current `is_setup_complete()` checks for `proxy_api_key` existence in the config table. This must be replaced. New definition: **setup is complete when at least one admin user exists** (`SELECT EXISTS(SELECT 1 FROM users WHERE role = 'admin')`). Expose via a public endpoint `GET /_ui/api/status` returning `{ "setup_complete": bool }` — no auth required, used by the frontend to decide whether to show the login page or the setup flow.

**Removed from original plan**: `user_kiro_tokens.region` field — all users share the gateway's global `kiro_region` config. Per-user regions would require dynamic `KiroHttpClient` endpoint construction, which is out of scope for v1.

### Legacy code removal scope (breaking change)

Removing `proxy_api_key` has a blast radius of 50+ references across the codebase. All of the following must be deleted or refactored — assign explicitly to work streams:

| File | What to remove/refactor | Stream |
|------|------------------------|--------|
| `src/web_ui/routes.rs` | ~400 lines of old Kiro SSO OAuth endpoints (`/oauth/start`, `/oauth/callback`, `/oauth/device/poll`), `save_initial_setup()` handler, `save_oauth_setup()` handler | Stream 2 |
| `src/web_ui/config_db.rs` | `is_setup_complete()` (replace with admin-exists check), `save_initial_setup()`, `save_oauth_setup()`, any `proxy_api_key` get/set methods | Stream 1 |
| `src/web_ui/config_api.rs` | `proxy_api_key` validation logic, config field descriptions referencing it | Stream 3 |
| `src/middleware/mod.rs` | Entire old `auth_middleware` that compares against single `proxy_api_key` | Stream 2 |
| `src/config.rs` | `proxy_api_key` field, env var loading, CLI arg | Stream 1 |
| `src/routes/mod.rs` | `auth_manager` field in AppState (replaced by per-user creds), global token usage in handlers | Stream 2 |
| `src/auth/` | Evaluate whether `AuthManager` is still needed at all, or if it's fully replaced by per-user token management in `user_kiro.rs` | Stream 2 |
| `web-ui/src/pages/Setup.tsx` | Password/API key step (entire first step of wizard) | Stream 4 |
| `web-ui/src/components/AuthGate.tsx` | Delete entirely (replaced by `SessionGate.tsx`) | Stream 4 |
| `web-ui/src/lib/auth.ts` | `getApiKey()`, `setApiKey()`, sessionStorage usage | Stream 4 |

---

## 2. Backend Architecture

### New Rust modules

| Module | Purpose |
|--------|---------|
| `src/web_ui/google_auth.rs` | Google OIDC flow (redirect, callback, token exchange, ID token validation) |
| `src/web_ui/session.rs` | Session CRUD, cookie management, session middleware, expired session cleanup |
| `src/web_ui/users.rs` | User CRUD, role management, domain validation (lowercase, exact match) |
| `src/web_ui/api_keys.rs` | API key generation (256-bit entropy), hashing, listing, revocation (max 10/user) |
| `src/web_ui/user_kiro.rs` | Per-user Kiro token storage, device code flow per user, background token refresh |

### Modified modules

| File | Changes |
|------|---------|
| `src/middleware/mod.rs` | Replace single-key auth with API key hash lookup → user → Kiro token resolution. Accept keys via `Authorization: Bearer`, `x-api-key` header, and query param (kept for non-browser API clients). Remove old `proxy_api_key` path entirely. |
| `src/web_ui/mod.rs` | Add new routes, replace auth_middleware with session_middleware for UI routes, register Stream 3 routes |
| `src/web_ui/routes.rs` | Refactor setup flow, add user management endpoints |
| `src/web_ui/config_db.rs` | Add inline migration for new tables (version 3), new query methods |
| `src/web_ui/config_api.rs` | Add domain allowlist config, restrict config writes to admin |
| `src/config.rs` | Add `google_client_id`, `google_client_secret`, `google_callback_url` fields (bootstrap from env). Remove `proxy_api_key`. Startup validation: if `web_ui_enabled && google_client_id.is_empty()`, **error and refuse to start** (Google SSO is the only auth path — without it the gateway is completely unusable). `GOOGLE_CALLBACK_URL` is **required** when `GOOGLE_CLIENT_ID` is set (no default — `SERVER_HOST=0.0.0.0` in Docker makes any default broken). |
| `src/routes/mod.rs` | Update AppState: add `session_cache: Arc<DashMap<Uuid, SessionInfo>>`, `api_key_cache: Arc<DashMap<String, (Uuid, Uuid)>>` (key_hash → user_id, key_id), `oauth_pending: Arc<DashMap<String, OAuthPendingState>>` (state param → nonce + PKCE verifier, 10-min TTL). Refactor handlers to read per-user Kiro creds from request extensions instead of `state.auth_manager` |
| `src/http_client.rs` | Decouple from `AuthManager` — accept per-request token instead of holding global `auth_manager` reference. 403-retry logic must use the per-user token refresh callback, not the global token |
| `src/error.rs` | Add error variants: `Forbidden` (403), `SessionExpired`, `DomainNotAllowed`, `KiroTokenRequired`, `KiroTokenExpired`, `LastAdmin` (409) |
| `Cargo.toml` | Add `openidconnect = "4"`, `axum-extra = { version = "0.7", features = ["cookie"] }`, `subtle = "2"`, `dashmap = "6"` (if not already present) |

### New API endpoints

```
# Google SSO (public)
GET  /_ui/api/auth/google          → redirect to Google consent screen
GET  /_ui/api/auth/google/callback → exchange code, validate ID token, create session, set cookie
POST /_ui/api/auth/logout          → destroy session

# Session info (session-authenticated)
GET  /_ui/api/auth/me              → current user info + role

# User management (admin only)
GET  /_ui/api/users                → list all users
PUT  /_ui/api/users/:id/role       → change user role (MUST reject if target is last admin — prevent lockout)
DELETE /_ui/api/users/:id          → remove user (MUST reject if target is last admin — prevent lockout; also evict from session_cache)

# Domain allowlist (admin only)
GET  /_ui/api/domains              → list allowed domains
POST /_ui/api/domains              → add domain
DELETE /_ui/api/domains/:domain    → remove domain

# Per-user Kiro token (session-authenticated, own user)
GET  /_ui/api/kiro/status          → has token? expired?
POST /_ui/api/kiro/setup           → start device code flow
POST /_ui/api/kiro/poll            → poll device code
DELETE /_ui/api/kiro/token         → remove own Kiro token

# API key management (session-authenticated, own user)
GET  /_ui/api/keys                 → list own API keys
POST /_ui/api/keys                 → generate new key (returns plaintext once, max 10/user)
DELETE /_ui/api/keys/:id           → revoke key (also evict from api_key_cache immediately)
```

### Security requirements

**CSRF protection**: All state-changing endpoints require a `X-CSRF-Token` header that matches a token stored in the session. Double-submit pattern: CSRF token set as a non-HttpOnly cookie, frontend reads it and sends as header. Additionally, session cookie uses `SameSite=Strict`.

**Session cookie attributes**: `HttpOnly; Secure; SameSite=Strict; Path=/_ui; Max-Age=86400`. Note: `Secure` flag prevents cookies over plain HTTP, which blocks Vite dev server (port 5173). For local development, conditionally omit `Secure` when `GOOGLE_CALLBACK_URL` starts with `http://localhost` or `http://127.0.0.1`. Document that production deployments must always use HTTPS.

**CORS update** (blocking prerequisite): Replace `allow_origin(Any)` with specific origin echoing + `allow_credentials(true)`. The CORS layer must validate the `Origin` header against the gateway's external URL, derived from `GOOGLE_CALLBACK_URL` (strip the path to get the origin, e.g. `https://gateway.example.com:9001`). This avoids the `SERVER_HOST=0.0.0.0` problem in Docker. Without this, `credentials: 'include'` and `EventSource({ withCredentials: true })` will not work.

**API key hashing**: SHA-256 with constant-time comparison (`subtle::ConstantTimeEq`) on the hash lookup result. Keys generated with 256 bits of entropy from `OsRng`.

### Auth flow changes

**Web UI auth** (session-based):
```
Browser → GET /_ui/api/auth/google
  → 302 to accounts.google.com (with PKCE + state + nonce)
  → User consents
  → GET /_ui/api/auth/google/callback?code=...&state=...
  → Validate state parameter against stored OAUTH_PENDING (DashMap<String, OAuthPendingState> on AppState, keyed by state param, with 10-minute TTL; background cleanup task or check-on-read expiry)
  → Exchange code for tokens via Google token endpoint
  → Validate ID token (openidconnect handles this):
    ✓ Signature via Google JWKS
    ✓ iss == "https://accounts.google.com"
    ✓ aud == GOOGLE_CLIENT_ID
    ✓ exp not expired
    ✓ nonce matches
    ✓ email_verified == true (CRITICAL — reject unverified emails)
  → Extract email, lowercase the domain part
  → Domain check: if allowed_domains is empty → allow (bootstrap mode)
                   if allowed_domains has entries → exact match required (reject subdomains)
  → Atomic first-user-admin upsert (use SERIALIZABLE isolation or pg_advisory_xact_lock):
    BEGIN ISOLATION LEVEL SERIALIZABLE;  -- prevents two concurrent txns both seeing COUNT(*)=0
    INSERT INTO users (id, email, name, picture_url, role)
    VALUES ($1, $2, $3, $4,
      CASE WHEN (SELECT COUNT(*) FROM users) = 0 THEN 'admin' ELSE 'user' END)
    ON CONFLICT (email) DO UPDATE SET last_login = NOW(), name = $3, picture_url = $4
    RETURNING id, role;
    COMMIT;
    -- On serialization failure (40001), retry the transaction.
  → Create session (fresh UUID), set cookie
  → If role == 'admin' AND first login → 302 to /_ui/admin (setup domains)
  → Else → 302 to /_ui/
  → On domain validation failure → 302 to /_ui/login?error=domain_not_allowed
  → On consent denied by user → 302 to /_ui/login?error=consent_denied
  → On invalid/expired state parameter → 302 to /_ui/login?error=invalid_state
  → On email_verified == false → 302 to /_ui/login?error=email_not_verified
  → On any other OAuth error → 302 to /_ui/login?error=auth_failed
```

**/v1/* API auth** (API key-based):
```
Client sends Authorization: Bearer <api_key>  OR  x-api-key: <api_key>
  → auth_middleware:
    SHA-256 hash token → look up api_keys table (via cache, 60s TTL)
      → Found: resolve user_id → look up user_kiro_tokens (via cache)
        → Token valid: inject UserKiroCreds into request extensions
        → Token expired: attempt refresh, if fails return 403 {"error": "kiro_token_expired"}
        → No token: return 403 {"error": "kiro_token_required"}
      → Not found: return 401
  → Handler reads UserKiroCreds from request extensions
```

### Background services

**Session cleanup**: `tokio::spawn` a task that runs every hour: `DELETE FROM sessions WHERE expires_at < NOW()`. Also check `expires_at` on every session lookup (reject expired inline).

**Per-user token refresh**: `tokio::spawn` a task that runs every 5 minutes: query `user_kiro_tokens` where `token_expiry < NOW() + interval '5 minutes'`, attempt refresh using the stored `refresh_token`. On failure, mark the token as expired (clear `access_token`, set `token_expiry = NULL`). Log structured events for audit.

### Crates to add

```toml
openidconnect = "4"                                    # Google OIDC (includes oauth2 crate, handles JWKS + token validation)
axum-extra = { version = "0.7", features = ["cookie"] } # Cookie jar extractors for Axum 0.7
subtle = "2"                                            # Constant-time comparison for API key hashes
dashmap = "6"                                           # Concurrent hash maps for session/API key caches (verify not already in Cargo.toml)
```

Note: `jsonwebtoken` dropped — `openidconnect` v4 handles all ID token validation internally.

---

## 3. Frontend Architecture

### New pages

| Page | Route | Purpose |
|------|-------|---------|
| `src/pages/Login.tsx` | `/login` | Google SSO button, CRT terminal style. Shows error messages for `domain_not_allowed`, `consent_denied`, `invalid_state`, `email_not_verified`, `auth_failed` from query params |
| `src/pages/Profile.tsx` | `/profile` | Kiro token setup + API key management |
| `src/pages/Admin.tsx` | `/admin` | User list, role management, domain allowlist. Shows first-time setup banner when no domains configured yet (replaces vestigial `/setup` route) |

### New components

| Component | Purpose |
|-----------|---------|
| `src/components/SessionGate.tsx` | Replaces `AuthGate.tsx` — checks session cookie via `/auth/me`, redirects to `/login`. Provides user context (id, email, role) to children |
| `src/components/AdminGuard.tsx` | Wraps admin-only routes, checks role from SessionGate context |
| `src/components/ApiKeyManager.tsx` | Generate, list, copy, revoke API keys |
| `src/components/KiroSetup.tsx` | Per-user Kiro device code flow (extracted from Setup.tsx) |
| `src/components/UserTable.tsx` | Admin user list with role toggles |
| `src/components/DomainManager.tsx` | Admin domain allowlist CRUD |

### Modified files

| File | Changes |
|------|---------|
| `src/App.tsx` | New routes: /login, /profile, /admin. Replace AuthGate with SessionGate. Remove /setup route (merged into /admin with first-time banner) |
| `src/lib/auth.ts` | Replace API key storage with session-based auth (cookie is automatic). `authHeaders()` returns `{}` — cookies handle auth. Add CSRF token header from cookie. Delete `getApiKey()`, `setApiKey()`, sessionStorage usage |
| `src/lib/api.ts` | Add `credentials: 'include'` to fetch for cookie auth. Add `X-CSRF-Token` header to all non-GET requests. Handle 401 → redirect to /login. Handle 403 with specific error codes |
| `src/lib/useSSE.ts` | Switch from `?api_key=` to cookie-based auth: `new EventSource(url, { withCredentials: true })`. On 401/403, stop retrying and redirect to /login |
| `src/pages/Setup.tsx` | Delete — functionality merged into Admin.tsx with first-time setup banner |
| `src/pages/Dashboard.tsx` | Show per-user metrics for regular users, all-user metrics for admins (requires MetricsCollector changes) |
| `src/pages/Config.tsx` | Admin-only guard |
| `src/components/Layout.tsx` | Add nav links for Profile, Admin (if admin). Add user avatar + logout |
| `src/styles/components.css` | Styles for new components (login, profile, admin, API key cards) |

### Routing structure

```tsx
<Routes>
  <Route path="login" element={<Login />} />
  <Route element={<SessionGate><Layout /></SessionGate>}>
    <Route index element={<Dashboard />} />
    <Route path="config" element={<AdminGuard><Config /></AdminGuard>} />
    <Route path="profile" element={<Profile />} />
    <Route path="admin" element={<AdminGuard><Admin /></AdminGuard>} />
  </Route>
</Routes>
```

Note: `/setup` route removed — it overlaps entirely with `/admin`. First-admin flow goes straight to `/admin` with a first-time setup banner when no domains are configured. `SessionGate` fetches `GET /_ui/api/status` to check `setup_complete`; if false and user is admin, redirects to `/admin`.

### Error UX

| Scenario | User sees |
|----------|-----------|
| Domain not in allowlist | Login page with: "Your email domain is not authorized. Contact your admin." |
| Consent denied | Login page with: "Google sign-in was cancelled." |
| Invalid/expired state | Login page with: "Login session expired. Please try again." |
| Email not verified | Login page with: "Your Google email is not verified." |
| Generic auth failure | Login page with: "Authentication failed. Please try again." |
| Kiro token missing (API) | `403 {"error": "kiro_token_required", "message": "Set up your Kiro token at /_ui/profile"}` |
| Kiro token expired (API) | `403 {"error": "kiro_token_expired", "message": "Re-authenticate your Kiro token at /_ui/profile"}` |
| API key revoked (SSE) | SSE stream closes, `useSSE.ts` detects 401/403, redirects to /login |
| Session expired (web UI) | `SessionGate` gets 401 from `/auth/me`, redirects to /login |
| Last admin demotion/deletion | `409 {"error": "last_admin", "message": "Cannot remove or demote the last admin user"}` |

---

## 4. Deployment

### Docker/deployment changes

- `docker-compose.yml`: Add `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `GOOGLE_CALLBACK_URL` to environment section
- `.env.example`: Update with new required vars and documentation
- Startup validation: if `web_ui_enabled && google_client_id.is_empty()`, **error and refuse to start** — Google SSO is the only auth path, so the gateway is unusable without it
- `GOOGLE_CALLBACK_URL` is **required** when `GOOGLE_CLIENT_ID` is set — no default. `SERVER_HOST=0.0.0.0` in Docker makes any auto-derived default broken. Must match Google Cloud Console authorized redirect URI.

---

## 5. Additional Scoped Work

### Handler refactor (Stream 2 scope)

`chat_completions_handler` and `anthropic_messages_handler` in `src/routes/mod.rs` currently hardcode `state.auth_manager.read().await` to get the Kiro access token. These must be refactored to:
1. Read `UserKiroCreds` from request extensions (injected by new auth middleware)
2. Use per-user token for the backend call
3. Remove the old global `auth_manager` usage from handlers

**KiroHttpClient coupling**: The HTTP client's 403-retry logic internally calls `self.auth_manager.get_access_token()` to refresh the global token. This must be decoupled — make `KiroHttpClient` token-agnostic by accepting a per-request token (or a token-refresh callback) instead of holding a reference to `AuthManager`. This is more than ~50 lines per handler; budget for refactoring `http_client.rs` as well.

**Model cache bootstrap**: `get_models_handler` reads from `state.model_cache`, populated at startup using the global `AuthManager`. With no global Kiro token, the model cache needs a new population strategy. Options: (a) populate lazily on first request from any authenticated user, (b) use the first admin's token after setup, or (c) add a `GET /v1/models` that fetches live using the requesting user's token (no cache). Recommend option (a) — lazy populate on first authenticated request, then refresh periodically using any available valid user token.

### MetricsCollector changes (Stream 1 scope)

`MetricsCollector` currently tracks global aggregates only. Changes needed:
- Add `user_id: Option<Uuid>` parameter to `record_request_start()` and `record_request_end()`
- Add `per_user_stats: DashMap<Uuid, DashMap<String, ModelStats>>` field
- Add `get_user_stats(user_id: Uuid)` method
- SSE metrics endpoint filters by authenticated user for non-admins
- Dashboard.tsx conditionally renders user-scoped vs global metrics

### Audit logging

Add structured `tracing` events for security-relevant actions:
- `info!(email = %email, role = %role, "user_login")`
- `info!(email = %email, "user_login_domain_rejected")`
- `info!(admin = %admin_email, target = %user_email, new_role = %role, "user_role_changed")`
- `info!(user = %email, key_prefix = %prefix, "api_key_created")`
- `info!(user = %email, key_prefix = %prefix, "api_key_revoked")`
- `warn!(user = %email, "kiro_token_refresh_failed")`

---

## 6. Work Stream Decomposition (4 agents)

### Stream 1: Database + User Model (backend foundation)
**Files owned:**
- `src/web_ui/users.rs` (new)
- `src/web_ui/session.rs` (new)
- `src/web_ui/config_db.rs` (migration additions, replace `is_setup_complete()` with admin-exists check)
- `src/config.rs` (add google_*, google_callback_url fields + startup validation errors. Remove proxy_api_key)
- `src/routes/mod.rs` (AppState changes: add caches, add `oauth_pending: Arc<DashMap<String, OAuthPendingState>>`, add UserKiroCreds type)
- `src/error.rs` (add Forbidden, SessionExpired, DomainNotAllowed, KiroTokenRequired, KiroTokenExpired, LastAdmin)
- `src/metrics/collector.rs` (add user_id parameter + per-user stats)
- `Cargo.toml` (new dependencies: openidconnect, axum-extra 0.7, subtle, dashmap if missing)

**Delivers:** User/session/api_key tables, CRUD operations, AppState with caches, error types, per-user metrics, `GET /_ui/api/status` endpoint

### Stream 2: Google OAuth + Session Middleware (backend auth)
**Files owned:**
- `src/web_ui/google_auth.rs` (new — OIDC flow with full ID token validation, SERIALIZABLE first-user-admin insert)
- `src/web_ui/mod.rs` (route registration for ALL new routes — coordinate with Stream 3 on route definitions)
- `src/web_ui/routes.rs` (delete ~400 lines of old OAuth code, refactored setup, handler refactor to use request extensions)
- `src/middleware/mod.rs` (API key hash lookup → user → Kiro token resolution + CORS update to specific origin + credentials. Accept `Authorization: Bearer`, `x-api-key`, and query param)
- `src/http_client.rs` (decouple from AuthManager — accept per-request token, refactor 403-retry to use per-user token refresh)
- `src/auth/` (evaluate whether AuthManager is still needed; if fully replaced by per-user tokens, remove or gut it)

**Blocked by:** Stream 1 (needs user/session types, DB methods, error types)

**Delivers:** Google SSO endpoints, session middleware with CSRF, API key auth middleware, CORS fix, handler refactor for per-user tokens, decoupled HTTP client

### Stream 3: Per-User Kiro + API Keys (backend features)
**Files owned:**
- `src/web_ui/user_kiro.rs` (new — includes background token refresh task)
- `src/web_ui/api_keys.rs` (new — 256-bit entropy, max 10/user, constant-time hash comparison. Evict from `api_key_cache` on key deletion)
- `src/web_ui/config_api.rs` (domain allowlist config, admin-only writes. Remove old `proxy_api_key` validation logic)

**Blocked by:** Stream 1 (needs user model and DB)

**Coordination:** Provide route handler functions to Stream 2 for registration in `mod.rs`. User deletion handler (Stream 1) must evict from `session_cache`; key deletion handler (this stream) must evict from `api_key_cache`.

**Delivers:** Kiro device code per user, background token refresh, API key CRUD with cache eviction, domain allowlist endpoints

### Stream 4: Frontend (all UI changes)
**Files owned:**
- `web-ui/src/pages/Login.tsx` (new — with error display for domain_not_allowed, consent_denied, invalid_state, email_not_verified, auth_failed)
- `web-ui/src/pages/Profile.tsx` (new)
- `web-ui/src/pages/Admin.tsx` (new — includes first-time setup banner when no domains configured)
- `web-ui/src/components/SessionGate.tsx` (new — fetches `/status` for setup_complete, `/auth/me` for session)
- `web-ui/src/components/AdminGuard.tsx` (new)
- `web-ui/src/components/ApiKeyManager.tsx` (new)
- `web-ui/src/components/KiroSetup.tsx` (new)
- `web-ui/src/components/UserTable.tsx` (new)
- `web-ui/src/components/DomainManager.tsx` (new)
- `web-ui/src/App.tsx` (remove /setup route, add /login /profile /admin)
- `web-ui/src/lib/auth.ts` (CSRF token header from cookie. Delete getApiKey/setApiKey/sessionStorage)
- `web-ui/src/lib/api.ts` (credentials: 'include', CSRF header, error code handling)
- `web-ui/src/lib/useSSE.ts` (withCredentials: true, stop retry on 401/403)
- `web-ui/src/pages/Setup.tsx` (delete — functionality merged into Admin.tsx)
- `web-ui/src/pages/Dashboard.tsx` (per-user vs global metrics)
- `web-ui/src/pages/Config.tsx`
- `web-ui/src/components/Layout.tsx`
- `web-ui/src/components/AuthGate.tsx` (delete entirely — replaced by SessionGate)
- `web-ui/src/styles/components.css`

**Blocked by:** Streams 2 & 3 (needs API contract finalized)

**Can start early:** TypeScript interfaces and component skeletons with mocked data while waiting for backend

**Delivers:** Complete frontend with Google login, profile, admin, API key management, error UX

### File ownership conflicts to coordinate

| File | Primary owner | Also touched by | Resolution |
|------|--------------|-----------------|------------|
| `src/web_ui/mod.rs` | Stream 2 | Stream 3 | Stream 3 provides route handler fns, Stream 2 registers them |
| `src/routes/mod.rs` | Stream 1 | Stream 2 | Stream 1 adds AppState fields + UserKiroCreds type + oauth_pending, Stream 2 refactors handlers |
| `Cargo.toml` | Stream 1 | Stream 2 (if axum-extra needed early) | Stream 1 adds all deps upfront |
| `src/http_client.rs` | Stream 2 | Stream 1 (AppState) | Stream 2 decouples from AuthManager, Stream 1 provides new AppState fields |

---

## 7. Verification

1. `cargo clippy` — no warnings
2. `cargo test --lib` — all tests pass (including new auth middleware tests)
3. `cargo build --release` — compiles
4. `cd web-ui && npm run build && npm run lint` — frontend builds clean
5. `docker compose up --build` — full stack runs
6. Manual test flow:
   - Google SSO login → first user is admin → redirected to /admin
   - Set domain allowlist → second user login → verify domain check
   - Each user: Kiro device code setup → generate API key → `curl /v1/chat/completions` with personal API key
   - Verify SSE streams work with cookie auth (no more `?api_key=` in URLs)
   - Verify `x-api-key` header works for API auth (Anthropic client compat)
   - Verify `GET /_ui/api/status` returns `{ "setup_complete": true/false }` without auth
   - Verify model cache populates on first authenticated request
7. Security checks:
   - Verify CSRF token required on all POST/PUT/DELETE
   - Verify `SameSite=Strict` on session cookie
   - Verify `Secure` flag conditional on GOOGLE_CALLBACK_URL scheme (omitted for localhost)
   - Verify CORS rejects `Origin: evil.com` with credentials
   - Verify CORS origin derived from GOOGLE_CALLBACK_URL, not SERVER_HOST
   - Verify unverified Google emails rejected
   - Verify subdomain `evil.example.com` doesn't match allowlist `example.com`
   - Verify first-user-admin race: two concurrent logins → only one admin (SERIALIZABLE isolation)
   - Verify last-admin protection: cannot demote or delete the sole admin
   - Verify API key cache evicted immediately on key revocation
   - Verify session cache evicted on user deletion
   - Verify GOOGLE_CLIENT_ID empty → startup error (not warning)
   - Verify GOOGLE_CALLBACK_URL required when GOOGLE_CLIENT_ID set
   - Verify OAuth callback handles all error codes: consent_denied, invalid_state, email_not_verified
8. Legacy cleanup verification:
   - `grep -r proxy_api_key src/` returns zero matches
   - `grep -r auth_manager src/routes/` returns zero matches (unless AuthManager is intentionally kept for a different purpose)
   - Old OAuth routes (`/oauth/start`, `/oauth/callback`, `/oauth/device/poll`) return 404
   - `AuthGate.tsx` deleted, `Setup.tsx` deleted
