# Plan: Admin-Configurable Provider Enable/Disable

Admin can enable/disable providers (Anthropic, OpenAI Codex, Copilot). Kiro is always enabled. Disabled providers hide their OAuth/credential UI and models from normal users. Models servable by other enabled providers remain available (credential-gate, not brand-gate).

## Design Decisions

1. **Kiro always enabled** — enforced in Rust handler code, not DB constraint
2. **Provider = credential gate** — disabling Anthropic blocks Anthropic OAuth path; Copilot can still serve claude-* models if enabled
3. **Toggle UI on Providers page StatusTab** — admin sees toggle on each ProviderHealthCard (except Kiro)
4. **Dedicated `provider_settings` table** (v25) — 4 rows including Kiro, "no row = disabled" fail-closed semantics
5. **Pipeline enforcement via credential loading** — `load_user_data()` skips disabled providers; explicit prefix requests get a dedicated check

## Consultation Summary

- **rust-backend-engineer**: Credential-gate simplifies pipeline — `load_user_data()` skips disabled providers, `pick_best_provider()` naturally falls through. Only explicit-prefix requests need `validate_provider_enabled()`. ~11 backend files affected. Medium-large complexity.
- **react-frontend-engineer**: Add `enabled` field to `ProviderRegistryEntry`, toggle button on `ProviderHealthCard` (admin only), filter disabled providers from StatusTab/ConnectionsTab for non-admins. ModelsTab filtering handled by backend. Small-medium complexity.
- **database-engineer**: New `provider_settings` table (v25), 4 rows seeded all-enabled. 3 query functions. Small complexity.
- **devops-engineer**: Zero infrastructure impact. Full-stack mode uses DB-stored config. Proxy mode already has `*_ENABLED` env vars. No Docker/compose/entrypoint changes.
- **backend-qa**: ~30 unit tests + ~6 integration tests. Existing registry.rs and pipeline.rs patterns provide templates. Medium complexity.
- **frontend-qa**: 2 new spec files (~15-20 tests), 3-5 existing spec updates. Cross-role testing uses multi-account pattern. Medium complexity.
- **document-writer**: 7 gh-pages files + CLAUDE.md need updates. No new doc files. Small-medium complexity.

## File Manifest

| File | Action | Owner | Wave |
|------|--------|-------|------|
| `backend/src/web_ui/config_db.rs` | modify (DDL + queries) | database-engineer | 1 |
| `backend/src/providers/types.rs` | modify (add enabled helpers) | rust-backend-engineer | 1 |
| `backend/src/providers/registry.rs` | modify (filter disabled in load_user_data, explicit-prefix check) | rust-backend-engineer | 1 |
| `backend/src/routes/pipeline.rs` | modify (add validate_provider_enabled for explicit prefix) | rust-backend-engineer | 1 |
| `backend/src/routes/openai.rs` | modify (filter /v1/models by provider enabled) | rust-backend-engineer | 1 |
| `backend/src/cache.rs` | modify (provider-aware registry model filtering) | rust-backend-engineer | 1 |
| `backend/src/web_ui/provider_oauth.rs` | modify (filter registry/status, block connect for disabled) | rust-backend-engineer | 2 |
| `backend/src/web_ui/routes.rs` | modify (add admin toggle endpoint) | rust-backend-engineer | 2 |
| `backend/src/routes/state.rs` | modify (add disabled_providers to AppState) | rust-backend-engineer | 1 |
| `backend/src/error.rs` | modify (add ProviderDisabled variant) | rust-backend-engineer | 1 |
| `frontend/src/lib/api.ts` | modify (add enabled field, toggle API fn) | react-frontend-engineer | 2 |
| `frontend/src/pages/providers/StatusTab.tsx` | modify (add toggle for admin, filter for non-admin) | react-frontend-engineer | 2 |
| `frontend/src/pages/providers/ConnectionsTab.tsx` | modify (filter disabled for non-admin) | react-frontend-engineer | 2 |
| `frontend/src/components/ProviderHealthCard.tsx` | modify (add toggle button, disabled badge) | react-frontend-engineer | 2 |
| `e2e-tests/specs/api/provider-toggle.spec.ts` | create | frontend-qa | 3 |
| `e2e-tests/specs/ui/provider-toggle.spec.ts` | create | frontend-qa | 3 |
| `e2e-tests/specs/api/provider-status.spec.ts` | modify (add enabled field assertions) | frontend-qa | 3 |

## Wave 1: Backend Foundations

- [ ] **v25 migration: provider_settings table** (assigned: database-engineer)
  - Files: `backend/src/web_ui/config_db.rs`
  - Create table with `provider_id TEXT PK`, `enabled BOOL DEFAULT true`, `updated_at TIMESTAMPTZ`
  - Seed 4 rows (kiro, anthropic, openai_codex, copilot) all enabled
  - Add query functions: `get_all_provider_settings()`, `is_provider_enabled()`, `set_provider_enabled()`
  - Depends on: none

- [ ] **Add ProviderDisabled error variant** (assigned: rust-backend-engineer)
  - Files: `backend/src/error.rs`
  - New `ApiError::ProviderDisabled` returning HTTP 403 with message
  - Depends on: none

- [ ] **Add disabled_providers state to AppState** (assigned: rust-backend-engineer)
  - Files: `backend/src/routes/state.rs`, `backend/src/providers/registry.rs`
  - Add `disabled_providers: Arc<RwLock<HashSet<ProviderId>>>` to `ProviderRegistry`
  - Load from DB at startup via `get_all_provider_settings()`
  - Add `is_provider_enabled(&self, provider: &ProviderId) -> bool` (Kiro short-circuits to true)
  - Add `set_provider_enabled(&self, provider: ProviderId, enabled: bool)` to update cache + DB
  - Depends on: v25 migration

- [ ] **Filter disabled providers in credential loading** (assigned: rust-backend-engineer)
  - Files: `backend/src/providers/registry.rs`
  - `load_user_data()` skips token loading for disabled providers
  - `pick_best_provider()` naturally falls through to next available provider
  - Depends on: disabled_providers state

- [ ] **Add explicit-prefix provider validation** (assigned: rust-backend-engineer)
  - Files: `backend/src/routes/pipeline.rs`
  - After `parse_prefixed_model()` succeeds, check `is_provider_enabled()` for the explicit provider
  - Return `ApiError::ProviderDisabled` if disabled
  - Depends on: disabled_providers state

- [ ] **Filter /v1/models by provider enabled state** (assigned: rust-backend-engineer)
  - Files: `backend/src/routes/openai.rs`, `backend/src/cache.rs`
  - `get_enabled_registry_models()` additionally filters out models from disabled providers
  - Per-provider model entries (e.g., `copilot/claude-sonnet-4`) remain if that provider is enabled
  - Depends on: disabled_providers state

## Wave 2: API Endpoints + Frontend

- [ ] **Admin toggle endpoint** (assigned: rust-backend-engineer)
  - Files: `backend/src/web_ui/routes.rs`, `backend/src/web_ui/provider_oauth.rs`
  - `PATCH /_ui/api/admin/providers/{provider_id}` with body `{ "enabled": bool }`
  - Admin-only + CSRF protected
  - Reject `provider_id = "kiro"` with 400
  - Update DB + in-memory cache
  - Depends on: Wave 1 backend

- [ ] **Filter provider registry/status for non-admins** (assigned: rust-backend-engineer)
  - Files: `backend/src/web_ui/provider_oauth.rs`
  - `GET /providers/registry` adds `enabled` field; non-admin response excludes disabled providers
  - `GET /providers/status` excludes disabled providers for non-admins
  - Block `POST /providers/:provider/connect` for disabled providers
  - Depends on: Wave 1 backend

- [ ] **Frontend: API types + toggle function** (assigned: react-frontend-engineer)
  - Files: `frontend/src/lib/api.ts`
  - Add `enabled: boolean` to `ProviderRegistryEntry` type
  - Add `toggleProviderEnabled(providerId: string, enabled: boolean)` API function
  - Depends on: admin toggle endpoint

- [ ] **Frontend: StatusTab toggle UI** (assigned: react-frontend-engineer)
  - Files: `frontend/src/pages/providers/StatusTab.tsx`, `frontend/src/components/ProviderHealthCard.tsx`
  - Admin: show enable/disable toggle on each ProviderHealthCard (except Kiro)
  - Non-admin: filter out disabled providers from the grid
  - Use existing role-badge button pattern (on/off with green/red)
  - Depends on: API types

- [ ] **Frontend: ConnectionsTab filtering** (assigned: react-frontend-engineer)
  - Files: `frontend/src/pages/providers/ConnectionsTab.tsx`
  - Non-admin: hide disabled providers from OAuth connection cards
  - Admin: show all, disabled ones marked with badge
  - Depends on: API types

## Wave 3: Testing + Verification

- [ ] **Backend unit tests** (assigned: backend-qa)
  - ~30 tests across registry.rs, pipeline.rs, types.rs, cache.rs
  - Key scenarios: credential-gate fallthrough, explicit-prefix rejection, Kiro non-disableable, model filtering
  - Depends on: Wave 1 + Wave 2 backend

- [ ] **Backend integration tests** (assigned: backend-qa)
  - ~6 tests in tests/integration_test.rs
  - Admin toggle flow, non-admin 403, model list filtering, re-enable restores access
  - Depends on: Wave 2 backend

- [ ] **E2E API tests** (assigned: frontend-qa)
  - Files: `e2e-tests/specs/api/provider-toggle.spec.ts`
  - Admin toggle lifecycle, non-admin rejection, model list filtering
  - Depends on: Wave 2

- [ ] **E2E UI tests** (assigned: frontend-qa)
  - Files: `e2e-tests/specs/ui/provider-toggle.spec.ts`
  - Admin toggle controls visible, disabled provider hidden for non-admin, re-enable flow
  - Depends on: Wave 2

- [ ] **Update existing E2E assertions** (assigned: frontend-qa)
  - Files: `e2e-tests/specs/api/provider-status.spec.ts`
  - Add `enabled` field to shape assertions
  - Depends on: Wave 2

## Wave 4: Documentation

- [ ] **Update gh-pages docs** (assigned: document-writer)
  - 7 files: web-ui.md, configuration.md, api-reference.md, client-setup.md, architecture/index.md, modules.md, getting-started.md
  - New "Provider Management" section in web-ui.md
  - New admin endpoint in api-reference.md
  - Provider availability notes in client-setup.md
  - Depends on: Wave 2

- [ ] **Update CLAUDE.md** (assigned: document-writer)
  - Add provider toggle to API Endpoints section
  - Note provider filtering in Backend Request Flow
  - Depends on: Wave 2

## Interface Contracts

### Admin Toggle Endpoint
```
PATCH /_ui/api/admin/providers/{provider_id}
Headers: X-CSRF-Token
Body: { "enabled": boolean }
Response 200: { "provider_id": "anthropic", "enabled": false, "updated_at": "..." }
Response 400: { "error": "Kiro cannot be disabled" }
Response 403: admin only
```

### Provider Registry Response (updated)
```
GET /_ui/api/providers/registry
Response: [
  { "id": "kiro", "display_name": "Kiro", "category": "native", "supports_pool": false, "enabled": true },
  { "id": "anthropic", "display_name": "Anthropic", "category": "direct", "supports_pool": true, "enabled": true },
  ...
]
// Non-admin: disabled providers excluded from array
// Admin: all providers included with enabled field
```

### DB Schema (v25)
```sql
CREATE TABLE IF NOT EXISTS provider_settings (
    provider_id  TEXT PRIMARY KEY,
    enabled      BOOLEAN NOT NULL DEFAULT true,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
-- Seeded: kiro, anthropic, openai_codex, copilot (all enabled)
```

## Verification

| Service | Command | Gate |
|---------|---------|------|
| Backend | `cd backend && cargo clippy --all-targets` | Zero warnings |
| Backend | `cd backend && cargo fmt --check` | No diffs |
| Backend | `cd backend && cargo test --lib` | Zero failures |
| Frontend | `cd frontend && npm run build` | Zero errors |
| Frontend | `cd frontend && npm run lint` | Zero errors |
| E2E | `cd e2e-tests && npm test` | Zero failures |

## Branch

`feat/provider-enable-disable`

## Review Status
- Codex review: timed out (6+ min), replaced with manual spot-check + 7-agent cross-validation
- All file paths verified against codebase (17 files confirmed)
- All function names verified (validate_model_provider, providers_registry, load_user_data, etc.)
- provider_priority.rs confirmed no-impact (disabled providers have no loaded creds, priority is moot)
- Credential-gate semantics validated across all agents after initial misunderstanding corrected
- Findings addressed: 1 (credential-gate vs model-gate confusion between backend-eng and backend-qa)
- Disputed findings: 0
