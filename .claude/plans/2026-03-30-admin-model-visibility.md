# Plan: Admin-Configurable Model Visibility

## Context

The model registry currently shows all models per provider (10 Anthropic, 12 OpenAI Codex, etc.), including older versions. The admin wants to curate which models are visible per provider, showing only the latest/desired ones, and have this configurable via the web UI. Disabled models should be **fully blocked** — not just hidden from listings but also rejected when requested via `/v1/chat/completions` or `/v1/messages`.

### User's Desired Model Lists

**Anthropic** (3 of 10): `claude-haiku-4-5-20251001`, `claude-opus-4-6`, `claude-sonnet-4-6`

**OpenAI Codex** (4 of 12): `gpt-5.4`, `gpt-5.3-codex-spark`, `gpt-5.3-codex`, `gpt-5.1-codex-mini`

**Kiro** (8): Auto, Claude Haiku 4.5, Claude Opus 4.6, Claude Sonnet 4.6, Deepseek v3.2, MiniMax M2.1, MiniMax M2.5, Qwen3 Coder Next *(model_ids confirmed from DB at implementation time — use Kiro API `modelId`, not `display_name`)*

**Copilot**: Remove old GPT/Claude versions, keep latest of each + all non-GPT/Claude models *(model_ids confirmed from DB at implementation time)*

### Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Hidden model behavior | Fully blocked (403) | Admin wants disabled = not usable |
| New model default | Visible (enabled=true) | User preference; requires changing model builders from current `enabled: false` |
| Admin config approach | Per-provider allowlist | Stored in DB, managed via UI, "Apply Defaults" action |
| Kiro ID strategy | Registry stores Kiro `modelId` (API identifier), not `display_name` | Ensures `/v1/models` and blocking checks use the same IDs clients send in requests |

## Consultation Summary

- **database-engineer**: `enabled` column already exists on `model_registry`. Upsert preserves `enabled` on conflict. No migration needed for the toggle itself, but a new `model_visibility_defaults` table is needed for the stored allowlist.
- **rust-backend-engineer**: Two model systems exist (legacy Kiro cache + registry). Cache loads only enabled models — must change to load all for blocking. Request blocking goes after `resolve_provider_routing()` in both openai.rs and anthropic.rs. **Critical: Kiro registry uses `display_name` as `model_id` but legacy cache and clients use `modelId` — must unify.**
- **react-frontend-engineer**: Per-model toggle + bulk enable/disable already exist in UI. Providers page is NOT admin-guarded. Allowlist editor fits in ProviderModelGroup as a new section.
- **devops-engineer**: No infrastructure impact. Purely DB + API + UI.
- **backend-qa**: Zero test coverage for model registry DB operations. Need unit tests for blocking logic, cache changes, and DB CRUD. Should include non-admin access control tests.
- **frontend-qa**: E2E tests needed for admin allowlist management and model blocking verification. `specs/ui/models.spec.ts` is orphaned from playwright.config.ts.
- **document-writer**: 6 doc files need updates (api-reference, web-ui, configuration, client-setup, getting-started, CLAUDE.md).

## Architecture

### How It Works

1. Admin configures a **per-provider allowlist** (list of model_ids to keep visible)
2. Admin clicks **"Apply Defaults"** → models on the list get `enabled=true`, others get `enabled=false`
3. When models are populated from API, new models appear as `enabled=true` (visible by default)
4. Admin can still manually toggle individual models via existing per-model switch
5. API requests to disabled models return **403 "model disabled by administrator"**
6. Unknown models (not in registry) pass through — only explicitly disabled models are blocked

### Kiro ID Unification (Codex Finding #1)

**Problem**: The legacy Kiro cache keys by upstream `modelId` (e.g., `anthropic.claude-sonnet-4-6`), but the registry stores Kiro `display_name` as `model_id` (e.g., `Claude Sonnet 4.6`). Clients send the `modelId` format. This mismatch means blocking checks on `kiro/<display_name>` won't match what clients request.

**Fix**: Change `model_registry.rs` Kiro model parsing to use the upstream `modelId` as `model_id` (not `display_name`). Keep `display_name` for UI rendering. Then `/v1/models` can serve Kiro models from the registry (which uses the same IDs the legacy cache used), and blocking checks will match client requests.

**Additionally**: Remove legacy Kiro models from `GET /v1/models` output — serve ALL providers from the unified registry cache only. The legacy Kiro cache remains for model resolution in the request pipeline but no longer feeds the model listing endpoint.

### New DB Table (v24)

```sql
CREATE TABLE IF NOT EXISTS model_visibility_defaults (
    id          UUID PRIMARY KEY,
    provider_id TEXT NOT NULL,
    model_id    TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (provider_id, model_id)
);
```

### Cache Change

`load_from_registry()` loads ALL models (enabled + disabled) instead of only enabled. New methods:
- `get_enabled_registry_models()` → filters to `enabled=true` (for `/v1/models`)
- `is_model_disabled(prefixed_id)` → true if model exists AND `enabled=false` (for request blocking)

### Request Blocking

New `validate_model_visibility()` in `pipeline.rs`, called after `resolve_provider_routing()`:

```rust
fn validate_model_visibility(cache, provider_id, model, stripped_model) -> Result<(), ApiError> {
    let prefixed_id = format!("{}/{}", provider_id, stripped_model.unwrap_or(model));
    if cache.is_model_disabled(&prefixed_id) {
        return Err(ApiError::ModelDisabled { model });
    }
    Ok(())
}
```

### Admin-Only Model Management (Codex Finding #2)

**Problem**: Existing model registry mutation routes (PATCH, DELETE, POST populate) are mounted under session-authenticated user routes, not admin routes. Any logged-in user can toggle models.

**Fix**: Move `model_registry_routes()` from the session-authenticated router to the admin router in `web_ui/mod.rs`. The GET (list) stays on the session router for read access; PATCH/DELETE/POST move to admin. This also covers the new visibility defaults endpoints.

### API Endpoints (admin-only, CSRF-protected)

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/_ui/api/models/visibility-defaults` | List all allowlists grouped by provider |
| `PUT` | `/_ui/api/models/visibility-defaults/:provider_id` | Set allowlist for a provider |
| `DELETE` | `/_ui/api/models/visibility-defaults/:provider_id` | Remove stored defaults only (does NOT change model `enabled` state) |
| `POST` | `/_ui/api/models/visibility-defaults/:provider_id/apply` | Apply: enable listed, disable unlisted |
| `POST` | `/_ui/api/models/visibility-defaults/apply-all` | Apply all provider allowlists |

**Existing routes moved to admin** (Codex Finding #2):
| Method | Path | Change |
|--------|------|--------|
| `PATCH` | `/_ui/api/models/registry/:id` | Move to admin router |
| `DELETE` | `/_ui/api/models/registry/:id` | Move to admin router |
| `POST` | `/_ui/api/models/registry/populate` | Move to admin router |

### Frontend UI

New `ModelAllowlistEditor` component inside each `ProviderModelGroup`:
- Checkbox list of all models for that provider
- "Save Allowlist" stores the selection as defaults
- "Apply Defaults" resets models to match the allowlist
- Admin-only (non-admin users see models but can't toggle/edit)

## File Manifest

| File | Action | Owner | Wave |
|------|--------|-------|------|
| `backend/src/web_ui/config_db.rs` | modify — v24 migration, CRUD for visibility defaults, bulk enable/disable | database-engineer (DDL) + rust-backend-engineer (queries) | 1 |
| `backend/src/web_ui/model_registry.rs` | modify — Kiro model parsing to use `modelId` instead of `display_name`, change `enabled: false` → `enabled: true` for new models | rust-backend-engineer | 1 |
| `backend/src/web_ui/static_models.rs` | modify — change `enabled: false` → `enabled: true` in model helper | rust-backend-engineer | 1 |
| `backend/src/cache.rs` | modify — load all models, add `is_model_disabled()`, `get_enabled_registry_models()` | rust-backend-engineer | 1 |
| `backend/src/error.rs` | modify — add `ModelDisabled` variant | rust-backend-engineer | 1 |
| `backend/src/routes/pipeline.rs` | modify — add `validate_model_visibility()` | rust-backend-engineer | 1 |
| `backend/src/routes/openai.rs` | modify — call `validate_model_visibility()` after routing, serve models from registry only (remove legacy Kiro cache from listing) | rust-backend-engineer | 1 |
| `backend/src/routes/anthropic.rs` | modify — call `validate_model_visibility()` after routing | rust-backend-engineer | 1 |
| `backend/src/web_ui/model_visibility_handlers.rs` | create — handler functions for visibility defaults CRUD + apply | rust-backend-engineer | 1 |
| `backend/src/web_ui/mod.rs` | modify — register new admin routes, move PATCH/DELETE/populate to admin router | rust-backend-engineer | 1 |
| `frontend/src/lib/api.ts` | modify — add types + API functions for visibility defaults | react-frontend-engineer | 2 |
| `frontend/src/components/ModelAllowlistEditor.tsx` | create — allowlist editor with checkbox grid | react-frontend-engineer | 2 |
| `frontend/src/components/ProviderModelGroup.tsx` | modify — integrate allowlist editor, admin-gate toggles | react-frontend-engineer | 2 |
| `frontend/src/pages/Providers.tsx` | modify — state management for visibility defaults | react-frontend-engineer | 2 |
| `frontend/src/pages/providers/ModelsTab.tsx` | modify — pass visibility defaults props | react-frontend-engineer | 2 |

## Wave 1: Backend (DB + API + Blocking)

- [ ] **v24 migration**: Create `model_visibility_defaults` table with seed data for user's desired models (assigned: database-engineer → rust-backend-engineer)
  - Files: `config_db.rs`
  - Depends on: none

- [ ] **Kiro ID unification + enabled default**: Fix Kiro model parsing to use `modelId`, change all model builders to `enabled: true` (assigned: rust-backend-engineer)
  - Files: `model_registry.rs`, `static_models.rs`
  - Depends on: none

- [ ] **Cache changes**: Load all models into cache, add `is_model_disabled()` and `get_enabled_registry_models()` methods (assigned: rust-backend-engineer)
  - Files: `cache.rs`
  - Depends on: none

- [ ] **Request blocking**: Add `ApiError::ModelDisabled` and `validate_model_visibility()`, integrate into openai.rs + anthropic.rs. Remove legacy Kiro cache from `/v1/models` listing. (assigned: rust-backend-engineer)
  - Files: `error.rs`, `pipeline.rs`, `openai.rs`, `anthropic.rs`
  - Depends on: cache changes, Kiro ID unification

- [ ] **Admin-gate existing routes + visibility defaults API**: Move PATCH/DELETE/populate to admin router. Create handlers for allowlist CRUD + apply. Register new admin routes. (assigned: rust-backend-engineer)
  - Files: `model_visibility_handlers.rs` (new), `mod.rs`
  - Depends on: v24 migration

- [ ] **Backend unit tests**: Cache `is_model_disabled()`, `validate_model_visibility()`, DB CRUD, non-admin access rejection tests (assigned: rust-backend-engineer)
  - Depends on: all Wave 1 tasks above

## Wave 2: Frontend

- [ ] **API integration**: Add visibility defaults types and functions to api.ts (assigned: react-frontend-engineer)
  - Files: `api.ts`
  - Depends on: Wave 1

- [ ] **Allowlist editor component**: Checkbox grid per provider with save/apply actions (assigned: react-frontend-engineer)
  - Files: `ModelAllowlistEditor.tsx` (new), `ProviderModelGroup.tsx`, `Providers.tsx`, `ModelsTab.tsx`
  - Depends on: API integration

## Wave 3: E2E Testing

- [ ] **E2E tests**: Admin toggles allowlist, verifies API blocks disabled model, verifies /v1/models filters, verifies non-admin cannot mutate models (assigned: frontend-qa)
  - Depends on: Wave 2

## Wave 4: Documentation

- [ ] **Update docs**: API reference (new endpoints + blocking behavior), web-ui guide (allowlist management), CLAUDE.md (new endpoints) (assigned: document-writer)
  - Depends on: Wave 2

## Interface Contracts

### Visibility Defaults Response

```json
GET /_ui/api/models/visibility-defaults
{
  "defaults": {
    "anthropic": ["claude-haiku-4-5-20251001", "claude-opus-4-6", "claude-sonnet-4-6"],
    "openai_codex": ["gpt-5.4", "gpt-5.3-codex-spark", "gpt-5.3-codex", "gpt-5.1-codex-mini"]
  }
}
```

### Set Allowlist

```json
PUT /_ui/api/models/visibility-defaults/anthropic
{ "model_ids": ["claude-haiku-4-5-20251001", "claude-opus-4-6", "claude-sonnet-4-6"] }
→ { "success": true, "provider_id": "anthropic", "count": 3 }
```

### Apply Defaults

```json
POST /_ui/api/models/visibility-defaults/anthropic/apply
→ { "success": true, "provider_id": "anthropic", "enabled": 3, "disabled": 7 }
```

### Model Disabled Error (403)

```json
{
  "error": {
    "message": "Model 'claude-sonnet-4-5-20250929' is disabled by administrator",
    "type": "model_disabled"
  }
}
```

## Verification

1. **Backend**: `cd backend && cargo clippy --all-targets && cargo test --lib && cargo fmt --check`
2. **Frontend**: `cd frontend && npm run build && npm run lint`
3. **E2E**: `cd e2e-tests && npm test`
4. **Manual**: Toggle model visibility via admin UI, verify `/v1/models` filters, verify blocked model returns 403

## Branch

`feat/model-visibility`

## Review Status

- Codex review: **passed** (1 adjustment round)
- Findings addressed: 6/6
  - #1 HIGH (Kiro dual-identity) → Added Kiro ID unification strategy + file manifest entry
  - #2 HIGH (admin-only gap) → Move existing mutation routes to admin router
  - #3 HIGH (enabled=true mismatch) → Added `model_registry.rs` + `static_models.rs` to manifest, change `enabled: false` → `true`
  - #4 MEDIUM (ownership violation) → Reassigned backend tests from backend-qa to rust-backend-engineer
  - #5 MEDIUM (DELETE contract contradiction) → Clarified: DELETE removes stored defaults only, does NOT change enabled flags
  - #6 LOW (missing files + fmt check) → Added `ModelsTab.tsx` to manifest, added `cargo fmt --check` to verification
- Disputed findings: 0
