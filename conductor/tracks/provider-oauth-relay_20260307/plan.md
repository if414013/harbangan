# provider-oauth-relay_20260307: Implementation Plan

**Status**: planned
**Branch**: feat/provider-oauth-relay_20260307

---

## Phase 1: Backend — DB Migration v8
Agent: rust-backend-engineer

- [ ] 1.1 — Add `migrate_to_v8()` in `config_db.rs`: create `user_provider_tokens` table with UNIQUE(user_id, provider_id), refresh_token DEFAULT '', provider_id CHECK constraint
- [ ] 1.2 — Implement `upsert_user_provider_token()` with conditional refresh_token update (only overwrite when non-empty)
- [ ] 1.3 — Implement `get_user_provider_token()`, `delete_user_provider_token()`, `get_user_connected_providers()`
- [ ] 1.4 — Unit tests for all new DB methods (upsert idempotency, refresh_token preservation, cascade delete)

## Phase 2: Backend — OAuth Endpoints
Agent: rust-backend-engineer

- [ ] 2.1 — Define `ProviderOAuthPendingState` struct, add `provider_oauth_pending` DashMap to AppState (separate from `oauth_pending`)
- [ ] 2.2 — Define `TokenExchanger` trait with `exchange_code()` and `refresh_token()` methods; implement production `HttpTokenExchanger`; add `Arc<dyn TokenExchanger>` to AppState
- [ ] 2.3 — Implement provider config constants (client IDs, token URLs, redirect URIs, userinfo endpoints) with env var overrides
- [ ] 2.4 — Implement `GET /providers/status` endpoint (session-authenticated)
- [ ] 2.5 — Implement `GET /providers/{provider}/connect` endpoint: PKCE generation, relay_token creation, pending state storage with duplicate invalidation
- [ ] 2.6 — Implement `GET /providers/{provider}/relay-script` endpoint: dynamic shell script generation with DOMAIN sanitization
- [ ] 2.7 — Implement `POST /providers/{provider}/relay` endpoint: relay_token validation (single-use), token exchange, userinfo fetch, DB storage with retry, cache invalidation
- [ ] 2.8 — Implement `DELETE /providers/{provider}` endpoint (CSRF-protected disconnect)
- [ ] 2.9 — Provider path validation middleware (reject unknown providers with 400)
- [ ] 2.10 — Swap `provider_keys` routes for `provider_oauth` routes in `web_ui/mod.rs`
- [ ] 2.11 — Unit tests for relay endpoint scenarios (valid flow, expired token, consumed token, state mismatch, invalid provider, exchange failure, DB retry)

## Phase 3: Backend — Registry Update + Token Refresh
Agent: rust-backend-engineer

- [ ] 3.1 — Implement `ensure_fresh_token()` with mutex-based refresh locking per (user_id, provider)
- [ ] 3.2 — Update `registry.rs` to load from `user_provider_tokens` instead of `user_provider_keys`
- [ ] 3.3 — Handle permanent refresh failure: delete token row, invalidate cache, fall back to Kiro
- [ ] 3.4 — Delete `provider_keys.rs` and `key_detection.rs`
- [ ] 3.5 — Unit tests for token refresh (transparent refresh, concurrent mutex, permanent failure fallback)

## Phase 4: Frontend — Remove Providers Page
Agent: react-frontend-engineer

- [ ] 4.1 — Delete `frontend/src/pages/Providers.tsx`
- [ ] 4.2 — Remove providers route from `App.tsx` and nav link from `Sidebar.tsx`
- [ ] 4.3 — Remove API key types/functions from `api.ts` (`addProviderKey`, `removeProviderKey`, `ProviderInfo`, `ProvidersStatusResponse`, `AddProviderKeyResponse`)
- [ ] 4.4 — Remove orphaned CSS classes from `components.css` (`.providers-page`, `.providers-grid`, `.provider-key-form`, `.provider-key-input`, `.provider-key-prefix`, `.provider-key-value`, `.provider-models`, `.provider-models-label`, `.provider-models-list`, `.provider-model-tag`)
- [ ] 4.5 — Delete `frontend/e2e/specs/providers.spec.ts`

## Phase 5: Frontend — PROVIDERS Section in Profile
Agent: react-frontend-engineer

- [ ] 5.1 — Add OAuth API functions to `api.ts`: `getProvidersStatus()`, `getProviderConnectUrl()`, `disconnectProvider()`
- [ ] 5.2 — Build PROVIDERS section in `Profile.tsx` with connect/disconnect cards per provider (reuse KiroSetup card pattern)
- [ ] 5.3 — Implement relay modal: curl command display with copy button (reuse DeviceCodeDisplay pattern), polling with useEffect cleanup, 10-min timeout, cancel button
- [ ] 5.4 — CSS for provider OAuth cards and relay modal in `components.css`

## Phase 6: QA — Backend Tests
Agent: backend-qa

- [ ] 6.1 — Verify all existing tests still pass (`cargo test --lib`)
- [ ] 6.2 — Verify `cargo clippy` passes with no warnings
- [ ] 6.3 — Review test coverage for relay endpoints (tasks 2.11, 3.5) and flag gaps

## Phase 7: QA — Frontend E2E
Agent: frontend-qa

- [ ] 7.1 — Verify `npm run lint && npm run build` passes clean
- [ ] 7.2 — Write Playwright E2E tests for PROVIDERS section on Profile page (connected/disconnected states, connect modal display, disconnect flow)
- [ ] 7.3 — Verify `/providers` route returns 404 and no API key input exists in UI
