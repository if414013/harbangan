# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Structure

```
rkgw/
├── backend/                    # Rust API server (Axum 0.7 + Tokio)
├── frontend/                   # React 19 SPA (Vite 7 + TypeScript 5.9), served by jonasal/nginx-certbot
├── docker-compose.yml          # 3 services: db, backend, frontend (nginx + auto-TLS)
├── docker-compose.gateway.yml  # Proxy-only: single backend container, no DB/SSO
└── .env.example
```

Runs via docker-compose. Two modes: full deployment (`docker-compose.yml`) or proxy-only (`docker-compose.gateway.yml`).

## Build & Dev Commands

### Backend

```bash
cd backend && cargo build                        # Debug build
cd backend && cargo build --release              # Release build
cd backend && cargo clippy                       # Lint — fix ALL warnings before committing
cd backend && cargo fmt                          # Format
cd backend && cargo test --lib                   # Unit tests (395 tests)
cd backend && cargo test --lib <test_name>       # Single test
cd backend && cargo test --lib <module>::        # All tests in a module
cd backend && cargo test --lib -- --nocapture    # Show println! output
cd backend && cargo test --features test-utils   # Integration tests
```

### Frontend

```bash
cd frontend && npm run build    # tsc -b && vite build
cd frontend && npm run lint     # eslint
cd frontend && npm run dev      # dev server (port 5173, proxies /_ui/api → localhost:8000)
```

### Docker

```bash
docker compose build    # Build all images
docker compose up -d    # Start all services
```

## Environment Variables

Set in `.env` (see `.env.example`):

| Variable | Required | Description |
|----------|----------|-------------|
| `DOMAIN` | Yes | Domain for Let's Encrypt TLS certs |
| `EMAIL` | Yes | Let's Encrypt notification email |
| `POSTGRES_PASSWORD` | Yes | PostgreSQL password |
| `GOOGLE_CLIENT_ID` | Yes | Google OAuth Client ID |
| `GOOGLE_CLIENT_SECRET` | Yes | Google OAuth Client Secret |
| `GOOGLE_CALLBACK_URL` | Yes | OAuth callback (e.g. `https://$DOMAIN/_ui/api/auth/google/callback`) |

Auto-set by docker-compose: `DATABASE_URL`, `SERVER_HOST` (0.0.0.0), `SERVER_PORT` (8000).

All runtime config (region, timeouts, debug mode, etc.) is managed via the Web UI at `/_ui/` and persisted in PostgreSQL. This includes `mcp_enabled` and `guardrails_enabled` (both default to `false`).

## Architecture

### Docker Services

```
Internet → nginx-certbot (frontend, :443/:80)
              ├── /_ui/*           → React SPA static files
              ├── /_ui/api/*       → proxy → backend:8000
              ├── /v1/*            → proxy → backend:8000 (SSE streaming)
              └── TLS auto-provisioned by jonasal/nginx-certbot
           backend   → Rust API server (plain HTTP, internal only)
           db        → PostgreSQL 16
```

### Backend Request Flow

```
Client (OpenAI or Anthropic format)
  → nginx (TLS termination)
  → middleware/ (CORS, API key auth → per-user Kiro creds)
  → routes/mod.rs (validate request, resolve model)
  → guardrails/ input check (if enabled, CEL rule matching + Bedrock API)
  → converters/ (OpenAI/Anthropic → Kiro format)
  → auth/ (get per-user Kiro access token, auto-refresh)
  → http_client.rs (POST to Kiro API)
  → streaming/mod.rs (parse AWS Event Stream)
  → thinking_parser.rs (extract reasoning blocks)
  → guardrails/ output check (if enabled, non-streaming only)
  → converters/ (Kiro → OpenAI/Anthropic format)
  → SSE response back to client
```

### Authentication

Two separate auth systems:

1. **API key auth** (for `/v1/*` proxy endpoints): Clients send `Authorization: Bearer <api-key>` or `x-api-key` header. Middleware SHA-256 hashes the key, looks up user in cache/DB, injects per-user Kiro credentials into the request.

2. **Google SSO** (for `/_ui/api/*` web UI): PKCE + OpenID Connect flow. Session cookie `kgw_session` (24h TTL), CSRF token in separate cookie. Admin vs User roles.

### Setup-Only Mode

On first run (no admin user in DB), gateway blocks `/v1/*` with 503 and only serves the web UI so the first user can complete setup via Google SSO (first user gets admin role).

### AppState

Defined in `backend/src/routes/mod.rs`:
- `config: Arc<RwLock<Config>>` — env vars + DB overlay
- `auth_manager: Arc<tokio::sync::RwLock<AuthManager>>` — Kiro token management
- `http_client: Arc<KiroHttpClient>` — connection-pooled HTTP client
- `model_cache: ModelCache` — cached model list from Kiro API
- `resolver: ModelResolver` — model name alias resolution
- `metrics: Arc<MetricsCollector>` — request latency/token tracking
- `log_buffer: Arc<Mutex<VecDeque<LogEntry>>>` — captured logs for SSE streaming
- `config_db: Option<Arc<ConfigDb>>` — PostgreSQL persistence
- `setup_complete: Arc<AtomicBool>` — setup wizard state
- `session_cache: Arc<DashMap<Uuid, SessionInfo>>` — in-memory session cache
- `api_key_cache: Arc<DashMap<String, (Uuid, Uuid)>>` — API key hash → (user_id, key_id)
- `kiro_token_cache: Arc<DashMap<Uuid, (String, String, Instant)>>` — per-user Kiro tokens (4-min TTL)
- `oauth_pending: Arc<DashMap<String, OAuthPendingState>>` — PKCE state (10-min TTL, 10k cap)
- `guardrails_engine: Option<Arc<GuardrailsEngine>>` — Content validation engine (CEL rules + Bedrock API)
- `mcp_manager: Option<Arc<McpManager>>` — MCP Gateway orchestrator (client connections, tool discovery, execution)

### Key Modules (backend/src/)

- `converters/` — Bidirectional format translation. One file per direction (e.g. `openai_to_kiro.rs`). Shared logic in `core.rs`.
- `auth/` — Kiro authentication via refresh tokens in PostgreSQL, auto-refreshes before expiry.
- `streaming/mod.rs` — Parses Kiro's AWS Event Stream binary format into `KiroEvent` variants.
- `models/` — Request/response types for OpenAI, Anthropic, and Kiro formats.
- `web_ui/` — Web UI API handlers. Google SSO (`google_auth.rs`), session management (`session.rs`), per-user API keys (`api_keys.rs`), per-user Kiro tokens (`user_kiro.rs`), config persistence (`config_db.rs`).
- `middleware/` — CORS, API key auth (SHA-256 + cache/DB lookup), debug logging.
- `guardrails/` — Content safety via AWS Bedrock guardrails (CEL rule engine + Bedrock API). Input/output validation with configurable rules stored in PostgreSQL.
- `mcp/` — MCP Gateway. Manages external tool servers over HTTP/SSE/STDIO transports. Includes client lifecycle (`client_manager.rs`), health monitoring, tool discovery/sync, and DB persistence.
- `metrics/` — Request latency and token usage tracking (`MetricsCollector`).
- `resolver.rs` — Maps model aliases to canonical Kiro model IDs. Don't hardcode model IDs.
- `tokenizer.rs` — Token counting via tiktoken (cl100k_base) with Claude correction factor (1.15x).
- `truncation.rs` — Detects truncated API responses and triggers recovery retries.
- `cache.rs` — `ModelCache` with TTL-based model metadata caching.
- `log_capture.rs` — Tracing capture layer for web UI SSE log streaming.

### API Endpoints

**Proxy (auth via API key):**
- `POST /v1/chat/completions` — OpenAI-compatible
- `POST /v1/messages` — Anthropic-compatible
- `GET /v1/models` — List models
- `POST /v1/mcp/tool/execute` — Execute MCP tool

**MCP Server Protocol (auth via API key):**
- `POST /mcp` — JSON-RPC 2.0 MCP server protocol
- `GET /mcp` — MCP SSE stream

**Infrastructure:**
- `GET /health` — Health check
- `GET /` — Status JSON

**Web UI API (`/_ui/api/*`, auth via session cookie):**
- Public: `/status`, `/auth/google`, `/auth/google/callback`
- Session: `/metrics`, `/system`, `/models`, `/logs`, `/config`, `/config/schema`, `/config/history`, `/auth/me`, `/stream/metrics` (SSE), `/stream/logs` (SSE)
- Mutations (+ CSRF): `/auth/logout`, Kiro token routes, API key routes
- Admin-only (+ CSRF): `PUT /config`, domain allowlist routes, user management routes
- Admin-only: MCP client CRUD routes (`/_ui/api/admin/mcp/clients/*`)
- Admin-only: Guardrails profile/rule CRUD routes (`/_ui/api/guardrails/*`), CEL validation, profile testing

## Code Style

### Imports

Group: `std` → external crates (alphabetical) → `crate::` modules, separated by blank lines.

### Error Handling

- `thiserror` for error enums in `error.rs`
- `anyhow::Result` with `.context()` for propagation
- `ApiError` implements `IntoResponse` for HTTP error mapping

### Logging

`tracing` macros with structured fields:
```rust
debug!(model = %model_id, "Processing request");
info!(tokens = count, "Request completed");
error!(error = ?err, "Failed to process");
```

### Testing

- Unit tests in `#[cfg(test)] mod tests` at bottom of each file
- Names: `test_<function>_<scenario>`
- Async: `#[tokio::test]`
- Helper configs: `create_test_config()` / `Config::with_defaults()`
- Feature-gated: `#[cfg(any(test, feature = "test-utils"))]`
