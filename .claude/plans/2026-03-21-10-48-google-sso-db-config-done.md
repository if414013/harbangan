# Plan: Centralize Google SSO Configuration in Admin Web UI

Move Google SSO configuration (client_id, client_secret, callback_url, enable/disable) from environment variables to database-backed admin UI. Remove all GOOGLE_* env vars.

## Scope Constraints
- No backward compat — no env var fallback, no migration scripts
- No existing data — fresh database
- Proxy-only mode — unaffected (no DB, no SSO, no web UI)

## Key Decisions
- Google SSO disabled by default on first run (empty fields, `auth_google_enabled=false`)
- Admin logs in with password+TOTP — NO forced password change, NO forced TOTP re-setup
- `auth_password_enabled` enforced at backend: non-admin login rejected when disabled. Admin always has password fallback.
- Backend rejects disabling both auth methods simultaneously (including clearing SSO fields when password disabled)
- `google_callback_url` empty state: treat as local dev (no Secure cookie flag, permissive CORS)

## First-Run Flow
1. Fresh DB — Google SSO disabled, fields empty
2. Login page shows password-only (no Google button)
3. Admin logs in with INITIAL_ADMIN_EMAIL + INITIAL_ADMIN_PASSWORD + TOTP → lands on dashboard (no forced password change)
4. Admin goes to Config → Authentication → sets Client ID / Client Secret / Callback URL → enables Google SSO → save
5. Login page now shows Google sign-in button

## File Manifest

| File | Action | Owner | Wave |
|------|--------|-------|------|
| `backend/src/web_ui/config_db.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/web_ui/config_api.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/config.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/web_ui/google_auth.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/web_ui/password_auth.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/main.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/middleware/mod.rs` | modify | rust-backend-engineer | 1 |
| `frontend/src/pages/Config.tsx` | modify | react-frontend-engineer | 2 |
| `.env.example` | modify | devops-engineer | 2 |
| `.env.prod.example` | modify | devops-engineer | 2 |
| `CLAUDE.md` | modify | devops-engineer | 2 |
| `.claude/agents/devops-engineer.md` | modify | devops-engineer | 2 |
| `e2e-tests/specs/api/sso-config.spec.ts` | create | frontend-qa | 3 |
| `e2e-tests/specs/ui/sso-config-flow.spec.ts` | create | frontend-qa | 3 |
| `e2e-tests/specs/ui/config.spec.ts` | modify | frontend-qa | 3 |
| `e2e-tests/playwright.config.ts` | modify | frontend-qa | 3 |
| backend test modules (config.rs, config_db.rs, config_api.rs, google_auth.rs, password_auth.rs) | modify | backend-qa | 3 |

## Wave 1: Backend — DB-Backed SSO Config

**Assigned to: rust-backend-engineer. Sequential — files are interleaved.**

### Task 1.1: Add Google SSO fields to load_into_config
- **File**: `config_db.rs` — add to `load_into_config()` after line 1024
- Map: `google_client_id` → plain string, `google_client_secret` → encrypted (existing `value_type == "encrypted"` path at line 909), `google_callback_url` → plain string
- Follow existing pattern for `qwen_oauth_client_id` etc. (lines 1022-1024)
- Add `apply_config_field` arms for all 3 fields so hot-reload works: persist to DB then re-run `load_into_config` to update in-memory Config

### Task 1.2: Add validation, classification, descriptions to config_api
- **File**: `config_api.rs`
- `validate_config_field`: Add `google_client_id` (string, max 256), `google_client_secret` (string, max 512), `google_callback_url` (string, URL format, max 512)
- `classify_config_change`: All 3 → `HotReload`
- `get_config_field_descriptions`: Human-readable descriptions for all 3
- **Auth toggle guard**: When `auth_google_enabled` or `auth_password_enabled` is set to `false`, check the other is `true` AND properly configured. Also reject clearing SSO fields (e.g. `google_client_id=""`) when `auth_password_enabled=false`. Edge case: clearing a required SSO field when password is disabled must be blocked.

### Task 1.3: Mask secret in GET /config response
- When serializing config, mask `google_client_secret`: show `"••••{last4}"` if set, empty string if not
- Mask in config history entries too
- PUT with a value matching the mask pattern → treat as no-op (don't overwrite). Use prefix `"••••"` as sentinel — if value starts with `"••••"`, skip the update.

### Task 1.4: Remove google_* from Config struct and env var loading
- **File**: `config.rs`
- Remove fields from struct definition (lines 91-93)
- Remove from `Config::load()` env var reading (lines 274-277)
- Remove from `Config::with_defaults()` (lines 196-198)
- Remove from `Debug` impl (lines 131-133)
- Remove `validate()` auth method check entirely (lines 305-332) — auth config now lives in DB, not available at startup validation time. Keep only proxy mode validation. The "must have at least one auth method" guard is now runtime-only via PUT /config (Task 1.2).
- Update all test struct literals in config.rs and google_auth.rs that reference google_* fields

**Wait — correction from backend review**: The google_* fields should STAY on Config struct since `load_into_config` populates them and handlers read via `state.config.read()`. Only remove the env var loading (lines 274-277) and the startup validation auth check. Keep the struct fields, with_defaults (empty strings), and Debug impl (redacted secret).

### Task 1.5: Handle empty callback_url for cookies/CORS
- **File**: `google_auth.rs:82-84` — Change `is_local_dev()` to treat empty string as local dev:
```rust
fn is_local_dev(callback_url: &str) -> bool {
    callback_url.is_empty()
        || callback_url.starts_with("http://localhost")
        || callback_url.starts_with("http://127.0.0.1")
}
```
- This covers `password_auth.rs:140` and `middleware/mod.rs:255-258` too since they call the same function or use similar logic. Verify all callers handle empty gracefully.

### Task 1.6: Fix admin seeding — no forced password change
- **File**: `config_db.rs` — Change `create_password_user` to accept `must_change_password: bool` parameter (currently hardcoded `true` at line 1200-1201)
- **File**: `main.rs` seeding — pass `false` for seeded admin
- **File**: `password_auth.rs:662` (admin creates user via UI) — pass `true` (admin-created users must change password)

### Task 1.7: Backend enforcement of auth_password_enabled with admin exemption
- **File**: `password_auth.rs` — In the login handler (`POST /auth/login`, around line 231):
  - After password verification succeeds, check `auth_password_enabled` from DB config
  - If `false` AND user role is NOT admin → reject with 403 "Password authentication is disabled"
  - If `false` AND user role IS admin → allow login (admin fallback)
  - Currently `auth_password_enabled` is only used in the status endpoint (frontend gating) — this adds actual backend enforcement

### Verification
```bash
cd backend && cargo clippy --all-targets  # zero warnings
cd backend && cargo fmt --check           # no diffs
cd backend && cargo test --lib            # zero failures
```

## Wave 2: Frontend + DevOps — UI Fields and Env Cleanup

**Runs in parallel with Wave 1.**

### Task 2.1: Add SSO fields to Config page (react-frontend-engineer)
- **File**: `frontend/src/pages/Config.tsx` — Expand Authentication group (~line 144):
```typescript
{
  title: "Authentication",
  icon: "lock",
  fields: [
    { key: "auth_google_enabled", label: "Google SSO", type: "checkbox" },
    { key: "google_client_id", label: "Google Client ID", type: "text" },
    { key: "google_client_secret", label: "Google Client Secret", type: "password" },
    { key: "google_callback_url", label: "Google Callback URL", type: "text" },
    { key: "auth_password_enabled", label: "Password Auth", type: "checkbox" },
  ],
}
```
- Existing renderer handles all field types automatically. Password reveal/hide built in.
- Dirty tracking (`changedKeysSet`) already prevents sending unchanged masked secret values.
- No new components, styles, routes, or API functions needed.

### Task 2.2: Remove GOOGLE_* env vars from docs (devops-engineer)
- `.env.example:10-13` — Remove `# Google SSO (required)` comment + 3 vars
- `.env.prod.example:8-11` — Remove same
- `CLAUDE.md` — Remove GOOGLE_* from Environment Variables table, note SSO is configured via admin UI
- `.claude/agents/devops-engineer.md:49-51` — Remove GOOGLE_* from env var table

### Verification
```bash
cd frontend && npm run build   # zero errors
cd frontend && npm run lint    # zero errors
```

## Wave 3: Testing — Full Coverage

**Depends on Wave 1 + Wave 2. Backend-qa and frontend-qa can run in parallel within this wave.**

### Task 3.1: Backend unit tests (~35 tests) (backend-qa)

**config_db.rs:**
- `test_load_into_config_google_client_id` — set in DB, verify loaded
- `test_load_into_config_google_client_secret_encrypted` — set encrypted, verify decrypted
- `test_load_into_config_google_callback_url` — set in DB, verify loaded
- `test_load_into_config_google_sso_all_three` — all 3 set, all loaded
- `test_load_into_config_google_sso_partial` — only client_id → others stay empty
- `test_load_into_config_google_secret_no_encryption_key` — value skipped gracefully
- `test_create_password_user_must_change_false` — seeded admin path
- `test_create_password_user_must_change_true` — admin-created user path

**config_api.rs:**
- `test_validate_google_client_id_valid` / `_empty` / `_non_string`
- `test_validate_google_client_secret_valid`
- `test_validate_google_callback_url_valid` / `_invalid`
- `test_classify_google_client_id_hot_reload`
- `test_classify_google_client_secret_hot_reload`
- `test_classify_google_callback_url_hot_reload`
- `test_reject_disable_both_auth_methods` — both to false → 400
- `test_reject_clear_sso_when_password_disabled` — clear google_client_id when auth_password_enabled=false → 400
- `test_allow_disable_password_when_sso_configured` — works when google fully configured + enabled

**config.rs:**
- `test_validate_no_auth_at_startup` — empty config valid (setup mode, no auth check at startup)
- `test_validate_proxy_mode_only` — proxy mode validation still works

**google_auth.rs:**
- `test_status_google_configured_from_db` — all 3 fields → `google_configured: true`
- `test_status_google_not_configured` — empty → `google_configured: false`
- `test_status_google_partially_configured` — only client_id → `google_configured: false`
- `test_status_auth_google_enabled_requires_configured` — enabled but not configured → effectively false
- `test_is_local_dev_empty_string` — empty returns true (new fallback)

**password_auth.rs:**
- `test_admin_login_when_password_disabled` — admin role bypasses check → login succeeds
- `test_non_admin_login_blocked_when_password_disabled` — non-admin → 403
- `test_login_works_when_password_enabled` — normal behavior preserved

**Secret masking:**
- `test_google_client_secret_masked_in_get_config` — GET shows "••••last4"
- `test_google_client_secret_masked_value_noop_on_put` — PUT with masked sentinel skips update
- `test_google_client_id_stored_plain` — not encrypted
- `test_google_callback_url_stored_plain` — not encrypted

### Task 3.2: E2E tests (~59 scenarios) (frontend-qa)

**New file: `e2e-tests/specs/api/sso-config.spec.ts`** (serial, api-mutating):
1. GET /config returns google_client_id, google_client_secret (masked), google_callback_url with empty defaults
2. PUT google_client_id → persisted, verified via GET
3. PUT google_client_secret → persisted encrypted, GET returns masked "••••{last4}"
4. PUT google_callback_url → persisted, verified via GET
5. PUT all three → all persist
6. GET /config/schema includes 3 fields with type/description/requires_restart
7. Config history records SSO changes, secret masked in history
8. Secret masking: GET never returns full secret
9. PUT with masked sentinel value → no-op, original secret unchanged
10. Validation: control chars rejected, max length enforced
11. GET /status returns google_configured: true after all 3 set
12. GET /status returns google_configured: false when any field empty
13. Reject disabling both auth methods → 400
14. Reject clearing SSO field when password disabled → 400
15. Non-admin cannot PUT SSO config → 403
16. Unauthenticated → 401
17. PUT without CSRF → 403
18. Admin disables auth_password_enabled → admin can still log in via API with password
19. Non-admin login rejected via API when auth_password_enabled=false

**Extend `e2e-tests/specs/ui/config.spec.ts`** (ui-admin):
20. Authentication group renders google_client_id (text), google_client_secret (password), google_callback_url (text)
21. Admin edits SSO fields → save → toast → reload → persisted
22. Secret field shows masked value with reveal/hide toggle
23. Unsaved changes indicator on edit without save
24. Admin-created user gets forced password change on first login
25. Seeded admin does NOT get forced password change

**New file: `e2e-tests/specs/ui/sso-config-flow.spec.ts`** (serial, ui-admin):
26. Full flow: set all 3 SSO fields + enable toggle → save → login page shows Google button
27. Disable SSO → save → login page hides Google button
28. Toggle round-trip: enable → verify → disable → verify → re-enable → verify
29. Guard: unchecking both auth toggles → save → error toast
30. Fresh start: config page loads with empty SSO fields, login page password-only
31. Admin configures SSO → login page immediately shows Google button
32. Admin disables password auth → admin can still log in with password (UI test)
33. google_configured status: false when fields empty, true when all 3 set

**Register new spec files in `e2e-tests/playwright.config.ts`:**
- `sso-config.spec.ts` → `api-mutating` project testMatch
- `sso-config-flow.spec.ts` → `ui-admin` project testMatch

### Verification
```bash
cd backend && cargo test --lib       # unit tests
cd e2e-tests && npm run test:api     # API E2E
cd e2e-tests && npm run test:ui      # UI E2E
```

## Interface Contracts

### GET /config response (new fields)
```json
{
  "config": {
    "google_client_id": "123456.apps.googleusercontent.com",
    "google_client_secret": "••••cret",
    "google_callback_url": "http://localhost:9999/_ui/api/auth/google/callback"
  }
}
```

### PUT /config (unchanged pattern, new keys)
```json
{ "google_client_id": "123456.apps.googleusercontent.com" }
{ "google_client_secret": "GOCSPX-actual-secret" }
{ "google_callback_url": "http://localhost:9999/_ui/api/auth/google/callback" }
```
PUT with `"••••..."` prefix → no-op (sentinel detection).

### Auth toggle guard (400 rejection)
```json
PUT { "auth_password_enabled": false }
→ 400 { "error": "Cannot disable both authentication methods" }
```
Also rejects clearing SSO fields when password disabled.

### GET /status (unchanged shape)
```json
{
  "google_configured": true,
  "auth_google_enabled": true,
  "auth_password_enabled": true
}
```

### classify_config_change
All 3 SSO fields → `HotReload`

### Login rejection (password disabled, non-admin)
```
POST /auth/login → 403 { "error": "Password authentication is disabled" }
```
Admin role exempted — login succeeds regardless.

## Team Sizing

| Wave | Agent | Complexity | Parallel? |
|------|-------|------------|-----------|
| 1 | rust-backend-engineer | Medium (7 files, 7 tasks, sequential) | — |
| 2 | react-frontend-engineer | XS (3 lines in 1 file) | Parallel with Wave 1 |
| 2 | devops-engineer | XS (remove lines from 4 files) | Parallel with Wave 1 |
| 3 | backend-qa | Medium (~35 unit tests) | Parallel with frontend-qa |
| 3 | frontend-qa | Medium (~59 E2E scenarios) | Parallel with backend-qa |

## Recommended Preset
`/team-implement --preset fullstack`
