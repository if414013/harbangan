# model-registry-refactor_20260309: Implementation Plan

**Status**: planned
**Branch**: refactor/model-registry-refactor

---

## Phase 1: Foundation (Sequential)
Agent: rust-backend-engineer

- [ ] 1.1 — Add `RegistryModel` struct and v11 migration creating `model_registry` table in `config_db.rs`
  - Table: id (UUID), provider_id, model_id, display_name, prefixed_id (UNIQUE), context_length, max_output_tokens, capabilities (JSONB), enabled, source, upstream_meta (JSONB), created_at, updated_at
  - UNIQUE constraint on (provider_id, model_id)
  - Partial index on enabled=true
  - Files: `backend/src/web_ui/config_db.rs`

- [ ] 1.2 — Add CRUD methods to `ConfigDb` for model registry
  - `get_all_registry_models()`, `get_enabled_registry_models()`, `upsert_registry_model()`, `bulk_upsert_registry_models()`, `update_model_enabled()`, `delete_registry_model()`, `clear_registry_by_provider()`
  - Files: `backend/src/web_ui/config_db.rs`

- [ ] 1.3 — Create `model_registry.rs` module with static model definitions
  - Static lists for Anthropic, OpenAI Codex, Gemini, Qwen (curated with context_length, max_output_tokens, capabilities)
  - Reference: `/Users/hikennoace/ai-gateway/CLIProxyAPI/internal/registry/model_definitions_static_data.go`
  - Files: `backend/src/web_ui/model_registry.rs` (new), `backend/src/web_ui/mod.rs`

- [ ] 1.4 — Add dynamic model fetch logic per provider
  - Kiro: extract `load_models_from_kiro()` from `main.rs`, store internal IDs in `upstream_meta`
  - Copilot: `GET {base_url}/models` using any user's copilot token from DB
  - Anthropic: `GET https://api.anthropic.com/v1/models` (if credentials available)
  - OpenAI: `GET https://api.openai.com/v1/models` (if credentials available)
  - Gemini: `GET https://generativelanguage.googleapis.com/v1beta/models` (if credentials available)
  - Qwen: static only (no known public model listing API)
  - `populate_provider()` orchestrator: tries API first, falls back to static
  - `generate_prefixed_id(provider_id, model_id) -> String` helper
  - Files: `backend/src/web_ui/model_registry.rs`

- [ ] 1.5 — Define admin API contract (types + route stubs)
  - Define request/response types for all admin endpoints so frontend can build against them:
    - `GET /_ui/api/admin/models` → `Vec<RegistryModel>` (optional `?provider=` filter)
    - `PATCH /_ui/api/admin/models/:id` ← `{ "enabled": bool }` → `RegistryModel`
    - `DELETE /_ui/api/admin/models/:id` → 204
    - `POST /_ui/api/admin/models/populate` ← `{ "providers": ["kiro","copilot"] }` → `{ "results": { "kiro": 15 }, "errors": { "qwen": "no credentials" } }`
  - Register route stubs (can return 501 initially) so frontend integration tests can target them
  - Files: `backend/src/web_ui/mod.rs`, `backend/src/web_ui/model_registry_handlers.rs` (new)

- [ ] 1.6 — Unit tests for Phase 1
  - Test CRUD methods, static model list generation, prefixed_id generation
  - Files: `backend/src/web_ui/config_db.rs`, `backend/src/web_ui/model_registry.rs`

---

> **After Phase 1, Phases 2 and 3 run in PARALLEL.**
> - Phase 2 (backend): rust-backend-engineer
> - Phase 3 (frontend): react-frontend-engineer

---

## Phase 2: Backend — Routing + Handlers (Parallel with Phase 3)
Agent: rust-backend-engineer

- [ ] 2.1 — Add `parse_prefixed_model()` to provider registry
  - Split on first `_`, validate prefix is known `ProviderId`, return `(ProviderId, String)` or 400 error
  - Remove `provider_for_model()` (clean break, no fallback)
  - Update `resolve_provider()` to accept already-parsed `(ProviderId, &str)`
  - Files: `backend/src/providers/registry.rs`

- [ ] 2.2 — Refactor `ModelCache` to be DB-backed
  - Add `load_from_db(db: &ConfigDb)` — queries enabled models, populates DashMap keyed by `prefixed_id`
  - Add `get_by_prefixed_id()` → returns `RegistryModel` (provider + upstream model_id)
  - Add `invalidate_and_reload(db: &ConfigDb)` — clear + reload
  - Keep DashMap for fast concurrent reads
  - Files: `backend/src/cache.rs`

- [ ] 2.3 — Scope `ModelResolver` to Kiro pipeline only
  - Remove "default to auto" for unrecognized models
  - Read Kiro internal IDs from `upstream_meta` in registry instead of hardcoded `hidden_models` HashMap
  - Files: `backend/src/resolver.rs`

- [ ] 2.4 — Update route handlers for prefix-based routing
  - `chat_completions_handler`: parse `request.model` via `parse_prefixed_model()`, replace model with upstream name before passing to provider
  - `anthropic_messages_handler`: same treatment
  - If `provider_id == Kiro`: use resolver for normalization, then Kiro pipeline
  - Else: route to direct provider with stripped model name
  - Files: `backend/src/routes/mod.rs`

- [ ] 2.5 — Update `get_models_handler` to return from registry
  - Return enabled models from DB-backed cache with prefixed IDs
  - Include richer metadata: `owned_by` = provider_id, context_length in response
  - Files: `backend/src/routes/mod.rs`

- [ ] 2.6 — Update startup flow in `main.rs`
  - Remove `load_models_from_kiro()` call at startup
  - Remove `add_hidden_models()` function entirely
  - At startup: call `model_cache.load_from_db(config_db)` to populate from registry
  - If registry empty: log info directing admin to populate via UI
  - Files: `backend/src/main.rs`

- [ ] 2.7 — Implement admin endpoint handlers (replace stubs from 1.5)
  - `admin_list_models`, `admin_update_model`, `admin_delete_model`, `admin_populate_models`
  - Populate calls `model_cache.invalidate_and_reload()` after upsert
  - All admin-only + CSRF protected
  - Files: `backend/src/web_ui/model_registry_handlers.rs`

- [ ] 2.8 — Unit tests for Phase 2
  - Test `parse_prefixed_model()`: valid prefixes, invalid prefixes, edge cases
  - Test `load_from_db()`, `get_by_prefixed_id()`
  - Update `test_get_models_handler`, `create_test_state`
  - Test admin endpoint handlers
  - Files: `backend/src/providers/registry.rs`, `backend/src/cache.rs`, `backend/src/routes/mod.rs`, `backend/src/web_ui/model_registry_handlers.rs`

## Phase 3: Frontend — Models Admin Page (Parallel with Phase 2)
Agent: react-frontend-engineer

> Builds against the API contract defined in task 1.5. Can use route stubs or mock data until Phase 2 endpoints are live.

- [ ] 3.1 — Add model registry API functions
  - `getRegistryModels(provider?: string)`, `updateModelEnabled(id, enabled)`, `deleteRegistryModel(id)`, `populateModels(providers?: string[])`
  - TypeScript interfaces matching the API contract from task 1.5
  - Files: `frontend/src/lib/api.ts`

- [ ] 3.2 — Create Models admin page
  - Per-provider collapsible sections with model count
  - Table per provider: enabled toggle | prefixed_id | display_name | context_length | source
  - "Populate" button per provider + "Populate All" at top
  - "Enable All" / "Disable All" per provider section
  - CRT terminal aesthetic matching existing admin pages
  - Files: `frontend/src/pages/Models.tsx` (new)

- [ ] 3.3 — Add routing and navigation
  - Add route in `App.tsx`: `<Route path="models" element={<Models />} />` (admin-guarded)
  - Add "Models" link in admin navigation
  - Files: `frontend/src/App.tsx`, navigation component

- [ ] 3.4 — Add component styles
  - Model table styles following existing CRT patterns from Guardrails/MCP pages
  - Files: `frontend/src/styles/components.css`

---

## Phase 4: Integration QA (After Phases 2 + 3)
Agents: backend-qa, frontend-qa

- [ ] 4.1 — Backend test coverage review
  - Ensure all CRUD methods, prefix parsing, cache loading, admin endpoints have tests
  - Files: `backend/src/web_ui/config_db.rs`, `backend/src/providers/registry.rs`, `backend/src/cache.rs`

- [ ] 4.2 — Frontend E2E tests for model management
  - Test populate flow, enable/disable toggle, model list display
  - Files: `e2e-tests/specs/ui/`

- [ ] 4.3 — Integration smoke test
  - Full flow: admin populates → models appear in registry → `GET /v1/models` returns prefixed IDs → `POST /v1/chat/completions` with prefixed model routes correctly → admin disables model → model disappears
  - Files: `e2e-tests/specs/api/`
