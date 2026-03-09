# rename-openai-provider_20260309: Implementation Plan

**Status**: planned
**Branch**: refactor/rename-openai-provider

## Parallel Execution Strategy

```
Stream A (rust-backend-engineer)     Stream B (react-frontend-engineer)
─────────────────────────────────    ──────────────────────────────────
Phase 1: Types & provider module     Phase 2: Frontend display name
Phase 3: OAuth, priority, DB         (independent — no backend dependency)
Phase 4: Verification (cargo)
                                     ↓
         ←── merge point ──→

Phase 5: Integration verification (leader)
```

## Phase 1: Backend — Types & Provider Module
Agent: rust-backend-engineer
Depends on: nothing
Blocks: Phase 3

- [ ] 1.1 — Rename `ProviderId::OpenAI` → `ProviderId::OpenAICodex` in `backend/src/providers/types.rs`. Update serde rename to `"openai_codex"`, `as_str()`, `FromStr`, and all tests.
- [ ] 1.2 — Rename file `backend/src/providers/openai.rs` → `backend/src/providers/openai_codex.rs`. Rename struct `OpenAIProvider` → `OpenAICodexProvider`. Update `backend/src/providers/mod.rs` module declaration.
- [ ] 1.3 — Update `backend/src/routes/mod.rs`: import path, AppState field `openai_provider` → `openai_codex_provider`, all `ProviderId::OpenAI` match arms → `ProviderId::OpenAICodex`, all `state.openai_provider` → `state.openai_codex_provider`.

## Phase 2: Frontend (PARALLEL with Phase 1)
Agent: react-frontend-engineer
Depends on: nothing (only needs to know new ID is `openai_codex`)
Blocks: Phase 5

- [ ] 2.1 — Update `frontend/src/pages/Profile.tsx`: change `PROVIDERS` constant from `'openai'` to `'openai_codex'`. Add a display name map so `openai_codex` renders as `"OpenAI Codex"` in TreeNode label and ProviderCard.

## Phase 3: Backend — OAuth, Priority & DB Migration
Agent: rust-backend-engineer
Depends on: Phase 1
Blocks: Phase 4

- [ ] 3.1 — Update `backend/src/web_ui/provider_oauth.rs`: rename `openai_config()` → `openai_codex_config()`, all `"openai"` string matches → `"openai_codex"`, validation lists, provider iteration loops, and all tests.
- [ ] 3.2 — Update `backend/src/web_ui/provider_priority.rs`: `VALID_PROVIDERS` list `"openai"` → `"openai_codex"`, default priority test data, and all tests/comments.
- [ ] 3.3 — Add DB migration in `backend/src/web_ui/config_db.rs` (new version): UPDATE rows in `user_provider_keys`, `user_provider_tokens`, `user_provider_priority`, `model_routes` where `provider_id = 'openai'` → `'openai_codex'`. Drop and recreate CHECK constraints on all tables to include `'openai_codex'` instead of `'openai'`.
- [ ] 3.4 — Update all `"openai"` test strings in `config_db.rs` tests → `"openai_codex"`.

## Phase 4: Backend Verification
Agent: rust-backend-engineer
Depends on: Phase 3

- [ ] 4.1 — Run `cargo clippy` and `cargo test --lib` — all pass, no warnings from changed files.

## Phase 5: Integration Verification
Agent: leader
Depends on: Phase 2 + Phase 4

- [ ] 5.1 — Docker build and deploy: verify DB migration runs, UI shows "OpenAI Codex", other providers unaffected.
