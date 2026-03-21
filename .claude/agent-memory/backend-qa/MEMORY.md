# Backend QA Memory

## Test Count Baseline
- As of 2026-03-21 (post-Qwen removal): 817 tests pass (`cargo test --lib`, 11.08s)
- Previous: 931 (2026-03-21 pre-removal), 779 (2026-03-12), 747 (2026-03-08)
- ~114 tests removed with Qwen provider deletion (qwen.rs ~40, qwen_auth.rs ~38, registry ~28, others ~8)
- 1 integration test file: `backend/tests/integration_test.rs` (requires `test-utils` feature, 19 tests)
- Bench module: `backend/src/bench/` (runner, metrics, mock_server, report)

## Gotchas
- f32 temperature values lose precision through serde_json (0.7 becomes 0.699999988079071). Use `as_f64()` + epsilon comparison, not `assert_eq!` against float literals.
- `AnthropicMessagesRequest` has 11 fields — all must be specified (no Default impl). Use `None` for optionals.
- Rate limiter key uses `&token[..min(len, 16)]` — tokens sharing a 16-char prefix share a bucket.
- Pre-existing clippy warnings (13) in main codebase — all `result_large_err` on ApiError. Not from test code.

## File Locations
- `backend/src/providers/qwen.rs` — DELETED (Qwen removal 2026-03-21)
- `backend/src/web_ui/qwen_auth.rs` — DELETED (Qwen removal 2026-03-21)
- `backend/src/providers/registry.rs` — ProviderRegistry tests (Qwen tests removed)
- `backend/src/providers/types.rs` — ProviderId enum (now 5 variants: Kiro, Anthropic, OpenAICodex, Copilot, Custom)
- `backend/src/web_ui/provider_priority.rs` — VALID_PROVIDERS + tests
