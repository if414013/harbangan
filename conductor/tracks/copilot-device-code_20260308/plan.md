# copilot-device-code_20260308: Implementation Plan

**Status**: completed
**Branch**: refactor/copilot-device-code

## Phase 1: Backend
Agent: rust-backend-engineer

- [x] 1.1 — Remove `github_copilot_client_id`, `github_copilot_client_secret`, `github_copilot_callback_url` from Config struct, `with_defaults()`, and `load()` in `backend/src/config.rs`
- [x] 1.2 — Add `copilot_device_pending: CopilotDevicePendingMap` to AppState in `backend/src/routes/mod.rs` and initialize in `backend/src/main.rs`
- [x] 1.3 — Rewrite `backend/src/web_ui/copilot_auth.rs`: remove `copilot_connect`, `copilot_callback`, `exchange_github_code`, `CallbackQuery`, `GITHUB_AUTH_URL`; add `GITHUB_CLIENT_ID` (`Iv1.b507a08c87ecfe98`), `GITHUB_DEVICE_CODE_URL`, `GITHUB_SCOPE`; implement `POST /copilot/device-code` and `GET /copilot/device-poll` handlers following Qwen pattern; handle RFC 8628 error codes (`authorization_pending`, `slow_down`, `expired_token`, `access_denied`)
- [x] 1.4 — Update unit tests: remove tests for deleted types/functions, add tests for new device code response types, pending map operations, RFC 8628 error codes; keep existing tests for `base_url_for_plan`, status, helpers
- [x] 1.5 — Run `cargo clippy`, `cargo fmt`, `cargo test --lib` — 763 tests passed

## Phase 2: Frontend
Agent: react-frontend-engineer

- [x] 2.1 — Add `startCopilotDeviceFlow()` (POST) and `pollCopilotDeviceCode()` (GET) to `frontend/src/lib/api.ts`; reuse existing `DevicePollResponse` type
- [x] 2.2 — Rewrite `frontend/src/components/CopilotSetup.tsx` to use `DeviceCodeDisplay` component (matching QwenSetup pattern); keep copilot-info section (github_username + copilot_plan); remove URL param handling for `?copilot=connected`
- [x] 2.3 — Run `npm run lint`, `npm run build` — all green

## Phase 3: Cleanup
Agent: rust-backend-engineer

- [x] 3.1 — Remove `GITHUB_COPILOT_*` env vars from `.env.example`
