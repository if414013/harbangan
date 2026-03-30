# Backend QA Memory

## Test Count Baseline
- As of 2026-03-31: 901 unit tests pass (`cargo test --lib`, ~11s), 27 integration tests pass (`cargo test --features test-utils`, ~6s)
- Previous: 820 (2026-03-22), 817 (2026-03-21 post-Qwen removal)
- Previous: 931 (2026-03-21 pre-removal), 779 (2026-03-12), 747 (2026-03-08)
- ~114 tests removed with Qwen provider deletion (qwen.rs ~40, qwen_auth.rs ~38, registry ~28, others ~8)
- +81 unit tests from provider toggle feature (credential-gate, cache filtering, pipeline validation, error variants)
- +8 integration tests from provider toggle feature (model list filtering, admin endpoint auth/validation)
- Bench module: `backend/src/bench/` (runner, metrics, mock_server, report)

## Gotchas
- f32 temperature values lose precision through serde_json (0.7 becomes 0.699999988079071). Use `as_f64()` + epsilon comparison, not `assert_eq!` against float literals.
- `AnthropicMessagesRequest` has 11 fields — all must be specified (no Default impl). Use `None` for optionals.
- Rate limiter key uses `&token[..min(len, 16)]` — tokens sharing a 16-char prefix share a bucket.
- Pre-existing clippy warnings (13) in main codebase — all `result_large_err` on ApiError. Not from test code.
- `insert_registry_model()` on ModelCache is `#[cfg(any(test, feature = "test-utils"))]` — needed for integration tests.
- Integration tests need `--features test-utils` to access test helpers like `insert_registry_model()`.
- Admin toggle endpoint tests require session cache pre-population + CSRF cookie/header pair. Use `setup_admin_session()` / `setup_user_session()` helpers in integration_test.rs.

## Provider Toggle Feature (2026-03-31)
- Credential gate design: disabling a provider blocks its credentials, NOT its model names
- Disable Anthropic → claude-* still routes via Copilot if enabled
- `validate_provider_enabled()` only fires for explicit-prefix models (e.g. `anthropic/claude-sonnet-4`)
- Unprefixed models fall through naturally via `load_user_data()` skipping disabled providers
- `pick_best_provider()` signature unchanged — filtering happens upstream
- Kiro always enabled, cannot be disabled (short-circuit in `is_provider_enabled`)
- `ApiError::ProviderDisabled` returns HTTP 403

## File Locations
- `backend/src/providers/qwen.rs` — DELETED (Qwen removal 2026-03-21)
- `backend/src/web_ui/qwen_auth.rs` — DELETED (Qwen removal 2026-03-21)
- `backend/src/providers/registry.rs` — ProviderRegistry tests (credential-gate + enabled/disabled state)
- `backend/src/providers/types.rs` — ProviderId enum (4 variants: Kiro, Anthropic, OpenAICodex, Copilot)
- `backend/src/web_ui/provider_priority.rs` — VALID_PROVIDERS + tests
- `backend/src/cache.rs` — ModelCache registry filtering tests
- `backend/src/routes/pipeline.rs` — validate_provider_enabled tests
- `backend/src/error.rs` — ProviderDisabled error variant + tests
- `backend/tests/integration_test.rs` — 27 integration tests (model filtering + admin toggle)
