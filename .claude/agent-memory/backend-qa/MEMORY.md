# Backend QA Memory

## Test Count Baseline
- As of 2026-03-21: 931 tests pass (`cargo test --lib`, 15.81s)
- Previous: 779 tests (2026-03-12), 747 (2026-03-08)
- 63 files with `#[cfg(test)]` blocks, 63 `mod tests` blocks
- ~803 `#[test]` + ~133 `#[tokio::test]` annotations
- 1 integration test file: `backend/tests/integration_test.rs` (requires `test-utils` feature)
- Bench module added: `backend/src/bench/` (runner, metrics, mock_server, report)

## Gotchas
- f32 temperature values lose precision through serde_json (0.7 becomes 0.699999988079071). Use `as_f64()` + epsilon comparison, not `assert_eq!` against float literals.
- `AnthropicMessagesRequest` has 11 fields — all must be specified (no Default impl). Use `None` for optionals.
- Rate limiter key uses `&token[..min(len, 16)]` — tokens sharing a 16-char prefix share a bucket.
- Pre-existing clippy warnings (10) in main codebase — not from test code.

## File Locations
- `backend/src/providers/qwen.rs` — QwenProvider + 40 tests
- `backend/src/providers/registry.rs` — ProviderRegistry + 50 tests
- `backend/src/providers/types.rs` — ProviderId enum + 22 tests
- `backend/src/web_ui/qwen_auth.rs` — Device flow + 38 tests
- `backend/src/web_ui/provider_priority.rs` — VALID_PROVIDERS + 12 tests
