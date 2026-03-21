# Plan: E2E Test Gap Coverage

## Context

A 5-agent gap analysis revealed **40 of 65 backend endpoints (62%) have zero E2E coverage**, 2 frontend pages are completely untested, 12 tests are broken/stubbed, and 10 critical user flows have no end-to-end test. A DashMap deadlock bug in `evict_user_caches()` blocks 3 endpoint tests. This plan addresses all identified gaps in priority order.

### Review Findings Addressed

1. **Playwright config uses hardcoded testMatch** — Wave 0 adds a harness task to register all new UI specs in `playwright.config.ts` project testMatch arrays
2. **Task 1.3 logout mixes API + browser** — Split into API-only spec (session invalidation) + UI spec (redirect behavior)
3. **Setup-only mode needs own Playwright project** — Wave 4 creates a separate `playwright.setup-mode.config.ts` with its own globalSetup that skips admin seeding
4. **Shared admin state + fullyParallel = race conditions** — Cross-file serialization requires `workers: 1`, not just `fullyParallel: false`. Wave 0 adds `api-mutating` project (`workers: 1`) and sets `ui-admin` to `workers: 1`. All mutating tests (including logout-redirect and profile-actions) route to these serial lanes. `ui-authenticated` stays parallel for read-only rendering only.
5. **Task 4.1 duplicates config/schema** — Dropped `config/schema` from Task 4.1, kept only `system` and `auth/me`

## Consultation Summary

- **rust-be**: 65 endpoints. Middleware: setup_guard, auth/session/csrf/admin. DashMap deadlock in `evict_user_caches()`.
- **react-fe**: 10 routes, 50+ API functions. Usage and UserDetail pages have zero coverage. SSE hook unused.
- **fe-qa**: ~53 API tests, ~150+ UI tests. 12 fixme tests. Hardcoded `testMatch` in 3 Playwright projects. `fullyParallel: true` globally.
- **be-qa**: 884 unit tests. Near-zero handler-level tests. password_auth has 7 untested handlers.
- **devops**: Setup-only mode and proxy-only mode untested. No frontend Docker health check.

## Blocking Bug: DashMap Deadlock

**File**: `backend/src/web_ui/password_auth.rs` — `evict_user_caches()` function
**Issue**: Holds DashMap `Ref` guard while calling `.retain()`, causing deadlock.
**Impact**: Blocks 3 E2E tests + 9 admin-users stubs depend on working password flows.
**Must fix before**: Wave 2 tests can run.

---

## Wave 0: Test Harness Updates
**Priority**: P0 | **Agents**: frontend-qa (harness)

### Task 0.1 — Update Playwright config: new projects + serialization (frontend-qa)
- **File**: `e2e-tests/playwright.config.ts` (modify)
- **Action**:
  1. **Add new `api-mutating` project** — `dependencies: ['api']`, `workers: 1`, `fullyParallel: false`. Runs all state-mutating API specs one file at a time, one test at a time:
     ```ts
     {
       name: 'api-mutating',
       dependencies: ['api', 'ui-public', 'ui-authenticated'],
       testDir: './specs/api',
       testMatch: [
         'api-keys.spec.ts', 'logout.spec.ts', 'domain-allowlist.spec.ts',
         'user-management.spec.ts', 'guardrails.spec.ts',
         'model-registry.spec.ts', 'provider-status.spec.ts'
       ],
       fullyParallel: false,
       use: {
         baseURL: GATEWAY_URL,
         storageState: '.auth/session.json',  // admin session cookies for /_ui/api/* endpoints
       },
     }
     ```
     **Note**: The existing `api` project uses bearer auth (`Authorization` header) for `/v1/*` proxy routes. This new project uses `storageState` with admin session cookies for `/_ui/api/*` web UI routes. They are different auth mechanisms.
     **CSRF**: Backend requires `X-CSRF-Token` header matching the `csrf_token` cookie on all POST/PUT/DELETE/PATCH to `/_ui/api/*` (see `backend/src/web_ui/google_auth.rs:735`). Existing tests handle this by extracting the cookie after login (see `e2e-tests/specs/api/config.spec.ts:25`). Task 0.3 adds a shared CSRF helper for the new specs.
  2. **Set `ui-admin` to `workers: 1` and `dependencies: ['api-mutating']`** — `workers: 1` prevents cross-file overlap within the project. `dependencies: ['api-mutating']` ensures `ui-admin` waits until `api-mutating` finishes, preventing cross-project races on shared state (users, keys, domains, roles, config). The full dependency chain is: `api` + `ui-public` + `ui-authenticated` (parallel) → `api-mutating` (serial) → `ui-admin` (serial).
  3. **Register new UI specs in testMatch**:
     - Add to `ui-authenticated`: `'usage.spec.ts'`, `'models.spec.ts'` (NOT currently listed at line 52 — existing file is a dead spec today)
     - Add to `ui-admin`: `'user-detail.spec.ts'`, `'logout-redirect.spec.ts'`, `'profile-actions.spec.ts'`
     - **Rationale**: `logout-redirect` and `profile-actions` mutate shared state (session invalidation, API key CRUD). They MUST run in the serialized `ui-admin` lane, not in `ui-authenticated` which remains parallel for read-only rendering tests.
  4. **Existing projects unchanged**:
     - `api` — parallel, bearer auth, read-only + existing specs
     - `ui-public` — parallel, no auth
     - `ui-authenticated` — parallel, read-only rendering tests only (no mutations)
- **Size**: S

### Task 0.2 — Create setup-mode Playwright config (frontend-qa)
- **File**: `e2e-tests/playwright.setup-mode.config.ts` (create)
- **Action**: New config that:
  - Has NO `globalSetup` (skips admin seeding entirely)
  - Points to a single project `setup-mode` with `testDir: './specs/api'` and `testMatch: ['setup-mode.spec.ts']`
  - Uses env vars: `GATEWAY_URL` pointing to a backend with empty DB
  - Add npm script: `"test:setup-mode": "playwright test --config playwright.setup-mode.config.ts"`
- **File**: `e2e-tests/package.json` (modify) — add the npm script
- **Size**: S

### Task 0.3 — Add CSRF helper for mutating API specs (frontend-qa)
- **File**: `e2e-tests/helpers/csrf.ts` (create)
- **Action**: Extract the CSRF pattern used in `config.spec.ts:25` into a reusable helper. The helper should:
  1. Accept a `storageState` path or cookie jar
  2. Extract the `csrf_token` cookie value
  3. Return headers object `{ 'X-CSRF-Token': token, 'Cookie': 'kgw_session=...; csrf_token=...' }` ready for `fetch()` calls
  - Existing pattern in `config.spec.ts`: logs in via `adminLogin()`, extracts cookies, manually builds headers for each `fetch()`. This is duplicated across every spec that hits mutating endpoints. A shared helper eliminates boilerplate and ensures all new `api-mutating` specs handle CSRF correctly.
- **Size**: XS

---

## Wave 1: Fix Blocking Bug + Auth/Security E2E Tests
**Priority**: P0 | **Agents**: rust-backend-engineer, frontend-qa

### Task 1.1 — Fix DashMap deadlock in evict_user_caches (rust-backend-engineer)
- **File**: `backend/src/web_ui/password_auth.rs`
- **Action**: Fix `evict_user_caches()` — collect matching keys into a Vec first, then iterate and remove individually (don't hold Ref guard during mutation)
- **Verify**: `cd backend && cargo clippy --all-targets && cargo test --lib`
- **Size**: XS

### Task 1.2 — API key lifecycle E2E (frontend-qa)
- **File**: `e2e-tests/specs/api/api-keys.spec.ts` (create)
- **Action**: `test.describe.serial` — create key → list keys (verify present) → use key for `GET /v1/models` (verify 200) → delete key → retry `GET /v1/models` (verify 401/403)
- **Endpoints**: `POST /_ui/api/keys`, `GET /_ui/api/keys`, `DELETE /_ui/api/keys/:id`, `GET /v1/models`
- **Size**: M

### Task 1.3a — Logout API E2E (frontend-qa)
- **File**: `e2e-tests/specs/api/logout.spec.ts` (create)
- **Action**: API-only test (no browser). Login via fetch → get session cookies → POST logout → verify `GET /_ui/api/auth/me` returns 401 with the old session cookie.
- **Endpoints**: `POST /_ui/api/auth/logout`, `GET /_ui/api/auth/me`
- **Size**: XS

### Task 1.3b — Logout redirect UI E2E (frontend-qa)
- **File**: `e2e-tests/specs/ui/logout-redirect.spec.ts` (create)
- **Project**: `ui-admin` (workers: 1, serial) — NOT `ui-authenticated`
- **Action**: Creates its own fresh session in `beforeAll` via `adminLogin()` helper (must NOT use the shared `storageState` since logout invalidates the session). Then: navigate to a protected page → click logout button in sidebar → verify redirect to `/login` → attempt navigating to protected page → verify redirect to `/login`.
- **Critical**: This test invalidates a session. Using the shared admin session would break all subsequent tests. The fresh-session pattern isolates the damage.
- **Size**: S

### Task 1.4 — Domain allowlist CRUD E2E (frontend-qa)
- **File**: `e2e-tests/specs/api/domain-allowlist.spec.ts` (create)
- **Action**: `test.describe.serial` — list domains → add `test-e2e.example.com` → verify in list → remove it → verify removed. Separate describe for RBAC: create non-admin user session, verify 403 on POST/DELETE.
- **Endpoints**: `GET /_ui/api/domains`, `POST /_ui/api/domains`, `DELETE /_ui/api/domains/:domain`
- **Size**: S

### Task 1.5 — Unblock password-auth fixme tests (frontend-qa)
- **File**: `e2e-tests/specs/api/password-auth.spec.ts` (modify)
- **Action**: Convert 3 `test.fixme` to real tests: admin reset password, change password wrong current, change password valid+revert
- **Depends on**: Task 1.1 (deadlock fix must be deployed to Docker)
- **Size**: S

---

## Wave 2: Admin Features E2E Tests
**Priority**: P1 | **Agents**: frontend-qa
**Depends on**: Wave 1 complete (especially Task 1.1 deadlock fix)

### Task 2.1 — Implement admin-users.spec.ts stubs (frontend-qa)
- **File**: `e2e-tests/specs/ui/admin-users.spec.ts` (modify)
- **Action**: `test.describe.serial` for the full block. Implement all 9 `test.fixme` stubs:
  1. Create user (form fill → submit → verify in table)
  2. Force password change flag
  3. Reset password (modal → submit → verify)
  4. Auth method column display
  5. 2FA status display
  6. Toggle password auth enabled
  7. Toggle 2FA requirement
  8. Form validation (empty fields, invalid email)
  9. Duplicate email rejection
- **Size**: L

### Task 2.2 — User management API E2E (frontend-qa)
- **File**: `e2e-tests/specs/api/user-management.spec.ts` (create)
- **Action**: `test.describe.serial` — create test user → list users (verify present) → get user detail → change role to admin → verify → change back → delete user → verify gone. Separate describe for RBAC (non-admin 403).
- **Endpoints**: `GET /_ui/api/users`, `GET /_ui/api/users/:id`, `PUT /_ui/api/users/:id/role`, `DELETE /_ui/api/users/:id`
- **Size**: M

### Task 2.3 — Guardrails CRUD E2E (frontend-qa)
- **File**: `e2e-tests/specs/api/guardrails.spec.ts` (create)
- **Action**: `test.describe.serial` — Test full guardrails API lifecycle:
  - Profile CRUD: create → list → get → update → delete
  - Rule CRUD: create (with CEL expression, linked to profile) → list → get → update → delete
  - CEL validation: valid expression → 200, invalid → 400
  - Test profile: submit content → verify response shape (action, response_time_ms)
  - RBAC: non-admin gets 403 on all mutating endpoints
- **Endpoints**: All 11 guardrails endpoints under `/_ui/api/guardrails/*`
- **Size**: L

### Task 2.4 — Guardrails UI functional test (frontend-qa)
- **File**: `e2e-tests/specs/ui/guardrails.spec.ts` (modify)
- **Action**: Extend rendering-only tests with mocked functional tests: new profile form → submit (mocked 200) → verify toast, new rule form with CEL textarea → validate button (mocked) → submit, test panel interaction.
- **Size**: M

---

## Wave 3: Untested Pages + Provider/Model E2E
**Priority**: P1 | **Agents**: frontend-qa
**Serialization**: All API specs (3.3, 3.4) land in `api-mutating` (workers:1). UI specs: 3.1 (usage — read-only) in `ui-authenticated` (parallel); 3.2 (user-detail — mutating) and 3.5 (profile-actions — mutating) in `ui-admin` (workers:1). Dependency chain ensures `api-mutating` finishes before `ui-admin` starts — no cross-project races.

### Task 3.1 — Usage page E2E (frontend-qa)
- **File**: `e2e-tests/specs/ui/usage.spec.ts` (create)
- **Registered in**: Task 0.1 adds to `ui-authenticated.testMatch`
- **Action**: Test page rendering (summary cards, data table), date picker interaction, group-by select switching (day/model/provider), admin tabs visibility (My Usage / Global / Per-User). Use `test.describe.serial` for tab interactions.
- **API coverage**: `GET /_ui/api/usage`, `GET /_ui/api/admin/usage`, `GET /_ui/api/admin/usage/users`
- **Size**: M

### Task 3.2 — UserDetail page E2E (frontend-qa)
- **File**: `e2e-tests/specs/ui/user-detail.spec.ts` (create)
- **Registered in**: Task 0.1 adds to `ui-admin.testMatch`
- **Action**: `test.describe.serial` — Navigate from admin page → click user row → verify detail page (account card, role badge, API keys table, remove button). Test role toggle. Test back navigation.
- **API coverage**: `GET /_ui/api/users/:id`, `PUT /_ui/api/users/:id/role`
- **Note**: Do NOT test delete here (would break other tests using shared users) — delete is covered in Task 2.2 API test.
- **Size**: M

### Task 3.3 — Model registry API E2E (frontend-qa)
- **File**: `e2e-tests/specs/api/model-registry.spec.ts` (create)
- **Action**: `test.describe.serial` — Two tiers of coverage:
  - **Always runs**: `GET /models/registry` (list, verify response shape — may be empty). `POST /models/registry/populate` (call it — handler returns 200 even with zero models added, so assert 200 + response shape only). `PATCH /models/registry/:id` and `DELETE /models/registry/:id` are **conditional**: skip if registry is empty after populate (use `test.skip` with reason).
  - **Conditional (models exist)**: If populate returned models or registry already has entries: disable a model → verify `enabled: false` → re-enable → delete → verify removed. Verify `/v1/models` excludes disabled model.
  - **Why conditional**: `populate` depends on a connected provider returning models. The handler returns success (200) regardless of whether models were found (`backend/src/web_ui/model_registry_handlers.rs:237`). Without a seeded provider, populate may yield zero models, making PATCH/DELETE tests vacuous.
- **Endpoints**: `GET /_ui/api/models/registry`, `POST /_ui/api/models/registry/populate`, `PATCH /_ui/api/models/registry/:id`, `DELETE /_ui/api/models/registry/:id`
- **Size**: M

### Task 3.4 — Provider status + priority API E2E (frontend-qa)
- **File**: `e2e-tests/specs/api/provider-status.spec.ts` (create)
- **Action**: Read-only shape tests for `GET /_ui/api/providers/status` (providers array shape). `test.describe.serial` for priority: get priority → update priority order → verify persisted → restore original. Test Kiro status endpoint shape.
- **Endpoints**: `GET /_ui/api/providers/status`, `GET/POST /_ui/api/providers/priority`, `GET /_ui/api/kiro/status`
- **Size**: S

### Task 3.5 — Profile page functional E2E (frontend-qa)
- **Read-only rendering** stays in `e2e-tests/specs/ui/profile.spec.ts` (ui-authenticated, parallel) — no changes needed, existing tests cover structure.
- **Mutating tests** go in new file: `e2e-tests/specs/ui/profile-actions.spec.ts` (create)
- **Project**: `ui-admin` (workers: 1, serial)
- **Action**: `test.describe.serial` — API key manager: create key → verify displayed in table → copy button works → revoke key → verify removed from table. Test Google link button visibility. Test security section conditional rendering based on auth method.
- **Why separate file**: API key create/delete mutates shared admin state. Must run in serialized `ui-admin` lane, not parallel `ui-authenticated`.
- **Size**: M

---

## Wave 4: Infrastructure + Hardening
**Priority**: P2 | **Agents**: frontend-qa, devops-engineer

### Task 4.1 — System/monitoring endpoints E2E (frontend-qa)
- **File**: `e2e-tests/specs/api/system.spec.ts` (create)
- **Action**: Test `GET /_ui/api/system` response shape (has cpu/memory/uptime fields), `GET /_ui/api/auth/me` response shape (email, role, auth_method).
- **Note**: Dropped `config/schema` — already tested in `config.spec.ts`.
- **Size**: XS

### Task 4.2 — CORS and cookie security assertions (frontend-qa)
- **File**: `e2e-tests/specs/api/security.spec.ts` (create)
- **Action**:
  - CORS: Send fetch with `Origin` header to `/v1/models` → verify `Access-Control-Allow-Origin` in response
  - Cookies: Login via fetch → inspect `Set-Cookie` headers → assert `HttpOnly`, `SameSite`, `Path=/_ui`
  - CSRF: POST to `/_ui/api/auth/logout` without CSRF token → verify 403/400
- **Size**: S

### Task 4.3 — Admin page functional E2E (frontend-qa)
- **File**: `e2e-tests/specs/ui/admin.spec.ts` (modify)
- **Action**: `test.describe.serial` — test domain manager (add domain → verify → remove), test provider pool interactions (mocked: add account → toggle → delete), test create user form validation.
- **Size**: M

### Task 4.4 — Setup-only mode E2E (frontend-qa)
- **File**: `e2e-tests/specs/api/setup-mode.spec.ts` (create)
- **Config**: Uses `playwright.setup-mode.config.ts` from Task 0.2 (no globalSetup, no admin seeding)
- **Run via**: `npm run test:setup-mode` (separate from main test suite)
- **Action**: Against a backend with empty DB:
  - `GET /_ui/api/status` → verify `setup_complete: false`
  - `POST /v1/chat/completions` → verify 503 (setup guard)
  - `GET /v1/models` → verify 503 (setup guard)
- **Prerequisite**: Requires a way to start backend with empty DB. Options: (a) separate docker-compose test profile, (b) DB reset script, (c) dedicated test container. The implementing agent should coordinate with devops on the simplest approach.
- **Size**: L

### Task 4.5 — Frontend Docker health check (devops-engineer)
- **File**: `docker-compose.yml` (modify)
- **Action**: Add health check to frontend service:
  ```yaml
  healthcheck:
    test: ["CMD", "curl", "-fs", "http://localhost:80/nginx-health"]
    interval: 30s
    timeout: 10s
    retries: 3
    start_period: 10s
  ```
- **Size**: XS

---

## File Manifest

| File | Action | Owner | Wave |
|------|--------|-------|------|
| `e2e-tests/playwright.config.ts` | modify (add `api-mutating` project, register new UI specs, set `ui-admin` serial) | frontend-qa | 0 |
| `e2e-tests/playwright.setup-mode.config.ts` | create | frontend-qa | 0 |
| `e2e-tests/helpers/csrf.ts` | create | frontend-qa | 0 |
| `e2e-tests/package.json` | modify | frontend-qa | 0 |
| `backend/src/web_ui/password_auth.rs` | modify | rust-backend-engineer | 1 |
| `e2e-tests/specs/api/api-keys.spec.ts` | create | frontend-qa | 1 |
| `e2e-tests/specs/api/logout.spec.ts` | create | frontend-qa | 1 |
| `e2e-tests/specs/ui/logout-redirect.spec.ts` | create | frontend-qa | 1 |
| `e2e-tests/specs/api/domain-allowlist.spec.ts` | create | frontend-qa | 1 |
| `e2e-tests/specs/api/password-auth.spec.ts` | modify | frontend-qa | 1 |
| `e2e-tests/specs/ui/admin-users.spec.ts` | modify | frontend-qa | 2 |
| `e2e-tests/specs/api/user-management.spec.ts` | create | frontend-qa | 2 |
| `e2e-tests/specs/api/guardrails.spec.ts` | create | frontend-qa | 2 |
| `e2e-tests/specs/ui/guardrails.spec.ts` | modify | frontend-qa | 2 |
| `e2e-tests/specs/ui/usage.spec.ts` | create | frontend-qa | 3 |
| `e2e-tests/specs/ui/user-detail.spec.ts` | create | frontend-qa | 3 |
| `e2e-tests/specs/api/model-registry.spec.ts` | create | frontend-qa | 3 |
| `e2e-tests/specs/api/provider-status.spec.ts` | create | frontend-qa | 3 |
| `e2e-tests/specs/ui/profile-actions.spec.ts` | create | frontend-qa | 3 |
| `e2e-tests/specs/api/system.spec.ts` | create | frontend-qa | 4 |
| `e2e-tests/specs/api/security.spec.ts` | create | frontend-qa | 4 |
| `e2e-tests/specs/ui/admin.spec.ts` | modify | frontend-qa | 4 |
| `e2e-tests/specs/api/setup-mode.spec.ts` | create | frontend-qa | 4 |
| `docker-compose.yml` | modify | devops-engineer | 4 |

## Team Composition

| Wave | Agents | Notes |
|------|--------|-------|
| Wave 0 | frontend-qa (harness) | Config changes first — must land before any new specs |
| Wave 1 | rust-backend-engineer + frontend-qa | Parallel: BE fixes deadlock while QA writes API tests. Task 1.5 waits for 1.1. |
| Wave 2 | frontend-qa | All mutating, all serial at runtime via `api-mutating` + `ui-admin` projects |
| Wave 3 | frontend-qa | API specs serial via `api-mutating`; UI specs serial via `ui-admin`/`ui-authenticated` |
| Wave 4 | frontend-qa + devops-engineer | devops does 4.5 independently; QA does 4.1-4.4 |

**Recommended team**: 4 agents total
1. **rust-backend-engineer** — Task 1.1 only (deadlock fix), then shutdown
2. **frontend-qa-api** — Wave 0 harness (playwright.config.ts, setup-mode config, package.json) + all API spec files (Waves 1-4)
3. **frontend-qa-ui** — All UI spec files (Waves 1-4)
4. **devops-engineer** — Task 4.5 only (Docker health check), then shutdown

Frontend-qa agents split by file type (API specs vs UI specs) to avoid write conflicts. Runtime serialization enforced by `workers: 1` on both `api-mutating` and `ui-admin` projects — this is the only way to prevent cross-file overlap. `test.describe.serial` within files is defense-in-depth for ordered lifecycle steps.

## Serialization Policy

Cross-file isolation requires `workers: 1` — `fullyParallel: false` alone only serializes tests within a file, not across files.

**`api-mutating` project** (`workers: 1`, `fullyParallel: false`) — all new mutating API specs:
- api-keys, logout, domain-allowlist, user-management, guardrails, model-registry, provider-status
- Runs AFTER `api` project completes (via `dependencies`)
- Single worker = one file at a time, one test at a time
- Uses `storageState` (admin session cookies), NOT bearer auth

**`ui-admin` project** (`workers: 1`, `fullyParallel: false`) — all admin + mutating authenticated UI specs:
- Existing: config, admin, admin-users, guardrails, multi-account
- New: user-detail, logout-redirect, profile-actions
- `logout-redirect` creates its own fresh session (does NOT use shared storageState) since it invalidates the session
- Single worker prevents cross-file collisions on users, config, domains, keys, roles

**Unchanged (parallel, no mutations)**:
- `api` — read-only + existing specs (health, auth-errors, SDK tests, config reads, password-auth). Uses bearer auth.
- `ui-public` — no auth, no mutations
- `ui-authenticated` — read-only rendering tests only (dashboard, profile rendering, navigation, provider-oauth/copilot/qwen rendering, totp-setup, password-change, usage, models)

**Rule**: If a test creates, deletes, or modifies shared state (users, keys, domains, config, roles, guardrails, sessions), it goes in `api-mutating` or `ui-admin` — never in a parallel project.

Within serial specs, `test.describe.serial` is used as defense-in-depth for ordered lifecycle tests (create → use → delete).

## Verification

After each wave:
1. `cd e2e-tests && npm test` — all E2E tests pass (including new ones)
2. `cd backend && cargo clippy --all-targets && cargo test --lib` — backend clean (Wave 1)
3. Verify no regressions in existing tests
4. Verify new UI specs appear in test output (not silently skipped due to testMatch)

For setup-mode: `cd e2e-tests && npm run test:setup-mode` (separate run)

Final verification:
- All 12 fixme tests converted to passing tests
- 16 new files created, 8 existing files modified (24 total file touches)
- Endpoint coverage: 22 → 55+ endpoints tested
- All 10 frontend pages have at least rendering + basic functional tests
- Dependency chain runs clean: `api`+`ui-public`+`ui-authenticated` (parallel) → `api-mutating` (serial) → `ui-admin` (serial)
