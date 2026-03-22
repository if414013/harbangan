# Plan: Remove Qwen Coder Provider Completely

## Context

The Qwen Coder provider is being fully removed from Harbangan. This includes all backend code (provider implementation, device code auth flow, routes, config), frontend UI (QwenSetup component, API functions, provider page integration), database data cleanup, infrastructure config (docker-compose, env vars, entrypoint device flow), E2E tests, and documentation. After removal, Harbangan will support 5 providers: Kiro, Anthropic, OpenAI Codex, Copilot, and Custom.

## Consultation Summary

- **rust-backend-engineer**: 2 files to delete (`providers/qwen.rs` ~1,100 lines, `web_ui/qwen_auth.rs` ~930 lines), 12 files to modify. `ProviderId::Qwen` enum variant, `Config.qwen_oauth_client_id`, `ProxyConfig.qwen_token/qwen_base_url` all need removal. Registry model prefix matching (`qwen-`, `qwen3-`, `qwq-`) needs removal. No Cargo.toml dependency changes.
- **react-frontend-engineer**: 1 file to delete (`QwenSetup.tsx`), 5 files to modify. Qwen types, API functions, and provider page integration all self-contained. Registry-driven UI will naturally stop showing Qwen once backend removes it.
- **database-engineer**: New v22 migration needed to purge Qwen rows from 6 tables (`user_provider_tokens`, `model_routes`, `model_registry`, `user_provider_priority`, `admin_provider_pool`, `config`). No CHECK constraint changes needed (v21 already dropped them). Existing migrations untouched.
- **devops-engineer**: Remove env vars from `docker-compose.yml`, `docker-compose.gateway.yml`, `.env.example`. Remove ~95-line `run_qwen_device_flow()` from `entrypoint.sh`. Clean up `.claude/rules/secrets.md` and `.claude/hooks/scan-secrets-before-write.sh`.
- **backend-qa**: ~120 tests affected. 85 deleted with file removal, ~28 Qwen-specific tests to delete in other files, ~15-20 tests to modify (remove Qwen variant from multi-provider assertions).
- **frontend-qa**: 1 spec file to delete (`qwen-setup.spec.ts`, ~431 lines), 7 files to modify (remove mocks, assertions, mock registry data).
- **document-writer**: ~120+ Qwen references across 18 documentation files. Two entire sections to delete (auth device flow ~45 lines, troubleshooting ~25 lines). All gh-pages provider lists, diagrams, and tables need updating.

## File Manifest

| File | Action | Owner | Wave |
|------|--------|-------|------|
| `backend/src/providers/qwen.rs` | delete | rust-backend-engineer | 1 |
| `backend/src/web_ui/qwen_auth.rs` | delete | rust-backend-engineer | 1 |
| `backend/src/providers/types.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/config.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/providers/mod.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/providers/registry.rs` | modify | rust-backend-engineer | 2 |
| `backend/src/web_ui/mod.rs` | modify | rust-backend-engineer | 2 |
| `backend/src/web_ui/routes.rs` | modify | rust-backend-engineer | 2 |
| `backend/src/web_ui/config_api.rs` | modify | rust-backend-engineer | 2 |
| `backend/src/web_ui/config_db.rs` (query match arm + doc comment) | modify | rust-backend-engineer | 2 |
| `backend/src/web_ui/config_db.rs` (v22 migration DDL) | modify | database-engineer | 1 |
| `backend/src/web_ui/provider_oauth.rs` | modify | rust-backend-engineer | 2 |
| `backend/src/web_ui/provider_priority.rs` | modify | rust-backend-engineer | 2 |
| `backend/src/web_ui/model_registry.rs` | modify | rust-backend-engineer | 2 |
| `backend/src/resolver.rs` | modify | rust-backend-engineer | 2 |
| `backend/src/streaming/sse.rs` | modify | rust-backend-engineer | 2 |
| `backend/src/error.rs` | modify | rust-backend-engineer | 2 |
| `backend/src/providers/openai_codex.rs` | modify | rust-backend-engineer | 2 |
| `backend/src/providers/traits.rs` | modify | rust-backend-engineer | 2 |
| `backend/tests/integration_test.rs` | modify | rust-backend-engineer | 2 |
| `frontend/src/components/QwenSetup.tsx` | delete | react-frontend-engineer | 3 |
| `frontend/src/lib/api.ts` | modify | react-frontend-engineer | 3 |
| `frontend/src/components/OAuthSettings.tsx` | modify | react-frontend-engineer | 3 |
| `frontend/src/pages/providers/ConnectionsTab.tsx` | modify | react-frontend-engineer | 3 |
| `frontend/src/pages/providers/StatusTab.tsx` | modify | react-frontend-engineer | 3 |
| `frontend/src/pages/Providers.tsx` | modify | react-frontend-engineer | 3 |
| `docker-compose.yml` | modify | devops-engineer | 3 |
| `docker-compose.gateway.yml` | modify | devops-engineer | 3 |
| `.env.example` | modify | devops-engineer | 3 |
| `.env.proxy.example` | modify | devops-engineer | 3 |
| `backend/entrypoint.sh` | modify | devops-engineer | 3 |
| `.claude/rules/secrets.md` | modify | devops-engineer | 3 |
| `.claude/hooks/scan-secrets-before-write.sh` | modify | devops-engineer | 3 |
| `e2e-tests/specs/ui/qwen-setup.spec.ts` | delete | frontend-qa | 4 |
| `e2e-tests/playwright.config.ts` | modify | frontend-qa | 4 |
| `e2e-tests/specs/ui/provider-oauth.spec.ts` | modify | frontend-qa | 4 |
| `e2e-tests/specs/ui/copilot-setup.spec.ts` | modify | frontend-qa | 4 |
| `e2e-tests/specs/ui/admin.spec.ts` | modify | frontend-qa | 4 |
| `e2e-tests/specs/ui/multi-account.spec.ts` | modify | frontend-qa | 4 |
| `e2e-tests/specs/api/provider-status.spec.ts` | modify | frontend-qa | 4 |
| `e2e-tests/specs/api/config.spec.ts` | modify | frontend-qa | 4 |
| `gh-pages/index.md` | modify | document-writer | 5 |
| `gh-pages/docs/quickstart.md` | modify | document-writer | 5 |
| `gh-pages/docs/research-notes.md` | modify | document-writer | 5 |
| `gh-pages/docs/client-setup.md` | modify | document-writer | 5 |
| `gh-pages/docs/troubleshooting.md` | modify | document-writer | 5 |
| `gh-pages/docs/getting-started.md` | modify | document-writer | 5 |
| `gh-pages/docs/web-ui.md` | modify | document-writer | 5 |
| `gh-pages/docs/modules.md` | modify | document-writer | 5 |
| `gh-pages/docs/api-reference.md` | modify | document-writer | 5 |
| `gh-pages/docs/configuration.md` | modify | document-writer | 5 |
| `gh-pages/docs/deployment.md` | modify | document-writer | 5 |
| `gh-pages/docs/architecture/index.md` | modify | document-writer | 5 |
| `gh-pages/docs/architecture/request-flow.md` | modify | document-writer | 5 |
| `gh-pages/docs/architecture/converter-routing-summary.md` | modify | document-writer | 5 |
| `gh-pages/docs/architecture/streaming.md` | modify | document-writer | 5 |
| `gh-pages/docs/architecture/authentication.md` | modify | document-writer | 5 |
| `README.md` | modify | document-writer | 5 |
| `ARCHITECTURE.md` | modify | document-writer | 5 |

## Wave 1: Foundations (types, config, migration, file deletion)

- [ ] Remove `Qwen` variant from `ProviderId` enum in `providers/types.rs` and all match arms (`as_str`, `display_name`, `category`, `all_visible`, `default_base_url`, `FromStr`) + delete Qwen-specific tests (assigned: rust-backend-engineer)
  - Files: `backend/src/providers/types.rs`
  - Depends on: none

- [ ] Remove `qwen_oauth_client_id` from `Config`, `qwen_token`/`qwen_base_url` from `ProxyConfig`, env var reads, and update tests (assigned: rust-backend-engineer)
  - Files: `backend/src/config.rs`
  - Depends on: none

- [ ] Remove `pub mod qwen` and `QwenProvider::new()` from provider map (assigned: rust-backend-engineer)
  - Files: `backend/src/providers/mod.rs`
  - Depends on: none

- [ ] Delete `providers/qwen.rs` entirely (assigned: rust-backend-engineer)
  - Files: `backend/src/providers/qwen.rs`
  - Depends on: mod.rs removal of `pub mod qwen`

- [ ] Delete `web_ui/qwen_auth.rs` entirely (assigned: rust-backend-engineer)
  - Files: `backend/src/web_ui/qwen_auth.rs`
  - Depends on: web_ui/mod.rs removal of `pub mod qwen_auth` (wave 2)

- [ ] Write v22 migration to purge Qwen data from `user_provider_tokens`, `model_routes`, `model_registry`, `user_provider_priority`, `admin_provider_pool`, and `config` table (assigned: database-engineer)
  - Files: `backend/src/web_ui/config_db.rs` (DDL block only)
  - Depends on: none

## Wave 2: Backend consumers (registry, routes, handlers)

- [ ] Remove Qwen credential loading, model prefix matching (`qwen-`, `qwen3-`, `qwq-`), base_url special case, DB credential loading match arm, and delete ~340 lines of Qwen tests (assigned: rust-backend-engineer)
  - Files: `backend/src/providers/registry.rs`
  - Depends on: Wave 1 (ProviderId::Qwen removed)

- [ ] Remove `pub mod qwen_auth` and `.merge(qwen_auth::qwen_auth_routes())` (assigned: rust-backend-engineer)
  - Files: `backend/src/web_ui/mod.rs`
  - Depends on: Wave 1

- [ ] Remove `qwen_oauth_client_id` from GET/PUT config handlers (assigned: rust-backend-engineer)
  - Files: `backend/src/web_ui/routes.rs`
  - Depends on: Wave 1 (Config field removed)

- [ ] Remove `qwen_oauth_client_id` from classify, validate, schema/descriptions, and update tests (assigned: rust-backend-engineer)
  - Files: `backend/src/web_ui/config_api.rs`
  - Depends on: Wave 1

- [ ] Remove `qwen_oauth_client_id` match arm in `load_into_config` and update `set_user_provider_base_url` doc comment (assigned: rust-backend-engineer)
  - Files: `backend/src/web_ui/config_db.rs` (query area only)
  - Depends on: Wave 1

- [ ] Remove Qwen early return in `refresh_token` and entire `refresh_qwen_token` method, update tests (assigned: rust-backend-engineer)
  - Files: `backend/src/web_ui/provider_oauth.rs`
  - Depends on: Wave 1

- [ ] Update provider priority tests to remove Qwen assertions (assigned: rust-backend-engineer)
  - Files: `backend/src/web_ui/provider_priority.rs`
  - Depends on: Wave 1

- [ ] Remove `ProviderId::Qwen` match arm in model fetching, update test (assigned: rust-backend-engineer)
  - Files: `backend/src/web_ui/model_registry.rs`
  - Depends on: Wave 1

- [ ] Update comments in resolver.rs, streaming/sse.rs, error.rs, openai_codex.rs, traits.rs (assigned: rust-backend-engineer)
  - Files: `backend/src/resolver.rs`, `backend/src/streaming/sse.rs`, `backend/src/error.rs`, `backend/src/providers/openai_codex.rs`, `backend/src/providers/traits.rs`
  - Depends on: none

- [ ] Remove `qwen_oauth_client_id: String::new()` from integration test Config construction (assigned: rust-backend-engineer)
  - Files: `backend/tests/integration_test.rs` (line 88)
  - Depends on: Wave 1 (Config field removed)

## Wave 3: Frontend + Infrastructure (parallel with Wave 2)

- [ ] Delete `QwenSetup.tsx`, remove Qwen types/API functions from `api.ts`, remove from `OAuthSettings.tsx`, `ConnectionsTab.tsx`, `StatusTab.tsx`, `Providers.tsx` (assigned: react-frontend-engineer)
  - Files: 6 frontend files (see manifest)
  - Depends on: none (frontend is independently buildable)

- [ ] Remove Qwen env vars from docker-compose files, `.env.example`; in `entrypoint.sh` remove: Qwen device flow constants (lines 18-21), cache loading block (lines 251-262), `run_qwen_device_flow()` function (lines 377-471), device flow invocation (lines 478-479), summary line (line 489), update header comment (line 5); clean up `.claude/rules/secrets.md` and `.claude/hooks/scan-secrets-before-write.sh` (assigned: devops-engineer)
  - Files: 6 infrastructure files (see manifest)
  - Depends on: none

## Wave 4: E2E Test Cleanup

- [ ] Delete `qwen-setup.spec.ts`, remove from `playwright.config.ts` testMatch, remove Qwen mocks/assertions from 6 spec files. Update provider count assertion in `provider-status.spec.ts:50` from `toBe(5)` to `toBe(4)` and backend `provider_priority.rs:214` from `len(), 5` to `len(), 4` (assigned: frontend-qa)
  - Files: 8 e2e-tests files (see manifest)
  - Depends on: Wave 2 (backend routes removed), Wave 3 (frontend components removed)
  - Note: `provider_priority.rs:214` count update is in rust-backend-engineer's scope (Wave 2), listed here for cross-reference

## Wave 5: Documentation

- [ ] Remove all Qwen references from 15 gh-pages docs files, `README.md`, and `ARCHITECTURE.md`. Delete "Qwen Coder Device Flow" section from `authentication.md` (~45 lines) and "Qwen Coder" troubleshooting section (~25 lines). Update all provider lists, Mermaid diagrams, env var tables, and module references to reflect 5 providers. (assigned: document-writer)
  - Files: 17 documentation files (see manifest)
  - Depends on: Wave 2 (to verify final provider list)

## Interface Contracts

No new interfaces. This is a removal — the remaining 5 providers (`Kiro`, `Anthropic`, `OpenAICodex`, `Copilot`, `Custom`) continue unchanged. The `ProviderId` enum shrinks from 6 to 5 variants.

## Verification

| Gate | Command | Expected |
|------|---------|----------|
| Backend lint | `cd backend && cargo clippy --all-targets` | Zero warnings (no dead code from Qwen removal) |
| Backend format | `cd backend && cargo fmt --check` | No diffs |
| Backend unit tests | `cd backend && cargo test --lib` | All pass (~818 tests, down from ~931) |
| Backend integration | `cd backend && cargo test --test integration_test` | All pass |
| Frontend build | `cd frontend && npm run build` | Zero errors |
| Frontend lint | `cd frontend && npm run lint` | Zero errors |
| Docker config | `docker compose config --quiet` | No errors |

## Branch
`refactor/remove-qwen-provider`

## Codex Review Summary

Codex (gpt-5.4) reviewed this plan with 7 advisory agents. Review hit usage limit after 3 agents reported. Findings addressed:

| # | Severity | Finding | Action |
|---|----------|---------|--------|
| 1 | medium | Missing `backend/tests/integration_test.rs` (line 88: `qwen_oauth_client_id`) from manifest | **Added** to manifest and Wave 2 |
| 2 | medium | Missing `backend/src/providers/traits.rs` (line 71: comment) from manifest | **Added** to manifest and Wave 2 |
| 3 | medium | `entrypoint.sh` scope too narrow — only mentioned `run_qwen_device_flow()` deletion, missed constants, cache loading block, summary line, header comment | **Expanded** Wave 3 task to list all 6 entrypoint.sh blocks |
| 4 | medium | Provider count assertions (`all_visible().len()` and E2E `providers.length`) need 5→4 update | **Added** cross-reference note in Wave 4 |
| 5 | low | Verification missing integration test gate | **Added** `cargo test --test integration_test` to verification table |
| 6 | medium | Missing `.env.proxy.example` (lines 28-30: Qwen section) from manifest | **Added** to manifest and Wave 3 |

## Review Status
- Codex review: adjusted (3/7 agents reported before usage limit)
- Findings addressed: 6
- Disputed findings: 0
