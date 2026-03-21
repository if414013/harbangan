# Plan: PR #149 Review Follow-up Fixes

## Context

PR #149 (dynamic provider registry) passed code review with 0 critical, 1 high, 5 medium, 7 low findings. This plan addresses the HIGH and MEDIUM items plus the most impactful LOWs. All work is in the existing worktree at `.trees/dynamic-provider-registry/`.

## File Manifest

| File | Action | Owner | CR# |
|------|--------|-------|-----|
| `backend/src/web_ui/provider_priority.rs` | modify | rust-backend-engineer | CR-002 |
| `backend/src/web_ui/model_registry.rs` | add tests | rust-backend-engineer | CR-003 |
| `backend/src/web_ui/admin_pool.rs` | add tests | rust-backend-engineer | CR-006 |
| `backend/src/web_ui/model_registry_handlers.rs` | modify | rust-backend-engineer | CR-008 |
| `backend/src/providers/registry.rs` | modify | rust-backend-engineer | CR-007, CR-009 |
| `backend/src/web_ui/config_db.rs` | add comment | rust-backend-engineer | CR-005 |
| `frontend/src/pages/Admin.tsx` | modify | react-frontend-engineer | CR-011 |
| `frontend/src/pages/Providers.tsx` | modify | react-frontend-engineer | CR-012 |

## Tasks

### T1: CR-002 — Migrate `provider_priority.rs` validation (MEDIUM, production code)

Replace hardcoded `VALID_PROVIDERS` (line 67) with `ProviderId::from_str()`:
```rust
// Before:
const VALID_PROVIDERS: &[&str] = &["kiro", "anthropic", "openai_codex", "copilot", "qwen"];
// ...
if !VALID_PROVIDERS.contains(&p.provider_id.as_str()) { ... }

// After:
use crate::providers::types::ProviderId;
// ...
ProviderId::from_str(&p.provider_id).map_err(|_| ApiError::ValidationError(...))?;
```
Update existing tests at lines 193-251 to use `ProviderId` instead of `VALID_PROVIDERS`.

### T2: CR-003 — Add test for keep-last-successful behavior (MEDIUM)

Add unit test in `model_registry.rs` verifying:
- When API returns `None` → function returns `Ok(0)`, no DB write
- When API returns empty vec → function returns `Ok(0)`, no DB write
- When API returns models → function proceeds to upsert

### T3: CR-006 — Add test for pool validation (MEDIUM)

Add unit test in `admin_pool.rs` verifying:
- Valid providers (anthropic, kiro, etc.) pass `ProviderId::from_str()` + `supports_pool()`
- `"custom"` is rejected by `supports_pool()`
- Unknown string `"gemini"` is rejected by `from_str()`

### T4: CR-007 — Delete dead `configured_proxy_providers()` (LOW)

Remove `configured_proxy_providers()` method (line 579) and its `#[allow(dead_code)]`. Remove associated tests at lines 1905-1913. It's unused after `known_models.rs` deletion.

### T5: CR-008, CR-009, CR-010 — Replace hardcoded lists in tests (LOW)

- `model_registry_handlers.rs:182` — replace `vec!["anthropic", ...]` with `ProviderId::all_visible().iter().map(|p| p.as_str().to_string()).collect()`
- `registry.rs:607` — replace `&["anthropic", "openai_codex", "qwen"]` with `ProviderId::all_visible()` iteration
- `provider_oauth.rs:1249` — replace `&["anthropic", "openai_codex"]` with filtered `all_visible()`

### T6: CR-005 — Add comment to migration v21 (MEDIUM)

Add comment in `config_db.rs` migration v21 documenting the tradeoff: CHECK constraints dropped, validation now in Rust via `ProviderId::from_str()`.

### T7: CR-011 — Fix silent error swallowing in Admin.tsx (LOW)

Replace empty `.catch(() => {})` with `.catch(() => showToast("Failed to load providers", "error"))`.

### T8: CR-012 — Memoize derived arrays in Providers.tsx (LOW)

Wrap `allProviders`, `multiAccountProviders`, `deviceCodeProviders` in `useMemo(registry)`.

### CR-001 (HIGH) — Proxy mode `/v1/models` — DEFERRED

Proxy mode (`docker-compose.gateway.yml`) has no DB, so the model registry is empty. `/v1/models` returns only custom env models. This is acceptable for now — proxy users use explicit model names. A proper fix would require a separate mechanism (env-based model list or embedded defaults for proxy mode). Out of scope for this follow-up.

## Verification

```bash
cd .trees/dynamic-provider-registry/backend && cargo clippy --all-targets && cargo test --lib && cargo fmt --check
cd .trees/dynamic-provider-registry/frontend && npm run build && npm run lint
```

## Recommended Preset

Single `rust-backend-engineer` agent (T1-T6) + single `react-frontend-engineer` agent (T7-T8). Both can run in parallel.
