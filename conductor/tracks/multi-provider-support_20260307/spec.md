# multi-provider-support_20260307: Multi-Provider Support

**Type**: feature
**Created**: 2026-03-07
**Preset**: fullstack
**Services**: backend, frontend, backend-qa, frontend-qa

## Problem Statement

rkgw currently proxies ALL requests through Kiro API (AWS CodeWhisperer). Users want to use their own provider API keys (Anthropic, OpenAI, Gemini) for direct access. The gateway should auto-detect the provider from the key format, auto-route matching models to the correct provider, and fall back to Kiro when no provider key exists.

## User Story

As a gateway user, I want to add my own provider API keys so that requests for matching models auto-route directly to those providers, giving me access to a broader model catalog while keeping Kiro as a fallback.

## Acceptance Criteria

1. No provider keys added -> all requests go to Kiro (zero behavior change)
2. Add Anthropic key -> `claude-*` models route to Anthropic directly
3. Add OpenAI key -> `gpt-*/o1*/o3*/o4*` models route to OpenAI directly
4. Add Gemini key -> `gemini-*` models route to Gemini directly
5. Remove a provider key -> matching models fall back to Kiro
6. `GET /providers/status` returns correct connection status + model lists
7. Streaming works for all providers (SSE for direct, AWS Event Stream for Kiro)
8. Frontend Providers page shows cards, add/remove keys works
9. All existing tests pass (Kiro path unchanged)
10. `cargo clippy` -- no warnings

## Scope Boundaries

Everything in the plan document is in scope (phases 1-10 from rkgw-multi-provider-plan.md). No exclusions for v1.

Three direct providers: Anthropic, OpenAI, Gemini. Kiro remains as fallback.

## Dependencies

- Existing patterns: `user_kiro.rs`, `KiroSetup.tsx`, `converters/core.rs`, `streaming/mod.rs`, `config_db.rs`
- No blockers identified
- Light-mode-toggle track does not conflict

## Reference

- Plan document: `/docs/rkgw-multi-provider-plan.md`
