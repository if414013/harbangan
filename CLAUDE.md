# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Structure

```
rkgw/
├── backend/          # Rust API server (Axum)
├── frontend/         # React SPA (Vite + TypeScript), served by nginx
├── docker-compose.yml
├── init-certs.sh     # First-time Let's Encrypt cert provisioning
└── .env / .env.example
```

The gateway runs exclusively via docker-compose. There is no standalone CLI binary.

## Build & Development Commands

### Backend (Rust)

```bash
cd backend && cargo build                  # Debug build
cd backend && cargo build --release        # Release build
cd backend && cargo clippy                 # Lint (fix all warnings before committing)
cd backend && cargo fmt                    # Format code
cd backend && cargo fmt -- --check         # Check formatting only
cd backend && cargo test --lib             # All unit tests
cd backend && cargo test --features test-utils  # Integration tests
```

### Frontend (React)

```bash
cd frontend && npm run build    # tsc -b && vite build
cd frontend && npm run lint     # eslint
cd frontend && npm run dev      # vite dev server (port 5173, proxies /_ui/api → localhost:8000)
```

### Docker

```bash
docker compose build            # Build all images
docker compose up -d            # Start all services
docker compose logs -f          # Follow logs
```

## Required Environment Variables

Set in `.env` (see `.env.example`):
- `DOMAIN` - Domain for TLS certs (e.g. `gateway.example.com`)
- `EMAIL` - Email for Let's Encrypt notifications
- `POSTGRES_PASSWORD` - PostgreSQL password
- `GOOGLE_CLIENT_ID` - Google OAuth Client ID (required)
- `GOOGLE_CLIENT_SECRET` - Google OAuth Client Secret (required)
- `GOOGLE_CALLBACK_URL` - Google OAuth callback URL (required)

Backend-only env vars (set automatically by docker-compose):
- `DATABASE_URL` - PostgreSQL connection string
- `SERVER_HOST` / `SERVER_PORT` - Bind address and port (defaults: `0.0.0.0:8000`)

## Architecture

Rust proxy gateway exposing OpenAI and Anthropic-compatible APIs, translating to the Kiro API (AWS CodeWhisperer) backend. Built with Axum 0.7 + Tokio.

### Docker Services

```
Internet → nginx (frontend, ports 443/80)
              ├── /_ui/*         → serve React SPA
              ├── /_ui/api/*     → proxy → backend:8000
              ├── /v1/*          → proxy → backend:8000
              └── /.well-known/  → certbot webroot
           certbot → Let's Encrypt cert renewal
           backend → Rust API server (HTTP, internal only)
           db → PostgreSQL
```

### Request Flow (Backend)

```
Client (OpenAI or Anthropic format)
  → nginx (TLS termination, static files)
  → backend/ middleware (CORS, auth, debug logging)
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

Defined in `backend/src/routes/mod.rs`. All handlers receive this via Axum's state extraction:
- `config: Arc<RwLock<Config>>` - loaded from env vars + DB overlay
- `auth_manager: Arc<AuthManager>` - token management with auto-refresh
- `http_client: Arc<KiroHttpClient>` - connection-pooled HTTP client
- `model_cache: Arc<RwLock<ModelCache>>` - cached model list from Kiro API
- `model_resolver: Arc<ModelResolver>` - normalizes model name aliases
- `metrics: Arc<MetricsCollector>` - request latency/token tracking
- `log_buffer: Arc<Mutex<VecDeque<LogEntry>>>` - recent logs for web UI SSE streaming
- `config_db: Option<Arc<ConfigDb>>` - PostgreSQL config persistence

### Key Modules (backend/src/)

- `converters/` - Bidirectional format translation. Each direction is a separate file (e.g. `openai_to_kiro.rs`). Shared logic lives in `core.rs`.
- `auth/` - Manages Kiro authentication using refresh tokens stored in PostgreSQL, auto-refreshes before expiry.
- `streaming/mod.rs` - Parses Kiro's AWS Event Stream binary format into `KiroEvent` variants, then formats as SSE.
- `models/` - Request/response types for OpenAI (`openai.rs`), Anthropic (`anthropic.rs`), and Kiro (`kiro.rs`) formats.
- `truncation.rs` - Detects truncated API responses and triggers recovery retries.
- `log_capture.rs` - Log entry struct + tracing capture layer for web UI SSE log streaming.
- `web_ui/` - Web UI API handlers at `/_ui/api/`. Google SSO, session management, config persistence.
- `resolver.rs` - Maps model name aliases to canonical Kiro model IDs. Don't hardcode model IDs.

### API Endpoints

- `POST /v1/chat/completions` - OpenAI-compatible chat
- `POST /v1/messages` - Anthropic-compatible messages
- `GET /v1/models` - List available models
- `GET /health` - Health check
- `/_ui` - Web dashboard (served by nginx)
- `/_ui/api/*` - Web UI API (proxied to backend by nginx)

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
