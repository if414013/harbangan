# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
cargo build              # Debug build
cargo build --release    # Release build (optimized)
cargo run --release      # Build and run
cargo test --lib         # Run unit tests only (recommended)
cargo test --lib <name>  # Run specific unit test
cargo clippy             # Lint
```

## Required Environment Variables

Set these in `.env` or export them:
- `PROXY_API_KEY` - Password to protect the proxy server (required)
- `KIRO_CLI_DB_FILE` - Path to kiro-cli SQLite database, e.g. `~/.kiro/data.db` (required)
- `KIRO_REGION` - AWS region (default: `us-east-1`)

## Optional Environment Variables

- `SERVER_HOST` - Server bind address (default: `127.0.0.1`). Use `0.0.0.0` for network access (requires TLS).
- `SERVER_PORT` - Server port (default: `8000`)
- `TLS_ENABLED` - Enable HTTPS (`true`/`false`, default: `false`). Auto-generates a self-signed cert if `TLS_CERT`/`TLS_KEY` are not set.
- `TLS_CERT` - Path to TLS certificate file (PEM format)
- `TLS_KEY` - Path to TLS private key file (PEM format)

## Architecture

This is a Rust proxy gateway that translates OpenAI and Anthropic API formats to the Kiro/CodeWhisperer API format, enabling Claude models to be used through standard API interfaces.

### Request Flow

```
Client Request (OpenAI/Anthropic format)
    ↓
routes/mod.rs (endpoint handlers)
    ↓
converters/ (format translation)
    ├── openai_to_kiro.rs
    └── anthropic_to_kiro.rs
    ↓
http_client.rs → Kiro API (codewhisperer.{region}.amazonaws.com)
    ↓
streaming/mod.rs (parse AWS Event Stream)
    ↓
converters/
    ├── kiro_to_openai.rs
    └── kiro_to_anthropic.rs
    ↓
Client Response (OpenAI/Anthropic format)
```

### Key Modules

- **routes/** - Axum HTTP handlers for `/v1/chat/completions` (OpenAI) and `/v1/messages` (Anthropic)
- **converters/** - Bidirectional format conversion between OpenAI/Anthropic and Kiro formats
- **streaming/** - Parses Kiro's AWS Event Stream format and converts to SSE for both API formats
- **auth/** - Token management with automatic refresh from kiro-cli SQLite database
- **resolver.rs** - Model name normalization (e.g., `claude-sonnet-4-5` → internal Kiro model ID)
- **cache.rs** - In-memory model cache populated at startup from Kiro API
- **thinking_parser.rs** - Extracts `<thinking>` blocks for extended thinking/reasoning support
- **tls.rs** - TLS certificate loading, self-signed certificate generation, and expiry management

### Authentication Flow

The gateway reads credentials from the kiro-cli SQLite database (`KIRO_CLI_DB_FILE`). The `AuthManager` handles:
1. Loading tokens from SQLite
2. Automatic token refresh before expiration
3. Providing access tokens for Kiro API requests

### Streaming Architecture

Kiro API always returns AWS Event Stream format. The `streaming` module:
1. Parses binary AWS Event Stream chunks
2. Extracts `assistantResponseEvent` payloads
3. Handles `<thinking>` tag parsing for reasoning content
4. Converts to OpenAI SSE (`data: {...}`) or Anthropic SSE (`event: ... data: ...`) format

### TLS

The gateway supports HTTPS via `axum-server` with `rustls`. When `--tls` is passed (or `TLS_ENABLED=true`):
1. If `--tls-cert` and `--tls-key` are provided, loads the custom PEM files
2. Otherwise, auto-generates a self-signed certificate (saved to `~/.kiro-gateway/tls/` for reuse)
3. Certificates are automatically regenerated 30 days before expiry

Both HTTP and HTTPS paths use `axum_server::Handle` for unified graceful shutdown.

## Testing

Unit tests are co-located in each module under `#[cfg(test)]`. Run with `cargo test --lib`.
