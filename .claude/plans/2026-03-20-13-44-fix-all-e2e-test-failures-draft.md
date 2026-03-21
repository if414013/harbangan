# Plan: Fix All E2E Test Failures

## Context

After implementing E2E gap coverage (16 new files, 8 modified), test results: **154 failed, 91 passed**. Baseline on clean main: **171 failed, 83 passed**. Our changes are net positive (+8 passing). Failures fall into 3 categories that can be fixed in parallel by separate agents.

## Root Cause Analysis

### Track A: No API Key — ~116 failures (pre-existing)
All SDK and proxy tests fail with `401 Invalid or missing API Key`. The `API_KEY` env var is empty. These tests need a real proxy key to work.
- **Files (12)**: `sdk-openai-chat`, `sdk-openai-streaming`, `sdk-anthropic-chat`, `sdk-anthropic-streaming`, `sdk-models`, `sdk-error-handling`, `sdk-extended-thinking`, `sdk-tool-use`, `openai-chat`, `openai-streaming`, `anthropic-messages`, `anthropic-streaming`
- **Fix**: Add `test.skip(!process.env.API_KEY, 'API_KEY not set')` guard to each file's top-level describe. ~5 min work.

### Track B: UI Selector Mismatches — ~30 failures (pre-existing from PR#142)
Provider page refactoring moved sections, renamed buttons, changed component structure. Old selectors no longer match DOM.
- **Files (10)**: `profile.spec.ts` (6), `copilot-setup.spec.ts` (19), `qwen-setup.spec.ts` (22), `provider-oauth.spec.ts` (21), `navigation.spec.ts` (5), `dashboard.spec.ts` (4), `models.spec.ts` (5), `login.spec.ts` (5), `auth-redirect.spec.ts` (4), `totp-setup.spec.ts` (5)
- **Fix**: Read actual frontend pages, update selectors to match current DOM. The biggest changes are in provider-related tests where the Profile page no longer has provider sections (moved to Providers page).

### Track C: New Test Fixes — ~8 failures (our code)
New specs have response shape mismatches, wrong selectors, or expected-skip conditions.
- **Files (8)**: `usage.spec.ts` (8), `setup-mode.spec.ts` (3), `user-management.spec.ts` (1), `system.spec.ts` (1), `model-registry.spec.ts` (1), `provider-status.spec.ts` (1), `logout.spec.ts` (1), `guardrails.spec.ts` (1), `api-keys.spec.ts` (1)
- **Fix**: Run each individually, read error, fix assertion/selector. `setup-mode.spec.ts` should be skipped when running against live DB.

## Team: 5 Agents, All Parallel

### Agent 1: `fix-sdk` (frontend-qa) — Track A
Add API key skip guards to 12 SDK/proxy test files. Simple mechanical fix.
- **Files**: All `specs/api/sdk-*.spec.ts`, `openai-chat.spec.ts`, `openai-streaming.spec.ts`, `anthropic-messages.spec.ts`, `anthropic-streaming.spec.ts`, `models.spec.ts`
- **Size**: XS (12 files, same 1-line change each)

### Agent 2: `fix-providers-ui` (frontend-qa) — Track B part 1
Fix provider-related UI test selectors: copilot-setup, qwen-setup, provider-oauth. These are the largest failure group (~62 tests). Must read actual frontend components to get correct selectors.
- **Files**: `copilot-setup.spec.ts`, `qwen-setup.spec.ts`, `provider-oauth.spec.ts`
- **Needs to read**: `frontend/src/pages/Providers.tsx`, `frontend/src/components/CopilotSetup.tsx`, `frontend/src/components/QwenSetup.tsx`, `frontend/src/components/ProviderCard.tsx`, `frontend/src/pages/Profile.tsx`
- **Size**: L (3 files but ~62 test cases to fix)

### Agent 3: `fix-pages-ui` (frontend-qa) — Track B part 2
Fix remaining UI test selectors: profile, navigation, dashboard, models, login, auth-redirect, totp-setup.
- **Files**: `profile.spec.ts`, `navigation.spec.ts`, `dashboard.spec.ts`, `models.spec.ts` (UI), `login.spec.ts`, `auth-redirect.spec.ts`, `totp-setup.spec.ts`
- **Needs to read**: Corresponding frontend pages and `helpers/selectors.ts`
- **Size**: M (7 files, ~34 tests)

### Agent 4: `fix-new-api` (frontend-qa) — Track C API
Fix new API test failures. Run each spec individually, read error, fix response shape or auth.
- **Files**: `user-management.spec.ts`, `system.spec.ts`, `model-registry.spec.ts`, `provider-status.spec.ts`, `logout.spec.ts`, `guardrails.spec.ts`, `api-keys.spec.ts`, `setup-mode.spec.ts`
- **Method**: `npx playwright test specs/api/<file> --reporter=list` per file, fix each error
- **Size**: M

### Agent 5: `fix-new-ui` (frontend-qa) — Track C UI
Fix new UI test failures (usage.spec.ts is the main one — 8 failures).
- **Files**: `usage.spec.ts`, `logout-redirect.spec.ts`, `profile-actions.spec.ts`, `user-detail.spec.ts`, `admin-users.spec.ts`, `admin.spec.ts`, `guardrails.spec.ts` (UI)
- **Method**: `npx playwright test specs/ui/<file> --reporter=list` per file, fix selectors
- **Size**: M

## File Manifest

| File | Action | Owner | Track |
|------|--------|-------|-------|
| `specs/api/sdk-openai-chat.spec.ts` | modify (skip guard) | fix-sdk | A |
| `specs/api/sdk-openai-streaming.spec.ts` | modify (skip guard) | fix-sdk | A |
| `specs/api/sdk-anthropic-chat.spec.ts` | modify (skip guard) | fix-sdk | A |
| `specs/api/sdk-anthropic-streaming.spec.ts` | modify (skip guard) | fix-sdk | A |
| `specs/api/sdk-models.spec.ts` | modify (skip guard) | fix-sdk | A |
| `specs/api/sdk-error-handling.spec.ts` | modify (skip guard) | fix-sdk | A |
| `specs/api/sdk-extended-thinking.spec.ts` | modify (skip guard) | fix-sdk | A |
| `specs/api/sdk-tool-use.spec.ts` | modify (skip guard) | fix-sdk | A |
| `specs/api/openai-chat.spec.ts` | modify (skip guard) | fix-sdk | A |
| `specs/api/openai-streaming.spec.ts` | modify (skip guard) | fix-sdk | A |
| `specs/api/anthropic-messages.spec.ts` | modify (skip guard) | fix-sdk | A |
| `specs/api/anthropic-streaming.spec.ts` | modify (skip guard) | fix-sdk | A |
| `specs/api/models.spec.ts` | modify (skip guard) | fix-sdk | A |
| `specs/ui/copilot-setup.spec.ts` | modify (selectors) | fix-providers-ui | B1 |
| `specs/ui/qwen-setup.spec.ts` | modify (selectors) | fix-providers-ui | B1 |
| `specs/ui/provider-oauth.spec.ts` | modify (selectors) | fix-providers-ui | B1 |
| `specs/ui/profile.spec.ts` | modify (selectors) | fix-pages-ui | B2 |
| `specs/ui/navigation.spec.ts` | modify (selectors) | fix-pages-ui | B2 |
| `specs/ui/dashboard.spec.ts` | modify (selectors) | fix-pages-ui | B2 |
| `specs/ui/models.spec.ts` | modify (selectors) | fix-pages-ui | B2 |
| `specs/ui/login.spec.ts` | modify (selectors) | fix-pages-ui | B2 |
| `specs/ui/auth-redirect.spec.ts` | modify (selectors) | fix-pages-ui | B2 |
| `specs/ui/totp-setup.spec.ts` | modify (selectors) | fix-pages-ui | B2 |
| `specs/api/user-management.spec.ts` | modify (fix) | fix-new-api | C |
| `specs/api/system.spec.ts` | modify (fix) | fix-new-api | C |
| `specs/api/model-registry.spec.ts` | modify (fix) | fix-new-api | C |
| `specs/api/provider-status.spec.ts` | modify (fix) | fix-new-api | C |
| `specs/api/logout.spec.ts` | modify (fix) | fix-new-api | C |
| `specs/api/guardrails.spec.ts` | modify (fix) | fix-new-api | C |
| `specs/api/api-keys.spec.ts` | modify (fix) | fix-new-api | C |
| `specs/api/setup-mode.spec.ts` | modify (fix) | fix-new-api | C |
| `specs/ui/usage.spec.ts` | modify (fix) | fix-new-ui | C |
| `specs/ui/logout-redirect.spec.ts` | modify (fix) | fix-new-ui | C |
| `specs/ui/profile-actions.spec.ts` | modify (fix) | fix-new-ui | C |
| `specs/ui/user-detail.spec.ts` | modify (fix) | fix-new-ui | C |
| `specs/ui/admin-users.spec.ts` | modify (fix) | fix-new-ui | C |
| `specs/ui/admin.spec.ts` | modify (fix) | fix-new-ui | C |
| `specs/ui/guardrails.spec.ts` | modify (fix) | fix-new-ui | C |

## Verification

Each agent runs their own files after fixing:
- `npx playwright test specs/api/<file> --reporter=list`
- `npx playwright test --project=<project> specs/ui/<file> --reporter=list`

Final: `cd e2e-tests && npm test` — target: **0 unexpected failures** (setup-mode skips, SDK tests skip without API key)
