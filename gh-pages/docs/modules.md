---
layout: default
title: Modules
nav_order: 7
---

# Module Reference
{: .no_toc }

Overview of all source modules in the Kiro Gateway codebase, their responsibilities, and how they interconnect.
{: .fs-6 .fw-300 }

<details open markdown="block">
  <summary>Table of contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Module Relationship Diagram

```mermaid
graph TD
    main["main.rs<br/><i>Entry point</i>"]
    config["config.rs<br/><i>ENV + DB config</i>"]
    routes["routes/mod.rs<br/><i>API handlers + AppState</i>"]
    middleware["middleware/<br/><i>Auth, CORS, Debug</i>"]
    converters["converters/<br/><i>Format translation</i>"]
    models["models/<br/><i>Request/response types</i>"]
    auth["auth/<br/><i>Token management</i>"]
    streaming["streaming/mod.rs<br/><i>AWS Event Stream parser</i>"]
    http_client["http_client.rs<br/><i>Connection pool + retry</i>"]
    resolver["resolver.rs<br/><i>Model name aliases</i>"]
    cache["cache.rs<br/><i>Model list cache</i>"]
    metrics["metrics/<br/><i>Request tracking</i>"]
    web_ui["web_ui/<br/><i>Google SSO, API keys,<br/>sessions, config API</i>"]
    log_capture["log_capture.rs<br/><i>Tracing → SSE logs</i>"]
    truncation["truncation.rs<br/><i>Truncation recovery</i>"]
    thinking["thinking_parser.rs<br/><i>Reasoning extraction</i>"]
    tokenizer["tokenizer.rs<br/><i>Token counting</i>"]
    error["error.rs<br/><i>Error types</i>"]
    bench["bench/<br/><i>Benchmarking</i>"]
    guardrails["guardrails/<br/><i>CEL rules + Bedrock<br/>content validation</i>"]
    mcp["mcp/<br/><i>MCP Gateway: tool servers,<br/>JSON-RPC, transports</i>"]

    main --> config
    main --> routes
    main --> web_ui
    main --> log_capture
    main --> guardrails
    main --> mcp

    routes --> middleware
    routes --> converters
    routes --> auth
    routes --> http_client
    routes --> streaming
    routes --> resolver
    routes --> metrics
    routes --> truncation
    routes --> tokenizer
    routes --> guardrails
    routes --> mcp

    converters --> models
    converters --> thinking

    streaming --> models
    streaming --> thinking

    auth --> web_ui

    resolver --> cache

    web_ui --> metrics
    web_ui --> auth

    http_client --> auth
    http_client --> error

    mcp --> web_ui
    guardrails --> web_ui

    style main fill:#4a9eff,color:#fff
    style routes fill:#ff6b6b,color:#fff
    style converters fill:#51cf66,color:#fff
    style streaming fill:#51cf66,color:#fff
    style auth fill:#ffd43b,color:#333
    style web_ui fill:#cc5de8,color:#fff
    style models fill:#868e96,color:#fff
    style log_capture fill:#ff922b,color:#fff
    style guardrails fill:#e64980,color:#fff
    style mcp fill:#20c997,color:#fff
```

---

## Module Index

### Core Application

| Module | File(s) | Description |
|--------|---------|-------------|
| `main` | `backend/src/main.rs` | Application entry point. Loads config from environment + PostgreSQL, initializes all subsystems (auth, HTTP client, model cache, metrics, log capture), builds the Axum router, and starts the HTTP server on port 8000. |
| `lib` | `backend/src/lib.rs` | Library root. Re-exports all public modules for use by integration tests. |
| `config` | `backend/src/config.rs` | Configuration management. Defines `Config` struct with all runtime settings, `DebugMode` and `FakeReasoningHandling` enums. Loads from environment variables + `.env` file, with PostgreSQL overlay for runtime config changes via the Web UI. |
| `error` | `backend/src/error.rs` | Error types. Defines `ApiError` enum (`AuthError`, `InvalidModel`, `KiroApiError`, `ConfigError`, `ValidationError`, `Internal`) with `IntoResponse` implementation that maps each variant to an HTTP status code and JSON error body. |

### Request Handling

| Module | File(s) | Description |
|--------|---------|-------------|
| `routes` | `backend/src/routes/mod.rs` | API route handlers and `AppState` definition. Contains handlers for `GET /`, `GET /health`, `GET /v1/models`, `POST /v1/chat/completions`, and `POST /v1/messages`. Manages request lifecycle with `RequestGuard` for metrics tracking. Builds separate router groups for health (unauthenticated), OpenAI (authenticated), and Anthropic (authenticated) routes. |
| `middleware` | `backend/src/middleware/mod.rs`, `backend/src/middleware/debug.rs` | Authentication, CORS, and debug logging middleware. `auth_middleware` validates API keys by SHA-256 hashing and looking up in cache/DB to identify the user and inject per-user Kiro credentials. `cors_layer()` creates a permissive CORS layer allowing all origins, methods, and headers. `debug.rs` provides request/response body logging controlled by `DebugMode`. |

### Format Translation

| Module | File(s) | Description |
|--------|---------|-------------|
| `converters` | `backend/src/converters/mod.rs` | Module root for bidirectional format translation between OpenAI/Anthropic and Kiro formats. |
| `converters::openai_to_kiro` | `backend/src/converters/openai_to_kiro.rs` | Converts OpenAI `ChatCompletionRequest` to Kiro's `KiroRequest` format. Maps messages, system prompts, tools, and conversation history. |
| `converters::anthropic_to_kiro` | `backend/src/converters/anthropic_to_kiro.rs` | Converts Anthropic `AnthropicMessagesRequest` to Kiro's `KiroRequest` format. Handles content blocks (text, images, tool use/results, thinking). |
| `converters::kiro_to_openai` | `backend/src/converters/kiro_to_openai.rs` | Converts Kiro streaming events back to OpenAI `ChatCompletionChunk` format. |
| `converters::kiro_to_anthropic` | `backend/src/converters/kiro_to_anthropic.rs` | Converts Kiro streaming events back to Anthropic `StreamEvent` format. |
| `converters::core` | `backend/src/converters/core.rs` | Shared conversion logic used by both OpenAI and Anthropic converters. Defines `UnifiedMessage`, `MessageContent`, `ToolCall`, `ToolResult`, and `UnifiedTool` types. |

### Data Models

| Module | File(s) | Description |
|--------|---------|-------------|
| `models::openai` | `backend/src/models/openai.rs` | OpenAI-compatible request/response types: `ChatCompletionRequest`, `ChatCompletionResponse`, `ChatCompletionChunk` (streaming), `ModelList`, `OpenAIModel`, `Tool`, `ToolCall`, and usage types. |
| `models::anthropic` | `backend/src/models/anthropic.rs` | Anthropic-compatible types: `AnthropicMessagesRequest`, `AnthropicMessagesResponse`, `ContentBlock` (text, thinking, image, tool_use, tool_result), `StreamEvent` variants (message_start, content_block_delta, etc.), and `Delta` types. |
| `models::kiro` | `backend/src/models/kiro.rs` | Kiro API (AWS CodeWhisperer) types: `KiroRequest` with builder methods (`with_system`, `with_tools`, `with_turns`, `with_images`), `KiroResponse`, `KiroStreamEvent`, turn-based conversation model, and tool configuration types. Uses `camelCase` serialization. |

### Authentication

| Module | File(s) | Description |
|--------|---------|-------------|
| `auth` | `backend/src/auth/mod.rs` | Module root for authentication subsystem. Re-exports `AuthManager` and related types. |
| `auth::manager` | `backend/src/auth/manager.rs` | `AuthManager` — manages per-user Kiro API authentication. Handles automatic token refresh before expiry via AWS SSO OIDC. Supports both testing mode (static token) and production mode (per-user refresh tokens from PostgreSQL). |
| `auth::oauth` | `backend/src/auth/oauth.rs` | OAuth device code flow implementation via AWS SSO OIDC. Functions for client registration (`register_client`), device authorization (`start_device_authorization`), PKCE generation, authorization URL building, code exchange, and device token polling. |
| `auth::credentials` | `backend/src/auth/credentials.rs` | Credential storage and retrieval helpers. Loads per-user Kiro credentials from PostgreSQL. |
| `auth::refresh` | `backend/src/auth/refresh.rs` | Token refresh logic. Handles automatic access token renewal using stored refresh tokens via AWS SSO OIDC. |
| `auth::types` | `backend/src/auth/types.rs` | Shared authentication types (`PollResult`, token response structures). |

### Streaming & Parsing

| Module | File(s) | Description |
|--------|---------|-------------|
| `streaming` | `backend/src/streaming/mod.rs` | Kiro AWS Event Stream parser. Parses the binary event stream format from the Kiro API into typed `KiroEvent` variants. Provides `stream_kiro_to_openai()`, `stream_kiro_to_anthropic()`, `collect_openai_response()`, and `collect_anthropic_response()` functions for both streaming and non-streaming response handling. |
| `thinking_parser` | `backend/src/thinking_parser.rs` | Extracts reasoning/thinking blocks from model responses. Parses `<thinking>` tags and converts them to structured content blocks for both OpenAI (`reasoning_content`) and Anthropic (`thinking` content block) formats. |
| `truncation` | `backend/src/truncation.rs` | Truncation detection and recovery. Injects recovery instructions into conversation context (`inject_openai_truncation_recovery`, `inject_anthropic_truncation_recovery`) and detects when responses are cut off mid-stream to trigger retries. |
| `tokenizer` | `backend/src/tokenizer.rs` | Approximate token counting for messages, tools, and system prompts. Provides `count_message_tokens()`, `count_anthropic_message_tokens()`, and `count_tools_tokens()` for input token estimation. |

### Infrastructure

| Module | File(s) | Description |
|--------|---------|-------------|
| `http_client` | `backend/src/http_client.rs` | Connection-pooled HTTP client (`KiroHttpClient`) for communicating with the Kiro API. Configurable connection pool size, connect/request timeouts, and automatic retry with exponential backoff. Provides `request_with_retry()` for resilient upstream calls. |
| `resolver` | `backend/src/resolver.rs` | Model name resolution. `ModelResolver` maps model name aliases (e.g. `claude-sonnet-4.5`) to canonical Kiro model IDs. Checks the model cache first, then falls back to pattern matching. |
| `cache` | `backend/src/cache.rs` | `ModelCache` — thread-safe cache for the model list fetched from the Kiro API at startup. Provides `get_all_model_ids()` and `update()` methods. Configurable TTL. |
| `log_capture` | `backend/src/log_capture.rs` | Custom `tracing` subscriber layer that captures log events into the shared log buffer. Enables real-time log streaming to the web UI via SSE (`/_ui/api/stream/logs`). |
| `utils` | `backend/src/utils.rs` | Miscellaneous utility functions shared across modules. |

### Monitoring & Metrics

| Module | File(s) | Description |
|--------|---------|-------------|
| `metrics` | `backend/src/metrics/mod.rs`, `backend/src/metrics/collector.rs` | `MetricsCollector` — lock-free metrics collection using atomic counters and ring buffers. Tracks active connections, total requests/errors, latency percentiles (p50/p95/p99), per-model statistics (request count, avg latency, input/output tokens), and error breakdown by type. `StreamingMetricsTracker` handles async output token counting for streaming requests. Provides `to_json_snapshot()` for the Web UI. Samples auto-expire after 15 minutes. |

### Web UI

| Module | File(s) | Description |
|--------|---------|-------------|
| `web_ui` | `backend/src/web_ui/mod.rs` | Module root. Builds the Web UI router with authenticated and public API routes. Includes `setup_guard` middleware that returns `503 Service Unavailable` on `/v1/*` endpoints when setup is incomplete. |
| `web_ui::routes` | `backend/src/web_ui/routes.rs` | Web UI HTTP handlers. Provides API endpoints for metrics, system info, models, logs (paginated with search), config CRUD, config schema, config history, and status. |
| `web_ui::google_auth` | `backend/src/web_ui/google_auth.rs` | Google SSO authentication with PKCE + OpenID Connect. Handles the OAuth authorization redirect, callback with code exchange, ID token verification, and user creation/lookup. |
| `web_ui::session` | `backend/src/web_ui/session.rs` | Session management. Creates and validates `kgw_session` cookies (24h TTL), manages CSRF tokens, and provides session middleware for web UI route protection. |
| `web_ui::api_keys` | `backend/src/web_ui/api_keys.rs` | Per-user API key management. CRUD endpoints for creating, listing, and revoking API keys. Keys are stored as SHA-256 hashes in PostgreSQL. |
| `web_ui::user_kiro` | `backend/src/web_ui/user_kiro.rs` | Per-user Kiro credential management. Endpoints for storing and updating each user's Kiro refresh token, client ID, client secret, and region. |
| `web_ui::users` | `backend/src/web_ui/users.rs` | User administration (admin-only). Endpoints for listing users, changing roles, and removing users. |
| `web_ui::config_api` | `backend/src/web_ui/config_api.rs` | Config field validation and metadata. `classify_config_change()` determines if a field change can be hot-reloaded or requires restart. `validate_config_field()` validates types and ranges. `get_config_field_descriptions()` provides human-readable descriptions for the config UI. |
| `web_ui::config_db` | `backend/src/web_ui/config_db.rs` | `ConfigDb` — PostgreSQL-backed configuration persistence using `sqlx`. Auto-creates `config`, `config_history`, and `schema_version` tables. Provides `get/set/get_all`, `load_into_config()` overlay, `save_initial_setup()`, `save_oauth_setup()`, and `get_history()` with automatic pruning (keeps last 1000 entries). All writes are transactional. |
| `web_ui::sse` | `backend/src/web_ui/sse.rs` | Server-Sent Event streams for the Web UI. `metrics_stream` pushes metrics snapshots every 1 second. `logs_stream` pushes new log entries every 500ms. Both include 15-second keep-alive pings. |

### Guardrails

| Module | File(s) | Description |
|--------|---------|-------------|
| `guardrails` | `backend/src/guardrails/mod.rs` | Module root. Re-exports `GuardrailsDb` and `GuardrailsEngine`. |
| `guardrails::engine` | `backend/src/guardrails/engine.rs` | `GuardrailsEngine` — orchestrates content validation. Loads rules from DB, evaluates CEL conditions, calls Bedrock API concurrently per profile, aggregates results. Supports sampling (0-100%). Fails open on engine errors. Methods: `validate_input()`, `validate_output()`, `reload()`. |
| `guardrails::cel` | `backend/src/guardrails/cel.rs` | `CelEvaluator` — compiles and evaluates CEL (Common Expression Language) expressions with caching. Available variables: `request.model`, `request.api_format`, `request.message_count`, `request.has_tools`, `request.is_streaming`, `request.content_length`. Empty expressions match all requests. |
| `guardrails::bedrock` | `backend/src/guardrails/bedrock.rs` | `BedrockGuardrailClient` — calls AWS Bedrock ApplyGuardrail API with SigV4 request signing. Supports content policy, topic policy, word policy, and PII detection violations. Returns `GuardrailAction::None`, `Intervened`, or `Redacted`. Includes region validation for SSRF protection. |
| `guardrails::db` | `backend/src/guardrails/db.rs` | `GuardrailsDb` — PostgreSQL layer for guardrail profiles and rules. Three tables: `guardrail_profiles`, `guardrail_rules`, `guardrail_rule_profiles` (junction). CRUD operations + `load_config()` for in-memory snapshot. |
| `guardrails::api` | `backend/src/guardrails/api.rs` | Admin API handlers for guardrail profiles and rules CRUD. Includes test endpoint for validating a profile against Bedrock and CEL expression validation endpoint. All admin-only with session + CSRF. |
| `guardrails::types` | `backend/src/guardrails/types.rs` | Type definitions: `GuardrailRule`, `GuardrailProfile`, `GuardrailAction` (None/Intervened/Redacted), `ApplyTo` (Input/Output/Both), `RequestContext`, `GuardrailViolation`, `GuardrailCheckResult`. |

### MCP Gateway

| Module | File(s) | Description |
|--------|---------|-------------|
| `mcp` | `backend/src/mcp/mod.rs` | `McpManager` — orchestrates MCP client connections, tool discovery, and execution. Manages background health monitors and tool syncers per client. Initialization loads clients from DB and auto-connects enabled clients. |
| `mcp::api` | `backend/src/mcp/api.rs` | Admin API handlers: CRUD for MCP clients, reconnect, list tools per client. Tool execution endpoint (`POST /v1/mcp/tool/execute`). All admin routes require session + CSRF. |
| `mcp::server` | `backend/src/mcp/server.rs` | JSON-RPC 2.0 server at `/mcp`. Implements `initialize`, `tools/list`, `tools/call`, `ping` methods. GET returns SSE stream for keep-alive. Aggregates tools from all connected clients with `{clientName}_{toolName}` namespacing. |
| `mcp::client_manager` | `backend/src/mcp/client_manager.rs` | `ClientManager` — handles transport creation, connection lifecycle, initialization handshake (MCP `initialize` + `notifications/initialized`), tool discovery, reconnection, and graceful shutdown. |
| `mcp::transport::http` | `backend/src/mcp/transport/http.rs` | HTTP transport — stateless POST-based JSON-RPC. Includes SSRF protection (blocks private IPs: 127/8, 10/8, 172.16/12, 192.168/16), URL validation. |
| `mcp::transport::sse` | `backend/src/mcp/transport/sse.rs` | SSE transport — persistent GET stream with POST-based request delivery. Receives `endpoint` event for POST URL, correlates responses via JSON-RPC IDs using oneshot channels. |
| `mcp::transport::stdio` | `backend/src/mcp/transport/stdio.rs` | STDIO transport — spawns child process with newline-delimited JSON-RPC on stdin/stdout. Command allowlist (npx, node, python, python3, uvx, docker). Blocked env vars (LD_PRELOAD, DYLD_*). |
| `mcp::health_monitor` | `backend/src/mcp/health_monitor.rs` | Background health check per client. Prefers `ping`, falls back to `tools/list`. Tracks consecutive failures, marks client as Error after max failures reached. |
| `mcp::tool_syncer` | `backend/src/mcp/tool_syncer.rs` | Background tool re-discovery per client (configurable interval). Refreshes tool list from `tools/list` JSON-RPC call. |
| `mcp::tool_manager` | `backend/src/mcp/tool_manager.rs` | Tool filtering and execution. Two-tier filtering: client config whitelist (`tools_to_execute`) + per-request headers (`x-kgw-mcp-include-clients`, `x-kgw-mcp-include-tools`). Validates client names (no underscores). |
| `mcp::db` | `backend/src/mcp/db.rs` | `McpDb` — PostgreSQL layer for MCP client configs. Table: `mcp_clients` with columns for connection type, URL/stdio config, auth headers (base64-encoded), tool whitelist, health check settings. |
| `mcp::types` | `backend/src/mcp/types.rs` | Type definitions: `McpClientConfig`, `McpClientState`, `McpConnectionType` (Http/Sse/Stdio), `McpConnectionState` (Connected/Connecting/Disconnected/Error), `McpAuthType`, `McpTool`, `McpStdioConfig`, `JsonRpcRequest/Response`, `ToolExecuteRequest/Response`, `McpToolInfo`. |

### Benchmarking

| Module | File(s) | Description |
|--------|---------|-------------|
| `bench` | `backend/src/bench/mod.rs` | Module root for the built-in benchmarking tool. |
| `bench::runner` | `backend/src/bench/runner.rs` | Benchmark execution engine. Runs concurrent requests against the gateway and collects timing data. |
| `bench::config` | `backend/src/bench/config.rs` | Benchmark configuration (concurrency, iterations, model selection). |
| `bench::metrics` | `backend/src/bench/metrics.rs` | Benchmark-specific metrics collection and aggregation. |
| `bench::mock_server` | `backend/src/bench/mock_server.rs` | Mock Kiro API server for benchmarking without upstream dependencies. |
| `bench::report` | `backend/src/bench/report.rs` | Benchmark result formatting and reporting. |

### Binaries

| Module | File(s) | Description |
|--------|---------|-------------|
| `bin::probe_limits` | `backend/src/bin/probe_limits.rs` | Standalone binary for probing Kiro API limits (max tokens, rate limits, etc.). |

---

## Key Design Patterns

### AppState

All request handlers receive shared application state via Axum's `State` extractor. The `AppState` struct (defined in `backend/src/routes/mod.rs`) contains:

- `config: Arc<RwLock<Config>>` — runtime configuration (hot-reloadable via Web UI)
- `auth_manager: Arc<tokio::sync::RwLock<AuthManager>>` — Kiro token management
- `http_client: Arc<KiroHttpClient>` — connection-pooled HTTP client
- `model_cache: ModelCache` — cached model list
- `resolver: ModelResolver` — model name alias resolution
- `metrics: Arc<MetricsCollector>` — request/latency/token tracking
- `log_buffer: Arc<Mutex<VecDeque<LogEntry>>>` — recent logs for Web UI SSE streaming
- `config_db: Option<Arc<ConfigDb>>` — PostgreSQL config persistence
- `setup_complete: Arc<AtomicBool>` — setup wizard state
- `session_cache: Arc<DashMap<Uuid, SessionInfo>>` — in-memory Google SSO sessions
- `api_key_cache: Arc<DashMap<String, (Uuid, Uuid)>>` — API key hash → (user_id, key_id)
- `kiro_token_cache: Arc<DashMap<Uuid, (String, String, Instant)>>` — per-user Kiro tokens (4-min TTL)
- `oauth_pending: Arc<DashMap<String, OAuthPendingState>>` — PKCE state (10-min TTL, 10k cap)
- `mcp_manager: Option<Arc<McpManager>>` — MCP Gateway orchestrator (None when disabled or not yet initialized)
- `guardrails_engine: Option<Arc<GuardrailsEngine>>` — Content validation engine (None when disabled or no DB)

### Request Guard

The `RequestGuard` pattern (in `backend/src/routes/mod.rs`) ensures metrics are always updated, even if a request handler panics or returns early. It increments active connections on creation and decrements on drop, recording latency and token counts when `complete()` is called.

### Bidirectional Conversion

Format translation follows a consistent pattern: one file per direction (e.g. `openai_to_kiro.rs`, `kiro_to_openai.rs`). Shared logic lives in `converters/core.rs`. This keeps each conversion self-contained and testable.

### Hot-Reload vs Restart

Configuration changes are classified as either `HotReload` (applied immediately to the runtime `Config`) or `RequiresRestart` (persisted to PostgreSQL but only take effect after restart). See `web_ui/config_api.rs` for the classification logic.
