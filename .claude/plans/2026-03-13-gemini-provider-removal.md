# Plan: Full Gemini Provider Removal (Team-Parallel)

## Agent Team: Fullstack

| Agent | Role | Files Owned |
|-------|------|-------------|
| `rust-backend-engineer` | All backend Rust changes | `backend/src/**` (5 deletions + 14 edits) |
| `react-frontend-engineer` | Frontend UI changes | `frontend/src/pages/Profile.tsx` |
| `frontend-qa` | E2E test cleanup | `e2e-tests/specs/ui/*.spec.ts` (3 files) |
| `backend-qa` | Backend verification | runs `cargo clippy` + `cargo test --lib` |
| `document-writer` | Changelog / PR description | — |

## Wave 1 — All agents in parallel

### Track A: Backend (`rust-backend-engineer`)

**Step 1 — Delete 5 Gemini-only files:**
- `backend/src/providers/gemini.rs`
- `backend/src/converters/openai_to_gemini.rs`
- `backend/src/converters/anthropic_to_gemini.rs`
- `backend/src/converters/gemini_to_openai.rs`
- `backend/src/converters/gemini_to_anthropic.rs`

**Step 2 — Edit core provider files:**
- `backend/src/providers/types.rs` — Remove `Gemini` variant from `ProviderId` enum, `as_str()`, `Display`, `FromStr`, all test refs
- `backend/src/providers/mod.rs` — Remove `pub mod gemini;` (L9) and `map.insert(ProviderId::Gemini, ...)` (L39-42)
- `backend/src/converters/mod.rs` — Remove 4 module declarations: `anthropic_to_gemini`, `gemini_to_anthropic`, `gemini_to_openai`, `openai_to_gemini`
- `backend/src/providers/registry.rs` — Remove `gemini-` prefix routing (L74-75), `"gemini"` from provider lists (L322, L332)

**Step 3 — Edit web UI + DB migration:**
- `backend/src/web_ui/provider_oauth.rs` — Remove `gemini_config()` (L82-108), match arms (L126, L138, L140), `extract_email()` case (L425-438), `providers_status()` loop (L522), auth URL params (L709-711), update tests (L944, L1027-1041, L1158, L1182)
- `backend/src/web_ui/provider_priority.rs` — Remove `"gemini"` from `VALID_PROVIDERS` (L71), update test count 6->5 (L204, L221-222)
- `backend/src/web_ui/model_registry.rs` — Remove `gemini_static_models()` (L219-275), `fetch_gemini_models()` (L536-607), call in `all_static_models()` (L320), match arms (L716, L748), update test (L884)
- `backend/src/web_ui/model_registry_handlers.rs` — Remove `"gemini"` from providers list (L182)
- `backend/src/web_ui/config_db.rs` — Add v13 migration: DELETE Gemini rows, drop+re-add CHECK constraints without `'gemini'`. Update test data `"gemini"` -> `"anthropic"`
- `backend/src/main.rs` — Remove `"gemini"` from providers array (L209)

**Step 4 — Comment-only cleanups:**
- `backend/src/error.rs` (L112), `backend/src/resolver.rs` (L90, L113), `backend/src/providers/traits.rs` (L56), `backend/src/streaming/sse.rs` (L3)

**Step 5 — Self-verify:** `cd backend && cargo clippy --all-targets && cargo test --lib`

### Track B: Frontend (`react-frontend-engineer`) — parallel with Track A
- `frontend/src/pages/Profile.tsx` — Remove `'gemini'` from `PROVIDERS` constant (L11)
- **Self-verify:** `cd frontend && npm run build && npm run lint`

### Track C: E2E Tests (`frontend-qa`) — parallel with Tracks A & B
- `e2e-tests/specs/ui/provider-oauth.spec.ts` — Remove all Gemini mock data and test cases, update provider card count 3->2
- `e2e-tests/specs/ui/qwen-setup.spec.ts` — Remove `gemini: { connected: false }` from mock (L101)
- `e2e-tests/specs/ui/copilot-setup.spec.ts` — Remove `gemini: { connected: false }` from mock (L105)

### Track D: Documentation (`document-writer`) — parallel with all
- Draft PR description summarizing the removal scope (5 deleted files, ~1300 lines removed, 14 edited files, v13 DB migration)

## Wave 2 — Cross-track verification (`backend-qa`)
- Run after all Wave 1 tracks complete
- `cd backend && cargo clippy --all-targets && cargo test --lib && cd ../frontend && npm run build && npm run lint`

## DB Migration Notes (v13)
1. `DELETE FROM` all tables where `provider_id = 'gemini'`
2. Drop + re-add CHECK constraints on affected tables without `'gemini'`
3. Record schema version 13
