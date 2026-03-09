# model-registry-refactor_20260309: Model Registry Refactor

**Type**: refactor
**Created**: 2026-03-09
**Preset**: fullstack
**Services**: backend, frontend, backend-qa, frontend-qa
**Dependency**: rename-openai-provider_20260309

## Problem Statement

The current model system is fragmented: Kiro-only discovery via in-memory `ModelCache`, no model lists for other providers (Anthropic, OpenAI, Gemini, Copilot, Qwen), hardcoded prefix routing in `provider_for_model()` that doesn't let clients choose which provider serves a model, and admin has no visibility or control over available models.

## Task Summary

Replace the in-memory `ModelCache` + hardcoded prefix routing with a PostgreSQL-backed global model registry. Admin populates models per provider, manages enable/disable. Clients use prefixed model IDs (e.g. `kiro_claude-sonnet-4`) for explicit provider routing.

## Success Criteria

1. `model_registry` table in PostgreSQL with full CRUD operations
2. Admin can populate models from all connected providers via the web UI
3. Admin can enable/disable individual models for all users
4. `GET /v1/models` returns prefixed IDs from the registry (only enabled models)
5. Prefixed model names (e.g. `kiro_claude-sonnet-4`) route correctly to the specified provider
6. Unprefixed model names return 400 error (clean break, no backward compat)

## Scope Boundaries

**In scope:**
- Database migration for `model_registry` table
- Static model lists for Anthropic, OpenAI, Gemini, Qwen + dynamic API fetch when credentials available
- Dynamic model fetch for Kiro (existing `ListAvailableModels`) and Copilot (`GET /models`)
- Prefix-based routing (`{provider}_{model}`) in request handlers
- Admin API endpoints for model management
- Frontend Models admin page
- Refactored `ModelCache` backed by DB
- Unit tests for new logic

**Out of scope:**
- Backward compatibility for unprefixed model names
- Per-user model visibility (all enabled models visible to all users)
- Model usage analytics or cost tracking
- Proxy-only mode changes (no DB = no registry, existing behavior)

## Dependencies

- `rename-openai-provider_20260309` — must complete first (renames OpenAI → OpenAI Codex in provider types)

## Risks

1. Kiro hidden model internal ID mapping (e.g. `claude-sonnet-4` → `CLAUDE_SONNET_4_20250514_V1_0`) must be preserved in `upstream_meta`
2. Proxy-only mode (no DB) needs graceful fallback — cache stays empty, requests pass through
3. Cache invalidation timing when admin toggles models — immediate reload after DB update
