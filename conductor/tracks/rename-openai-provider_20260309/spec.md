# rename-openai-provider_20260309: Rename OpenAI Provider

**Type**: refactor
**Created**: 2026-03-09
**Preset**: fullstack
**Services**: backend, frontend

## Problem Statement

Rename provider_id `openai` to `openai_codex` across DB schema, Rust backend (types, OAuth, routes, priority), and frontend (Profile page display). Includes DB migration for existing data. The "openai" provider actually connects to OpenAI Codex (used by Codex CLI), not the general OpenAI API. The name is misleading and could confuse users.

## Success Criteria

- All `openai` provider references renamed to `openai_codex` in code and DB
- Existing user data migrated (provider tokens, keys, priorities, model routes)
- UI shows "OpenAI Codex" on the Profile page
- All backend tests pass (`cargo test --lib`)
- No breaking changes to external API format (OpenAI chat completions format is unchanged)

## Scope Boundaries

- Only the provider identity is renamed — the OpenAI API format converters (`openai_to_kiro`, etc.) are NOT renamed
- The `OpenAIProvider` struct handles OpenAI-compatible API calls — it gets renamed to `OpenAICodexProvider`
- Environment variable `OPENAI_OAUTH_CLIENT_ID` stays as-is (or optionally renamed)

## Risk Assessment

- DB migration must handle existing rows with `openai` provider_id
- CHECK constraints need updating atomically — drop old, add new in same transaction
- If migration fails mid-way, data could be inconsistent
- Serde rename on `ProviderId` enum affects API serialization — must verify no external consumers depend on `"openai"` as a provider ID in API responses
