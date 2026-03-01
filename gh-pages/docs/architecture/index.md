---
layout: default
title: Architecture
nav_order: 5
has_children: true
permalink: /architecture/
---

# Architecture Overview
{: .no_toc }

Kiro Gateway is a Rust proxy that exposes OpenAI and Anthropic-compatible APIs, translating requests to the Kiro API (AWS CodeWhisperer) backend. This section provides a comprehensive look at the system's internal architecture, from the high-level component layout down to individual module responsibilities.

## Table of Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## High-Level System Diagram

The gateway sits between AI clients (any tool that speaks the OpenAI or Anthropic protocol) and the Kiro/CodeWhisperer backend on AWS. It handles authentication, format translation, streaming, and extended thinking extraction transparently.

```mermaid
flowchart TB
    subgraph Clients["Client Applications"]
        OAI["OpenAI-compatible Client<br/>(e.g. Cursor, Continue, OpenCode)"]
        ANT["Anthropic-compatible Client<br/>(e.g. Claude Code, Aider)"]
    end

    subgraph Gateway["Kiro Gateway (Axum + Tokio)"]
        subgraph MW["Middleware Stack"]
            CORS["CORS Layer<br/><i>tower-http</i>"]
            HSTS["HSTS Middleware"]
            DEBUG["Debug Logger"]
            AUTH["Auth Middleware<br/><i>PROXY_API_KEY check</i>"]
        end

        subgraph Routes["Route Handlers"]
            HEALTH["GET /<br/>GET /health"]
            OPENAI["POST /v1/chat/completions<br/>GET /v1/models"]
            ANTHRO["POST /v1/messages"]
            WEBUI["/_ui/*<br/><i>Web Dashboard</i>"]
        end

        subgraph Core["Core Services"]
            CONFIG["Config<br/><i>CLI + ENV + DB</i>"]
            CACHE["ModelCache<br/><i>DashMap + TTL</i>"]
            RESOLVER["ModelResolver<br/><i>Name normalization</i>"]
            AUTHMGR["AuthManager<br/><i>Token lifecycle</i>"]
            HTTPC["KiroHttpClient<br/><i>Pooled + retry</i>"]
            METRICS["MetricsCollector"]
        end

        subgraph Convert["Format Converters"]
            O2K["openai_to_kiro"]
            A2K["anthropic_to_kiro"]
            K2O["kiro_to_openai"]
            K2A["kiro_to_anthropic"]
            CORE_C["core.rs<br/><i>Unified types</i>"]
        end

        subgraph Stream["Streaming Pipeline"]
            PARSER["AWS Event Stream<br/>Binary Parser"]
            THINK["ThinkingParser<br/><i>FSM for &lt;thinking&gt; tags</i>"]
            SSE["SSE Formatter"]
            TRUNC["Truncation Recovery"]
        end
    end

    subgraph External["External Services"]
        KIRO["Kiro API<br/><i>codewhisperer.{region}.amazonaws.com</i>"]
        QAPI["Q API<br/><i>q.{region}.amazonaws.com</i>"]
        SSOOIDC["AWS SSO OIDC<br/><i>oidc.{region}.amazonaws.com</i>"]
        PG[("PostgreSQL<br/><i>Config + Credentials</i>")]
    end

    OAI --> CORS
    ANT --> CORS
    CORS --> HSTS --> DEBUG --> AUTH
    AUTH --> OPENAI
    AUTH --> ANTHRO
    HEALTH -.-> |no auth| CORS

    OPENAI --> O2K
    ANTHRO --> A2K
    O2K --> CORE_C
    A2K --> CORE_C
    CORE_C --> HTTPC
    HTTPC --> KIRO

    KIRO --> PARSER
    PARSER --> THINK
    THINK --> K2O
    THINK --> K2A
    K2O --> SSE
    K2A --> SSE
    SSE --> OAI
    SSE --> ANT

    AUTHMGR --> PG
    AUTHMGR --> SSOOIDC
    HTTPC --> AUTHMGR
    RESOLVER --> CACHE
    CACHE -.-> QAPI
    CONFIG --> PG
```

---

## Application State (AppState)

All Axum route handlers share a single `AppState` struct via Axum's state extraction. This struct is the central nervous system of the gateway — it holds references to every core service.

```mermaid
classDiagram
    class AppState {
        +ModelCache model_cache
        +Arc~RwLock~AuthManager~~ auth_manager
        +Arc~KiroHttpClient~ http_client
        +ModelResolver resolver
        +Arc~RwLock~Config~~ config
        +Arc~AtomicBool~ setup_complete
        +Arc~MetricsCollector~ metrics
        +Arc~Mutex~VecDeque~LogEntry~~~ log_buffer
        +Option~Arc~ConfigDb~~ config_db
    }

    class ModelCache {
        +DashMap cache
        +u64 cache_ttl
        +update(models)
        +is_valid_model(id) bool
        +add_hidden_model(display, internal)
        +get_all_model_ids() Vec~String~
    }

    class AuthManager {
        +Arc~RwLock~Credentials~~ credentials
        +Arc~RwLock~Option~String~~~ access_token
        +Arc~RwLock~Option~DateTime~~~ expires_at
        +Client client
        +i64 refresh_threshold
        +get_access_token() Result~String~
        +get_region() String
    }

    class KiroHttpClient {
        +Client client
        +Arc~AuthManager~ auth_manager
        +u32 max_retries
        +request_with_retry(req) Result
        +request_no_retry(req) Result
    }

    class ModelResolver {
        +ModelCache cache
        +HashMap hidden_models
        +resolve(name) ModelResolution
    }

    class Config {
        +String server_host
        +u16 server_port
        +String proxy_api_key
        +String kiro_region
        +DebugMode debug_mode
        +bool fake_reasoning_enabled
        +bool truncation_recovery
    }

    AppState --> ModelCache
    AppState --> AuthManager
    AppState --> KiroHttpClient
    AppState --> ModelResolver
    AppState --> Config
    ModelResolver --> ModelCache
    KiroHttpClient --> AuthManager
```

Key design decisions for AppState:

- `auth_manager` is wrapped in `tokio::sync::RwLock` so it can be swapped at runtime after re-authentication via the Web UI.
- `config` uses `std::sync::RwLock` since config reads are synchronous and fast.
- `model_cache` uses `DashMap` internally for lock-free concurrent reads.
- `setup_complete` is an `AtomicBool` that gates API routes — when `false`, only the Web UI and health endpoints are accessible.

---

## Module Dependency Graph

The following diagram shows how the Rust modules depend on each other. Arrows point from the dependent module to the dependency.

```mermaid
flowchart TD
    MAIN["main.rs"] --> CONFIG["config"]
    MAIN --> AUTH["auth/"]
    MAIN --> CACHE["cache"]
    MAIN --> RESOLVER["resolver"]
    MAIN --> HTTPC["http_client"]
    MAIN --> ROUTES["routes/"]
    MAIN --> MW["middleware/"]
    MAIN --> WEBUI["web_ui/"]
    MAIN --> METRICS["metrics"]
    MAIN --> DASH["dashboard/"]
    MAIN --> TLS["tls"]

    ROUTES --> CONVERTERS["converters/"]
    ROUTES --> STREAMING["streaming/"]
    ROUTES --> AUTH
    ROUTES --> CACHE
    ROUTES --> RESOLVER
    ROUTES --> HTTPC
    ROUTES --> MODELS["models/"]
    ROUTES --> TOKENIZER["tokenizer"]
    ROUTES --> TRUNC["truncation"]
    ROUTES --> METRICS

    STREAMING --> THINK["thinking_parser"]
    STREAMING --> TRUNC
    STREAMING --> ERROR["error"]

    CONVERTERS --> MODELS
    CONVERTERS --> CONFIG

    AUTH --> WEBUI

    MW --> ERROR
    MW --> ROUTES

    HTTPC --> AUTH
    HTTPC --> ERROR

    RESOLVER --> CACHE
```

---

## Technology Stack

| Layer | Technology | Purpose |
|-------|-----------|---------|
| HTTP Server | [Axum 0.7](https://github.com/tokio-rs/axum) | Async web framework with type-safe extractors |
| Async Runtime | [Tokio](https://tokio.rs/) | Multi-threaded async runtime |
| Middleware | [tower](https://github.com/tower-rs/tower) / tower-http | Composable middleware layers (CORS, logging) |
| HTTP Client | [reqwest](https://github.com/seanmonstar/reqwest) | Connection-pooled HTTP client with TLS |
| TLS | [rustls](https://github.com/rustls/rustls) + ring | Always-on TLS (self-signed or custom cert) |
| Serialization | [serde](https://serde.rs/) + serde_json | JSON serialization/deserialization |
| CLI Parsing | [clap](https://github.com/clap-rs/clap) | CLI argument parsing with env var support |
| Database | [sqlx](https://github.com/launchbadge/sqlx) (PostgreSQL) | Async PostgreSQL for config persistence |
| Caching | [DashMap](https://github.com/xacrimon/dashmap) | Lock-free concurrent hash map |
| Logging | [tracing](https://github.com/tokio-rs/tracing) | Structured, async-aware logging |
| Token Counting | [tiktoken-rs](https://github.com/zurawiki/tiktoken-rs) | GPT-compatible tokenizer (cl100k_base) |
| TUI Dashboard | [ratatui](https://github.com/ratatui-org/ratatui) | Terminal UI for real-time monitoring |
| Web UI | React + Vite (embedded via rust-embed) | Browser-based setup and monitoring |

---

## Design Principles

### 1. Protocol Translation, Not Reimplementation

The gateway does not implement its own LLM logic. It is a pure protocol translator: it accepts requests in OpenAI or Anthropic format, converts them to the Kiro wire format, and converts responses back. The `converters/core.rs` module defines a `UnifiedMessage` type that serves as the intermediate representation between all three formats.

### 2. Always-On TLS

TLS is mandatory. If no custom certificate is provided, the gateway generates a self-signed certificate at startup. This simplifies deployment security — there is no "HTTP mode" to accidentally expose.

### 3. Streaming-First Architecture

The Kiro API always returns responses in AWS Event Stream binary format, even for non-streaming requests. The gateway's streaming pipeline (`streaming/mod.rs`) is the primary response path. Non-streaming responses are simply collected from the stream into a single JSON object.

### 4. Graceful Degradation

The auth system implements graceful degradation: if a token refresh fails but the current token hasn't expired yet, the gateway continues serving requests with the existing token. This prevents transient OIDC failures from causing immediate outages.

### 5. Setup-First Mode

The gateway can start with no configuration. When `setup_complete` is `false`, only the Web UI is accessible. Users complete initial setup (OAuth device code flow, region selection) through the browser, and the gateway transitions to full operation without a restart.

---

## Source File Map

| File | Description |
|------|-------------|
| `src/main.rs` | Entry point, startup orchestration, Axum app builder |
| `src/config.rs` | Configuration from CLI + ENV + .env + PostgreSQL |
| `src/error.rs` | `ApiError` enum with `IntoResponse` for HTTP error mapping |
| `src/cache.rs` | Thread-safe model metadata cache (DashMap) |
| `src/resolver.rs` | Model name normalization and resolution pipeline |
| `src/auth/` | OAuth token lifecycle (manager, credentials, refresh, oauth, types) |
| `src/http_client.rs` | Connection-pooled HTTP client with retry + backoff |
| `src/routes/mod.rs` | Axum route handlers and AppState definition |
| `src/streaming/mod.rs` | AWS Event Stream parser, SSE formatters |
| `src/thinking_parser.rs` | FSM for extracting `<thinking>` blocks from streams |
| `src/converters/` | Bidirectional format translation (OpenAI/Anthropic/Kiro) |
| `src/models/` | Request/response type definitions per API format |
| `src/middleware/` | Auth, CORS, HSTS, and debug logging middleware |
| `src/tokenizer.rs` | Token counting with Claude correction factor |
| `src/truncation.rs` | Truncation detection and recovery injection |
| `src/metrics/` | Request latency, token usage, and error tracking |
| `src/dashboard/` | Optional ratatui TUI for real-time monitoring |
| `src/web_ui/` | Web dashboard (React SPA, config API, SSE logs) |
| `src/tls.rs` | TLS configuration (self-signed or custom cert) |
