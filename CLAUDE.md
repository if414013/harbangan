# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

```bash
cargo build                        # Debug build
cargo build --release              # Release build
cargo run --bin kiro-gateway --release  # Run the gateway
cargo clippy                       # Lint (fix all warnings before committing)
cargo fmt                          # Format code
cargo fmt -- --check               # Check formatting only
```

## Testing

```bash
cargo test --lib                    # All unit tests
cargo test --lib <test_name>        # Single test by name
cargo test --lib <module>::         # All tests in a module
cargo test --lib -- --nocapture     # Show println! output
cargo test --features test-utils    # Integration tests
```

## Required Environment Variables

Set in `.env` or export:
- `PROXY_API_KEY` - Password to protect the proxy server (configured via Web UI setup)
- `DATABASE_URL` - PostgreSQL connection string (e.g. `postgres://user:pass@localhost:5432/kiro_gateway`)
- `KIRO_REGION` - AWS region (default: `us-east-1`)

## Architecture

This is a Rust proxy gateway that exposes OpenAI and Anthropic-compatible APIs, translating requests to the Kiro API (AWS CodeWhisperer) backend. Built with Axum 0.7 + Tokio.

### Request Flow

```
Client (OpenAI or Anthropic format)
  → middleware/ (CORS, auth, debug logging)
  → routes/mod.rs (validate request, resolve model)
  → converters/ (OpenAI/Anthropic → Kiro format)
  → auth/ (get/refresh Kiro token via refresh token)
  → http_client.rs (POST to Kiro API)
  → streaming/mod.rs (parse AWS Event Stream)
  → thinking_parser.rs (extract reasoning blocks)
  → converters/ (Kiro → OpenAI/Anthropic format)
  → SSE response back to client
```

### Shared State (AppState)

Defined in `routes/mod.rs`. All handlers receive this via Axum's state extraction:
- `config: Config` - loaded from CLI args + env vars
- `auth_manager: Arc<AuthManager>` - token management with auto-refresh
- `http_client: Arc<KiroHttpClient>` - connection-pooled HTTP client
- `model_cache: Arc<RwLock<ModelCache>>` - cached model list from Kiro API
- `model_resolver: Arc<ModelResolver>` - normalizes model name aliases
- `metrics: Arc<MetricsCollector>` - request latency/token tracking
- `log_buffer: Arc<RwLock<Vec<LogEntry>>>` - recent logs for dashboard
- `config_db: Option<Arc<ConfigDb>>` - PostgreSQL config persistence (when web UI enabled)

### Key Modules

- `converters/` - Bidirectional format translation. Each direction is a separate file (e.g. `openai_to_kiro.rs`). Shared logic lives in `core.rs`.
- `auth/` - Manages Kiro authentication using refresh tokens stored in PostgreSQL, auto-refreshes before expiry.
- `streaming/mod.rs` - Parses Kiro's AWS Event Stream binary format into `KiroEvent` variants, then formats as SSE.
- `models/` - Request/response types for OpenAI (`openai.rs`), Anthropic (`anthropic.rs`), and Kiro (`kiro.rs`) formats.
- `truncation.rs` - Detects truncated API responses and triggers recovery retries.
- `dashboard/` - Optional TUI (ratatui) for real-time monitoring. Enabled with `--dashboard` flag.
- `web_ui/` - Web dashboard served at `/_ui/`. Has its own routes, templates, and PostgreSQL config persistence.
- `resolver.rs` - Maps model name aliases to canonical Kiro model IDs. Don't hardcode model IDs.

### API Endpoints

- `POST /v1/chat/completions` - OpenAI-compatible chat
- `POST /v1/messages` - Anthropic-compatible messages
- `GET /v1/models` - List available models
- `GET /health` - Health check
- `/_ui` - Web dashboard (when enabled)

## Code Style

### Imports

Group in order, separated by blank lines: `std` → external crates (alphabetical) → `crate::` modules.

### Error Handling

- `thiserror` for defining error enums in `error.rs`
- `anyhow::Result` with `.context()` for propagation
- `ApiError` implements `IntoResponse` for HTTP error mapping

### Logging

Use `tracing` macros with structured fields:
```rust
debug!(model = %model_id, "Processing request");
info!(tokens = count, "Request completed");
error!(error = ?err, "Failed to process");
```

### Testing Conventions

- Unit tests go in `#[cfg(test)] mod tests` at the bottom of each file
- Test names: `test_<function>_<scenario>`
- Use `#[tokio::test]` for async tests
- Helper configs: use `create_test_config()` pattern
- Feature-gated test utilities: `#[cfg(any(test, feature = "test-utils"))]`
