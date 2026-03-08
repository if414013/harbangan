# Backend Rules

Applies to files in `backend/`.

## Stack

- Rust (edition 2021) + Axum 0.7 + Tokio
- sqlx 0.8 (PostgreSQL, compile-time checked queries)
- tracing for structured logging
- thiserror + anyhow for error handling

## Build & Check

```bash
cd backend && cargo build              # debug build
cd backend && cargo clippy             # lint — fix ALL warnings before committing
cd backend && cargo fmt                # format
cd backend && cargo test --lib         # unit tests
```

## Import Ordering

Group imports with blank lines between groups:
1. `std` — standard library
2. External crates — alphabetical order
3. `crate::` — internal modules

## Error Handling

- Define error enums with `thiserror` in `error.rs` per module
- Use `anyhow::Result` with `.context()` for error propagation
- `ApiError` implements `IntoResponse` for HTTP error mapping
- Never use `.unwrap()` in production code — use `.context("reason")?`

## Logging

Use `tracing` macros with structured fields:
```rust
debug!(model = %model_id, "Processing request");
info!(tokens = count, "Request completed");
error!(error = ?err, "Failed to process");
```
- Use `%` for Display, `?` for Debug formatting
- Include relevant context fields, not just messages

## Testing

- Unit tests in `#[cfg(test)] mod tests` at the bottom of each file
- Naming: `test_<function>_<scenario>`
- Async tests: `#[tokio::test]`
- Helper configs: `create_test_config()` / `Config::with_defaults()`
- Feature-gated: `#[cfg(any(test, feature = "test-utils"))]`
- Run `cargo test --lib` before committing

## Model Resolution

- Never hardcode model IDs — always use `resolver.rs` for model name mapping
- Model metadata cached via `ModelCache` with TTL

## Key Patterns

- `Arc<RwLock<T>>` for shared mutable state in AppState
- `DashMap` for concurrent caches (API keys, sessions, tokens)
- Streaming via `async-stream` crate — watch for borrow checker pitfalls
- Serde: use `#[serde(rename_all = "snake_case")]` consistently; check `#[serde(rename = "...")]` on individual fields
