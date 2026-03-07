# multi-provider-support_20260307: Implementation Plan

**Status**: completed
**Branch**: feat/multi-provider-support_20260307

## Phase 1: Backend — Foundation, DB & Key Management
Agent: rust-backend-engineer

- [x] 1.1 — Provider types (`ProviderId`, `ProviderCredentials`, `ProviderContext`) and `Provider` trait in `backend/src/providers/{mod,types,traits}.rs`
- [x] 1.2 — Error types (`ProviderApiError`, `ProviderNotConfigured`) in `backend/src/error.rs` and config additions in `backend/src/config.rs`
- [x] 1.3 — DB migration v7: `user_provider_keys` and `model_routes` tables in `backend/src/web_ui/config_db.rs`
- [x] 1.4 — DB methods: `get_user_provider_key`, `upsert_user_provider_key`, `delete_user_provider_key`, `get_user_connected_providers`
- [x] 1.5 — Key format detection (`detect_provider()`) in `backend/src/providers/key_detection.rs`
- [x] 1.6 — Provider key routes (add/remove/status) in `backend/src/web_ui/provider_keys.rs` and route registration in `backend/src/web_ui/mod.rs`
- [x] 1.7 — Unit tests for key detection and DB methods

## Phase 2: Backend — Provider Implementations & Streaming
Agent: rust-backend-engineer

- [x] 2.1 — [TDD required] SSE streaming parser in `backend/src/streaming/sse.rs` — write tests first
- [x] 2.2 — KiroProvider: wrap existing pipeline in `backend/src/providers/kiro.rs`
- [x] 2.3 — AnthropicProvider: direct `api.anthropic.com` in `backend/src/providers/anthropic.rs`
- [x] 2.4 — OpenAIProvider: direct `api.openai.com` in `backend/src/providers/openai.rs`
- [x] 2.5 — GeminiProvider: direct `generativelanguage.googleapis.com` in `backend/src/providers/gemini.rs`
- [x] 2.6 — Unit tests for each provider implementation

## Phase 3: Backend — Converters & Cross-Format Translation
Agent: rust-backend-engineer

- [x] 3.1 — [TDD required] `openai_to_anthropic.rs` and `anthropic_to_openai.rs` — write tests first
- [x] 3.2 — [TDD required] `openai_to_gemini.rs` and `anthropic_to_gemini.rs` — write tests first
- [x] 3.3 — [TDD required] `gemini_to_openai.rs` and `gemini_to_anthropic.rs` — write tests first
- [x] 3.4 — Converter registration in `backend/src/converters/mod.rs`

## Phase 4: Backend — Registry, Routing & Integration
Agent: rust-backend-engineer

- [x] 4.1 — Provider registry (`resolve_provider`, `get_user_credentials`) in `backend/src/providers/registry.rs`
- [x] 4.2 — Provider key cache (`provider_key_cache`) in AppState (DashMap with 5-min TTL in ProviderRegistry)
- [x] 4.3 — Handler refactoring: wire providers into request flow in `backend/src/routes/mod.rs`
- [x] 4.4 — AppState additions and initialization in `backend/src/main.rs`
- [x] 4.5 — Unit tests for routing logic (registry tests in providers/registry.rs)
- [x] 4.6 — Verification: `cargo clippy` + `cargo test --lib` (530 tests pass)

## Phase 5: Frontend — Providers Page
Agent: react-frontend-engineer

- [x] 5.1 — API types and functions for provider endpoints in `frontend/src/lib/api.ts`
- [x] 5.2 — Providers page component in `frontend/src/pages/Providers.tsx`
- [x] 5.3 — Route registration in `App.tsx` and sidebar nav link in `Sidebar.tsx`
- [x] 5.4 — Styling and polish
- [x] 5.5 — Verification: `npm run lint` + `npm run build`

## Phase 6: QA
Agents: backend-qa, frontend-qa

- [x] 6.1 — Backend integration tests for provider routing end-to-end
- [x] 6.2 — Frontend E2E Playwright tests for Providers page
