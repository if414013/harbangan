# Plan: Static fallback model lists for Anthropic and OpenAI Codex

## Context

After fixing the Kiro region bug (#180), Kiro and Copilot now populate models successfully. However, Anthropic and OpenAI Codex still show 0 models because their OAuth relay tokens can't call `/v1/models`:
- Anthropic: OAT token (`sk-ant-oat01-...`) rejected with 401 `invalid x-api-key`
- OpenAI Codex: JWT token missing `api.model.read` scope, returns 403

CLIProxyAPI solves this by shipping static model definitions in `models.json` — no API calls needed. We'll adopt the same approach: add hardcoded fallback model lists that populate when API calls fail.

## Approach

Add a `static_models.rs` module with hardcoded model definitions for Anthropic and OpenAI Codex. Modify `populate_provider()` to fall back to these static lists when the API call returns empty/fails. Static models use `source: "static"` to distinguish from API-fetched models.

Model data sourced from CLIProxyAPI's `internal/registry/models/models.json`.

### Anthropic static models (from CLIProxyAPI "claude" section)
- claude-haiku-4-5-20251001 (200k ctx, 64k output)
- claude-sonnet-4-5-20250929 (200k ctx, 64k output)
- claude-sonnet-4-6 (200k ctx, 64k output)
- claude-opus-4-6 (1M ctx, 128k output)
- claude-opus-4-5-20251101 (200k ctx, 64k output)
- claude-opus-4-1-20250805 (200k ctx, 32k output)
- claude-opus-4-20250514 (200k ctx, 32k output)
- claude-sonnet-4-20250514 (200k ctx, 64k output)
- claude-3-7-sonnet-20250219 (128k ctx, 8k output)
- claude-3-5-haiku-20241022 (128k ctx, 8k output)

### OpenAI Codex static models (from CLIProxyAPI "codex-pro" section)
- gpt-5 (400k ctx, 128k output)
- gpt-5-codex (400k ctx, 128k output)
- gpt-5-codex-mini (400k ctx, 128k output)
- gpt-5.1 (400k ctx, 128k output)
- gpt-5.1-codex (400k ctx, 128k output)
- gpt-4.1 (1M ctx, 32k output)
- gpt-4o (128k ctx, 16k output)
- gpt-4o-mini (128k ctx, 16k output)
- o3 (200k ctx, 100k output)
- o4-mini (200k ctx, 100k output)

## File Manifest

| File | Action | Owner | Wave |
|------|--------|-------|------|
| `backend/src/web_ui/static_models.rs` | create | rust-backend-engineer | 1 |
| `backend/src/web_ui/mod.rs` | modify | rust-backend-engineer | 1 |
| `backend/src/web_ui/model_registry.rs` | modify | rust-backend-engineer | 2 |

## Wave 1: Static model definitions

- [ ] Create `backend/src/web_ui/static_models.rs` with two functions:
  - `pub fn static_anthropic_models() -> Vec<RegistryModel>` — returns hardcoded Anthropic models with `source: "static"`
  - `pub fn static_openai_codex_models() -> Vec<RegistryModel>` — returns hardcoded OpenAI Codex models with `source: "static"`
  - Each model uses `generate_prefixed_id()` for the `prefixed_id` field
  - `enabled: false` (user must explicitly enable)
  - `capabilities: json!({})` (empty — no special capabilities metadata)
  - `upstream_meta: None`
  - Files: `backend/src/web_ui/static_models.rs`, `backend/src/web_ui/mod.rs`

## Wave 2: Integrate fallback into populate

- [ ] Modify `populate_provider()` in `model_registry.rs`:
  - After the existing API call chain for `ProviderId::Anthropic`, if `api_models` is `None`, fall back to `static_models::static_anthropic_models()`
  - Same for `ProviderId::OpenAICodex` → `static_models::static_openai_codex_models()`
  - Log: `tracing::info!(provider, "Using static model definitions as fallback")`
  - The static fallback uses `source: "static"` — if a future API call succeeds, `bulk_upsert` will overwrite with `source: "api"` (ON CONFLICT updates source)
  - Files: `backend/src/web_ui/model_registry.rs`

## Key design decisions

1. Static models are a **fallback only** — API-fetched models take priority via the existing flow
2. `source: "static"` distinguishes fallback models from API-fetched ones in the UI
3. `enabled: false` by default — user must opt-in to each model
4. `bulk_upsert` uses `ON CONFLICT (provider_id, model_id)` — if API later works, it overwrites static entries
5. No DB migration needed — uses existing `model_registry` table

## Verification

```bash
cd backend && cargo clippy --all-targets   # zero new warnings
cd backend && cargo test --lib model_registry::  # existing + new tests pass
cd backend && cargo test --lib static_models::   # new static model tests
docker compose build backend && docker compose up -d backend
# Click "populate all" → Anthropic and OpenAI Codex should show models
```

## Branch

`fix/static-model-fallback`
