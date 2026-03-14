# Plan: Admin-Only Model Loading — No Providers Enabled by Default

## Context

Currently, on startup the backend auto-populates the model registry for `anthropic`, `openai_codex`, and `qwen` with all static models set to `enabled: true`. This means ~26 models are immediately available without any admin action. The desired behavior is that the model registry starts empty, models are only populated when an admin clicks "Populate" in the UI, and newly populated models default to `enabled: false` so the admin must explicitly enable the ones they want.

## Scope

Backend-only (Rust). No frontend changes, no DB migration needed.

## Changes

### 1. Remove auto-population on startup

**File:** `backend/src/main.rs` (lines 240-284)

Remove the loop that iterates over `["anthropic", "openai_codex", "qwen"]` and calls `populate_provider()` for each. Keep the `model_cache.load_from_registry().await` call so previously-configured models still load from DB into cache.

Before:
```rust
if !is_proxy_only {
    if let Some(ref db) = config_db {
        let providers = ["anthropic", "openai_codex", "qwen"];
        for provider_id in &providers {
            match web_ui::model_registry::populate_provider(...) { ... }
        }
        // Load enabled registry models into in-memory cache
        match model_cache.load_from_registry().await { ... }
    }
}
```

After:
```rust
if !is_proxy_only {
    if let Some(ref _db) = config_db {
        // Load enabled registry models into in-memory cache
        // (admin populates models via UI; no auto-population on startup)
        match model_cache.load_from_registry().await { ... }
    }
}
```

### 2. Default all new models to `enabled: false`

**File:** `backend/src/web_ui/model_registry.rs`

Change `enabled: true` → `enabled: false` in these locations:

| Function | Line | Description |
|----------|------|-------------|
| `static_to_registry()` | ~38 | All static model definitions |
| `fetch_kiro_models()` | ~324 | Kiro API-fetched models |
| `fetch_anthropic_models()` | ~467 | Anthropic API-fetched models |
| `parse_openai_models_response()` | ~502 | OpenAI/Copilot API-fetched models |

This is safe because the `bulk_upsert_registry_models()` ON CONFLICT clause does NOT update `enabled` — re-populating preserves admin's existing enable/disable choices.

### 3. Update unit tests

**File:** `backend/src/web_ui/model_registry.rs` (test module)

- `test_anthropic_static_models_not_empty` (line ~644): Change `assert!(m.enabled)` → `assert!(!m.enabled)`

No other tests assert on `enabled`.

## Files Modified

| File | Change |
|------|--------|
| `backend/src/main.rs` | Remove auto-populate loop (keep registry cache load) |
| `backend/src/web_ui/model_registry.rs` | `enabled: true` → `enabled: false` in 4 places + 1 test fix |

## Agent Assignment

- **Wave 1:** `rust-backend-engineer` — all changes (single agent, backend-only)
- **Wave 2:** `backend-qa` — run `cargo clippy --all-targets && cargo test --lib`

Team preset: `backend-feature`
Worktree: yes, dedicated worktree with `isolation: "worktree"`

## Verification

1. `cd backend && cargo clippy --all-targets` — zero warnings
2. `cd backend && cargo test --lib` — zero failures
3. Manual: start fresh (empty DB) → model registry should be empty → admin clicks "Populate" → models appear with `enabled: false` → admin toggles individual models on
