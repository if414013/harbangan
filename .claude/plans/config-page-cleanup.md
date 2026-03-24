# Plan: Config Page Cleanup

Remove Server Host/Port section from Config page and move Domain Allowlist from Admin to Config → Authentication section.

## Consultation Summary

- **rust-backend-engineer**: Config schema is driven by `get_config_field_descriptions()` in `config_api.rs`. Remove `server_host`/`server_port` from 3 functions (descriptions, validation, classification) + `routes.rs` response. Config struct stays unchanged — still needed for startup bind. Domain allowlist: zero backend changes — same admin+CSRF middleware already covers both Config and Admin pages.
- **react-frontend-engineer**: Config page uses hardcoded `CONFIG_GROUPS` array in `Config.tsx`. Remove Server group (~7 lines). For domain allowlist: render existing self-contained `<DomainManager />` component inside Authentication group body with a `group.title === "Authentication"` condition. Remove from `Admin.tsx`.
- **database-engineer**: Zero migrations needed. Orphaned `server_host`/`server_port` rows in config table are harmless. `allowed_domains` table unchanged.
- **devops-engineer**: Zero infrastructure changes. `SERVER_HOST`/`SERVER_PORT` are hardcoded in docker-compose, not in `.env.example`.
- **backend-qa**: 6 unit tests assert `server_host`/`server_port` in config schema — all need updating. Domain allowlist tests: 0 impact.
- **frontend-qa**: `config.spec.ts` needs Server group removed and count 8→7. `admin.spec.ts` domain tests need moving to `config.spec.ts`. API domain tests (`domain-allowlist.spec.ts`) unaffected.
- **document-writer**: 4 gh-pages docs reference domain allowlist on Admin page (web-ui.md, configuration.md, getting-started.md, troubleshooting.md). Not blocking.

## File Manifest

| File | Action | Owner | Wave |
|------|--------|-------|------|
| `backend/src/web_ui/config_api.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/web_ui/config_db.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/web_ui/routes.rs` | modify | rust-backend-engineer | 1 |
| `frontend/src/pages/Config.tsx` | modify | react-frontend-engineer | 1 |
| `frontend/src/pages/Admin.tsx` | modify | react-frontend-engineer | 1 |
| `frontend/src/components/DomainManager.tsx` | modify | react-frontend-engineer | 1 |
| `e2e-tests/specs/ui/config.spec.ts` | modify | frontend-qa | 2 |
| `e2e-tests/specs/ui/admin.spec.ts` | modify | frontend-qa | 2 |
| `gh-pages/docs/web-ui.md` | modify | document-writer | 3 |
| `gh-pages/docs/configuration.md` | modify | document-writer | 3 |
| `gh-pages/docs/getting-started.md` | modify | document-writer | 3 |
| `gh-pages/docs/troubleshooting.md` | modify | document-writer | 3 |

## Wave 1: Backend + Frontend Changes (parallel)

### 1a. Backend — Remove server_host/server_port from config UI schema
**Assigned to: rust-backend-engineer**

- [ ] `config_api.rs`: Remove `server_host` and `server_port` from `get_config_field_descriptions()` (lines 236-240)
- [ ] `config_api.rs`: Remove `"server_host"` and `"server_port"` arms from `validate_config_field()` (lines 37-38, 55-67)
- [ ] `config_api.rs`: Remove `"server_host"` and `"server_port"` from `classify_config_change()` (function at line 21)
- [ ] `routes.rs`: Remove `"server_host"` and `"server_port"` from `get_config()` JSON response (lines 72-73)
- [ ] `config_db.rs`: Remove `"server_host"` and `"server_port"` arms from `load_into_config()` (lines 957-959) — prevents invisible bind overrides from legacy DB rows (Codex finding #1)
- [ ] Update the 6 affected unit tests in `config_api.rs` and `routes.rs`:
  - `test_field_descriptions_complete` — remove from expected_keys
  - `test_classify_requires_restart` — remove server_host/server_port assertions
  - `test_validate_server_port_valid` — delete test
  - `test_validate_server_port_invalid` — delete test
  - `test_validate_string_fields` — remove server_host assertion
  - `test_get_config_schema_has_fields` — remove server_port assertion
- [ ] Add a test asserting `GET /config` response does not contain `server_host`/`server_port` keys (Codex finding #3)
- [ ] Update `config_db.rs` `load_into_config` tests that reference `server_port` (lines 4524, 4532, 4655, 4710) — remove or convert to test that unknown keys are silently ignored
- [ ] Run `cargo clippy --all-targets && cargo test --lib` — zero warnings, zero failures

**Do NOT change:**
- `Config` struct in `config.rs` — still needed for startup
- `Config::load()` — still reads env vars
- `main.rs` — still binds using these values

### 1b. Frontend — Remove Server section + Move Domain Allowlist
**Assigned to: react-frontend-engineer**

**Remove Server section from Config.tsx:**
- [ ] Delete the "Server" group object from `CONFIG_GROUPS` array (lines 26-32)
- [ ] Remove the `server` SVG icon from `ICONS` map (lines 172-187) — only used by Server group

**Move Domain Allowlist to Config → Authentication:**
- [ ] Add `import { DomainManager } from '../components/DomainManager'` to `Config.tsx`
- [ ] Render `<DomainManager />` **outside** the `<form>` element but visually within the Authentication group area. The Config page wraps fields in `<form onSubmit={handleSubmit}>` (line 437), and DomainManager's Enter key handler doesn't call `preventDefault()` — placing it inside the form would trigger config form submission when adding a domain (Codex finding #2). Two options:
  - **Option A (preferred)**: Render DomainManager after the `</form>` closing tag, inside a wrapper that visually continues the Authentication section
  - **Option B**: Add `e.preventDefault()` to DomainManager's `onKeyDown` handler (line 99) to stop form submission bubbling
- [ ] Visually verify the DomainManager renders properly inside the Authentication section

**Fix DomainManager.tsx (Codex finding #2):**
- [ ] Add `e.preventDefault()` to the `onKeyDown` handler (line 99) as a defense-in-depth measure regardless of placement approach

**Clean up Admin.tsx:**
- [ ] Remove the `// DOMAIN ALLOWLIST` section (heading + `<DomainManager />` wrapper, lines 314-317)
- [ ] Remove `DomainManager` import (line 4)
- [ ] Update `PageHeader` description to remove "domain access" reference (line 296)
- [ ] Update the setup banner text (lines 302-304) to remove "Add your organization's domain below" — after moving DomainManager, this copy is stale (Codex finding #5). Replace with text directing to Config → Authentication for domain setup.

**No changes to:**
- `api.ts` — endpoints unchanged

- [ ] Run `npm run build && npm run lint` — zero errors

## Wave 2: E2E Test Updates (depends on Wave 1)

**Assigned to: frontend-qa**

**config.spec.ts:**
- [ ] Remove `'Server'` from `CONFIG_GROUPS` array constant (line 5-14)
- [ ] Update group count assertions from 8 to 7
- [ ] Rewrite `unsaved changes indicator` test to target a different group (e.g. "Timeouts" or "HTTP Client") instead of "Server"
- [ ] Add new tests for Domain Allowlist in Authentication section:
  - Domain manager card visible within Authentication group
  - Add a domain (input + button interaction)
  - Remove a domain

**admin.spec.ts:**
- [ ] Remove `DOMAIN ALLOWLIST` header assertion from section rendering test
- [ ] Remove domain manager card visibility test
- [ ] Remove serial domain add/remove tests (lines 54-86) — moved to config.spec.ts
- [ ] Update section count/assertions as needed

- [ ] Run `cd e2e-tests && npm test` to verify

## Wave 3: Documentation Updates (depends on Wave 1)

**Assigned to: document-writer**

- [ ] `gh-pages/docs/web-ui.md`: Move `DomainAllow` from AdminAPI to Config subgraph in Mermaid diagram; update prose
- [ ] `gh-pages/docs/configuration.md`: Note domain allowlist is now under Config → Authentication
- [ ] `gh-pages/docs/getting-started.md`: Update reference from "Admin page" to "Config → Authentication"
- [ ] `gh-pages/docs/troubleshooting.md`: Update "admin panel" reference to "Config → Authentication"

## Interface Contracts

No new API contracts. Existing endpoints unchanged:
- `GET /_ui/api/config` — no longer returns `server_host`/`server_port`
- `GET /_ui/api/config/schema` — no longer includes `server_host`/`server_port` fields
- `GET/POST /_ui/api/domains` — unchanged
- `DELETE /_ui/api/domains/:domain` — unchanged

## Verification

| Service | Command | Must Pass |
|---------|---------|-----------|
| Backend | `cd backend && cargo clippy --all-targets && cargo test --lib` | Zero warnings, zero failures |
| Frontend | `cd frontend && npm run build && npm run lint` | Zero errors |
| E2E | `cd e2e-tests && npm test` | Zero failures |

## Branch

`refactor/config-page-cleanup`

## Review Status

- Codex review: **passed (after adjustment)**
- Findings addressed: 5/5
  1. (High) Legacy DB rows for server_host/server_port → remove from `load_into_config()` + update tests
  2. (High) DomainManager form-submit regression → add `e.preventDefault()` + render outside `<form>`
  3. (Medium) Missing test for GET /config contract change → add assertion test
  4. (Low) Line number references wrong → corrected in plan
  5. (Low) Admin setup banner stale copy → update banner text
- Disputed findings: 0
