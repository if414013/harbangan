# copilot-device-code_20260308: Switch Copilot OAuth to Device Code Flow

**Type**: refactor
**Created**: 2026-03-08
**Preset**: fullstack
**Services**: backend, frontend

## Problem Statement

The Copilot OAuth implementation uses Authorization Code Grant, requiring users to register a custom GitHub OAuth app and configure 3 env vars (`GITHUB_COPILOT_CLIENT_ID`, `GITHUB_COPILOT_CLIENT_SECRET`, `GITHUB_COPILOT_CALLBACK_URL`). This is incorrect — GitHub Copilot uses the Device Code Flow with a well-known hardcoded client_id (`Iv1.b507a08c87ecfe98`, GitHub's VS Code extension). No client secret, callback URL, or custom OAuth app registration needed.

The reference implementation at `copilot-api/` confirms this. Harbangan's own Kiro and Qwen providers already use device code flows successfully.

## Motivation

Currently the `/copilot/connect` endpoint returns a config error because no one can reasonably set up the required env vars — they shouldn't exist. This blocks all Copilot provider usage in the web UI.

## Acceptance Criteria

1. Copilot connect flow uses GitHub Device Code Flow (POST to `https://github.com/login/device/code`)
2. No env vars required — hardcoded client_id `Iv1.b507a08c87ecfe98`, scope `read:user`
3. `GITHUB_COPILOT_CLIENT_ID`, `GITHUB_COPILOT_CLIENT_SECRET`, `GITHUB_COPILOT_CALLBACK_URL` removed from config
4. Frontend shows device code UI (user_code + verification_uri) matching Kiro/Qwen pattern
5. After authorization: GitHub username, Copilot plan, and bearer token stored in DB (existing schema)
6. Background token refresh continues working (uses stored github_token)
7. Status and disconnect endpoints unchanged
8. All existing copilot-related tests updated, new tests for device code types and RFC 8628 error handling

## Scope Boundaries

**Out of scope:**
- DB schema changes (existing `user_copilot_tokens` table works as-is)
- CopilotProvider changes (consumes tokens from cache/DB, unchanged)
- Background refresh task logic changes (still refreshes copilot bearer token using github_token)

## Dependencies

- Existing `DeviceCodeDisplay` frontend component (used by Kiro/Qwen)
- Existing `DevicePollResponse` type in `api.ts`
- Qwen device code pattern in `qwen_auth.rs` (structural reference)
