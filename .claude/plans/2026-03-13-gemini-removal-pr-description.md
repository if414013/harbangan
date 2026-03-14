# Gemini Provider Removal -- PR Description

**Branch:** `feat/gemini-propider-removal` -> `main`

**Title:** `refactor(backend): remove Gemini provider and all associated converters`

**Body:**

```
## Summary

- **Remove the Gemini provider entirely** from the AI gateway, deleting ~1,300 lines across 5 files: core provider implementation (`providers/gemini.rs`) and all four bidirectional format converters (`openai_to_gemini`, `anthropic_to_gemini`, `gemini_to_openai`, `gemini_to_anthropic`)
- **Clean up all backend references** (~14 files): remove `Gemini` variant from `ProviderId` enum and all match arms, unregister the provider from the registry and router, strip Gemini branches from OAuth config, model registry, provider priority, and main startup
- **Add DB migration v13** that deletes all Gemini rows from `user_provider_keys`, `user_provider_tokens`, `model_routes`, `user_provider_priority`, and `model_registry`, then drops and re-creates CHECK constraints on those tables without the `'gemini'` value
- **Update frontend and E2E tests**: remove `'gemini'` from the providers array in `Profile.tsx` (3 to 2 providers), remove Gemini mock data and test cases from Playwright specs, and update expected card counts
- The gateway retains 5 supported providers after this change: Kiro, Anthropic, OpenAI Codex, Copilot, and Qwen. Gemini was rarely used and its converters were largely dead code; removing it reduces maintenance surface and simplifies the provider subsystem.

## Test plan

- [ ] `cd backend && cargo clippy --all-targets` passes with zero warnings
- [ ] `cd backend && cargo fmt --check` reports no diffs
- [ ] `cd backend && cargo test --lib` passes all tests (confirm updated test counts in `provider_priority`, `provider_oauth`, `model_registry`, and `config_db`)
- [ ] `cd frontend && npm run build && npm run lint` passes with zero errors
- [ ] `cd e2e-tests && npm run test:ui` passes with updated provider card counts and no Gemini references
- [ ] Verify migration v13 applies cleanly: Gemini rows are deleted and CHECK constraints no longer include `'gemini'`
- [ ] Confirm no remaining references to `gemini` (case-insensitive) in `backend/src/`, `frontend/src/`, or `e2e-tests/` outside of git history and migration comments
- [ ] `docker compose build` succeeds

🤖 Generated with [Claude Code](https://claude.com/claude-code)
```
