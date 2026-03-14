# Harbangan AI Gateway - High-Level Architecture

## System Overview

Harbangan is a multi-tenant AI gateway that proxies requests to various LLM providers (Kiro/Claude, Anthropic, OpenAI, Copilot, Qwen) with authentication, guardrails, and a Web UI for management.

## Deployment Modes

```mermaid
graph TB
    subgraph "Deployment Modes"
        direction TB
        FULL["🏢 Full Mode<br/>docker-compose.yml"]
        PROXY["⚡ Proxy-Only Mode<br/>docker-compose.gateway.yml"]
    end

    FULL --> FULL_SERVICES["Services:<br/>• PostgreSQL<br/>• Backend (Rust/Axum)<br/>• Frontend (React/Vite)<br/>• Datadog (optional)"]
    FULL --> FULL_AUTH["Auth:<br/>Google SSO + Sessions<br/>Password + TOTP 2FA"]
    FULL --> FULL_UI["Web UI:<br/>Multi-user admin interface"]

    PROXY --> PROXY_SERVICES["Services:<br/>• Gateway only<br/>• Datadog (optional)"]
    PROXY --> PROXY_AUTH["Auth:<br/>Device Code OAuth<br/>AWS SSO OIDC"]
    PROXY --> PROXY_USE["Use Case:<br/>CLI/CI proxy only"]
```

## High-Level Architecture

```mermaid
graph TB
    subgraph "External Clients"
        CLI["CLI / API Clients<br/>OpenAI/Anthropic SDK"]
        BROWSER["Web Browser"]
    end

    subgraph "Harbangan Gateway"
        direction TB

        subgraph "Frontend Layer"
            VITE["Vite Dev Server<br/>Port 5173<br/>(React 19 + TypeScript)"]
            REACT["React SPA<br/>CRT Terminal Aesthetic"]
        end

        subgraph "Backend Layer (Rust/Axum)"
            direction TB

            subgraph "Ingress"
                ROUTER["Axum Router"]
                MIDDLEWARE["Middleware Stack<br/>• CORS<br/>• Auth (API Key / Session)<br/>• Debug Logging<br/>• Setup Guard"]
            end

            subgraph "Core Services"
                PIPELINE["Request Pipeline"]
                GUARDRAILS["Guardrails Engine<br/>(CEL + AWS Bedrock)"]
                CONVERTERS["Format Converters<br/>OpenAI ↔ Kiro ↔ Anthropic"]
                STREAMING["Streaming Parser<br/>AWS Event Stream → SSE"]
            end

            subgraph "Provider Layer"
                PROVIDERS["Provider Trait Implementations"]
                KIRO["Kiro Provider"]
                ANTHRO["Anthropic Direct"]
                OPENAI["OpenAI Codex"]
                COPILOT["GitHub Copilot"]
                QWEN["Qwen"]
            end

            subgraph "State Management"
                APPSTATE["AppState<br/>(Shared State Container)"]
                CACHES["In-Memory Caches<br/>(DashMap)"]
                AUTHMGR["AuthManager<br/>(Token Refresh)"]
            end

            subgraph "Web UI API"
                SESSION["Session Mgmt"]
                GOOGLE["Google OAuth"]
                PASSWORD["Password + TOTP 2FA"]
                APIKEYS["API Key Mgmt"]
                CONFIG["Config Persistence"]
            end
        end

        subgraph "Data Layer"
            POSTGRES["PostgreSQL 16<br/>Config, Users, Sessions,<br/>API Keys, Guardrails"]
        end
    end

    subgraph "External APIs"
        KIRO_API["Kiro API<br/>(AWS Bedrock)"]
        AWS_BEDROCK["AWS Bedrock<br/>Guardrails"]
        ANTHRO_API["Anthropic API"]
        OPENAI_API["OpenAI API"]
    end

    %% Connections
    CLI --> ROUTER
    BROWSER --> VITE
    VITE --> REACT
    REACT --> |"API Calls /_ui/api"| ROUTER

    ROUTER --> MIDDLEWARE
    MIDDLEWARE --> PIPELINE
    MIDDLEWARE --> SESSION

    PIPELINE --> GUARDRAILS
    PIPELINE --> CONVERTERS
    PIPELINE --> PROVIDERS
    PIPELINE --> APPSTATE

    PROVIDERS --> KIRO
    PROVIDERS --> ANTHRO
    PROVIDERS --> OPENAI
    PROVIDERS --> COPILOT
    PROVIDERS --> QWEN

    KIRO --> KIRO_API
    GUARDRAILS --> AWS_BEDROCK
    ANTHRO --> ANTHRO_API
    OPENAI --> OPENAI_API

    APPSTATE --> CACHES
    APPSTATE --> AUTHMGR
    APPSTATE --> POSTGRES

    SESSION --> POSTGRES
    GOOGLE --> POSTGRES
    PASSWORD --> POSTGRES
    APIKEYS --> POSTGRES
    CONFIG --> POSTGRES
```

## Request Flow

```mermaid
sequenceDiagram
    participant Client as Client (SDK/Browser)
    participant Proxy as Vite Dev Proxy
    participant Backend as Backend (Axum)
    participant Middleware as Middleware Stack
    participant Pipeline as Request Pipeline
    participant Guardrails as Guardrails
    participant Provider as Provider Trait
    participant Kiro as Kiro API
    participant Stream as Streaming Parser

    Client->>Backend: POST /v1/chat/completions (port 9999)

    Backend->>Middleware: Route + Middleware
    Middleware->>Middleware: CORS validation
    Middleware->>Middleware: Auth (API key → user lookup)
    Middleware->>Middleware: Setup complete check

    Middleware->>Pipeline: Authenticated request
    Pipeline->>Pipeline: Parse request body
    Pipeline->>Pipeline: Resolve provider routing

    alt Guardrails Enabled
        Pipeline->>Guardrails: Validate input
        Guardrails->>Guardrails: Evaluate CEL rules
        Guardrails->>Guardrails: AWS Bedrock check
        Guardrails-->>Pipeline: Validation result
    end

    Pipeline->>Provider: Dispatch request
    Provider->>Provider: Convert format (OpenAI→Kiro)

    Provider->>Kiro: POST with AWS Event Stream response
    Kiro-->>Provider: Binary chunks

    Provider->>Stream: Parse Event Stream
    loop Streaming chunks
        Stream->>Stream: Extract text/deltas
        Stream->>Backend: SSE event
        Backend-->>Client: data: {...}
    end

    alt Guardrails Enabled (Non-streaming)
        Pipeline->>Guardrails: Validate output
        Guardrails-->>Pipeline: Validation result
    end

    Provider->>Provider: Convert response (Kiro→OpenAI)
    Backend-->>Client: Final SSE or JSON response
```

## Web UI Authentication Flow

```mermaid
sequenceDiagram
    participant Browser as Web Browser
    participant Frontend as React SPA
    participant Backend as Backend API
    participant Google as Google OAuth
    participant DB as PostgreSQL

    Browser->>Frontend: Access /_ui/
    Frontend->>Backend: GET /_ui/api/auth/me
    Backend-->>Frontend: 401 Unauthorized
    Frontend->>Browser: Redirect to /login

    alt Google SSO
        Browser->>Backend: GET /_ui/api/auth/google
        Backend->>Backend: Generate PKCE state
        Backend->>Google: Redirect to Google OAuth
        Google->>Browser: Login page
        Browser->>Google: Credentials
        Google->>Backend: Callback + auth code
        Backend->>Google: Exchange code for tokens
        Backend->>DB: Create user + session
        Backend-->>Browser: Set session cookie
    else Password Auth
        Browser->>Backend: POST /auth/login
        Backend->>DB: Verify Argon2 hash
        alt 2FA Required
            Backend-->>Browser: Pending 2FA token
            Browser->>Backend: POST /auth/login/2fa
            Backend->>DB: Verify TOTP
        end
        Backend->>DB: Create session
        Backend-->>Browser: Set session cookie
    end

    Browser->>Frontend: Reload with session
    Frontend->>Backend: GET /auth/me (with cookie)
    Backend-->>Frontend: User info
    Frontend->>Browser: Render dashboard
```

## Backend Module Structure

```mermaid
graph TB
    subgraph "Backend Modules"
        direction TB

        MAIN["main.rs<br/>Entry point, bootstrap"]

        subgraph "HTTP Layer"
            ROUTES["routes/<br/>Route definitions"]
            MIDDLEWARE["middleware/<br/>CORS, Auth, Debug"]
            WEBUI["web_ui/<br/>Web UI API handlers"]
        end

        subgraph "Business Logic"
            PIPELINE["routes/pipeline.rs<br/>Request routing"]
            GUARDRAILS["guardrails/<br/>Content safety"]
            CONVERTERS["converters/<br/>Format translation"]
            STREAMING["streaming/<br/>Event parsing"]
        end

        subgraph "Provider Layer"
            PROVIDERS["providers/<br/>Provider trait + impls"]
            KIRO["kiro.rs"]
            ANTHRO["anthropic.rs"]
            OPENAI["openai_codex.rs"]
            COPILOT["copilot.rs"]
            QWEN["qwen.rs"]
        end

        subgraph "Support Services"
            AUTH["auth/<br/>Token management"]
            MODELS["models/<br/>Request/response types"]
            CACHE["cache.rs<br/>Model cache"]
            RESOLVER["resolver.rs<br/>Model aliases"]
            HTTPCLIENT["http_client.rs<br/>HTTP client"]
            TOKENIZER["tokenizer.rs<br/>Token counting"]
        end

        subgraph "Data Layer"
            CONFIG["config.rs<br/>Env/DB config"]
            CONFIGDB["web_ui/config_db.rs<br/>PostgreSQL access"]
            STATE["routes/state.rs<br/>AppState"]
        end
    end

    MAIN --> ROUTES
    MAIN --> STATE
    MAIN --> CONFIG

    ROUTES --> MIDDLEWARE
    ROUTES --> PIPELINE
    ROUTES --> WEBUI

    PIPELINE --> GUARDRAILS
    PIPELINE --> CONVERTERS
    PIPELINE --> PROVIDERS
    PIPELINE --> STREAMING

    PROVIDERS --> KIRO
    PROVIDERS --> ANTHRO
    PROVIDERS --> OPENAI
    PROVIDERS --> COPILOT
    PROVIDERS --> QWEN

    KIRO --> AUTH
    KIRO --> HTTPCLIENT
    KIRO --> CONVERTERS

    STATE --> CACHE
    STATE --> CONFIGDB
    STATE --> AUTH

    WEBUI --> CONFIGDB

    CONVERTERS --> MODELS
    STREAMING --> MODELS
```

## Frontend Architecture

```mermaid
graph TB
    subgraph "Frontend (React 19 + Vite)"
        direction TB

        VITE["vite.config.ts<br/>Dev server + proxy"]

        subgraph "Entry"
            MAIN["main.tsx<br/>Entry point"]
            APP["App.tsx<br/>Router configuration"]
        end

        subgraph "Pages"
            LOGIN["Login.tsx"]
            PROFILE["Profile.tsx<br/>API keys, user info"]
            CONFIG["Config.tsx<br/>Gateway settings"]
            ADMIN["Admin.tsx<br/>User management"]
            GUARDRAILS["Guardrails.tsx<br/>Content rules"]
            PROVIDERS["Providers.tsx<br/>OAuth connections"]
        end

        subgraph "Components"
            LAYOUT["Layout.tsx<br/>Shell"]
            SIDEBAR["Sidebar.tsx<br/>Navigation"]
            SESSIONGATE["SessionGate.tsx<br/>Auth guard"]
            ADMINGUARD["AdminGuard.tsx<br/>Role check"]
        end

        subgraph "Utilities"
            API["api.ts<br/>API wrapper"]
            AUTH["auth.ts<br/>CSRF helpers"]
            USESSE["useSSE.ts<br/>SSE hook"]
            THEME["theme.tsx<br/>Light/dark"]
        end

        subgraph "Styling"
            VARIABLES["variables.css<br/>Design tokens"]
            GLOBAL["global.css<br/>Global styles"]
            COMPONENTS["components.css<br/>Component styles"]
        end

        VITE --> MAIN
        MAIN --> APP
        APP --> LOGIN
        APP --> PROFILE
        APP --> CONFIG
        APP --> ADMIN
        APP --> GUARDRAILS
        APP --> PROVIDERS

        APP --> LAYOUT
        LAYOUT --> SIDEBAR
        LAYOUT --> SESSIONGATE

        PROFILE --> API
        API --> AUTH
        PROFILE --> USESSE

        CONFIG --> ADMINGUARD
    end
```

## Data Flow - API Request

```mermaid
flowchart LR
    subgraph "Client"
        REQ["Request<br/>OpenAI format"]
    end

    subgraph "Gateway"
        direction TB

        AUTH["Auth Middleware<br/>API Key → User"]

        subgraph "Request Processing"
            R1["Parse request"]
            R2["Resolve model"]
            R3["Input guardrails"]
            R4["Convert to Kiro"]
        end

        subgraph "Upstream"
            U1["Send to Kiro API"]
            U2["AWS Event Stream"]
        end

        subgraph "Response Processing"
            P1["Parse Event Stream"]
            P2["Extract thinking blocks"]
            P3["Convert to OpenAI"]
            P4["Output guardrails"]
        end

        STREAM["SSE Stream"]
    end

    subgraph "Upstream API"
        KIRO["Kiro API"]
    end

    REQ --> AUTH
    AUTH --> R1 --> R2 --> R3 --> R4
    R4 --> U1 --> U2
    U2 --> KIRO
    KIRO --> P1 --> P2 --> P3 --> P4
    P4 --> STREAM
    STREAM --> REQ
```

## State & Caching Architecture

```mermaid
graph TB
    subgraph "State Management"
        direction TB

        APPSTATE["AppState<br/>Shared container"]

        subgraph "In-Memory Caches (DashMap)"
            SESSION["session_cache<br/>Session ID → SessionInfo<br/>TTL: 24h"]
            APIKEY["api_key_cache<br/>Hash → (user_id, key_id)<br/>Max: 10k"]
            KIROTOK["kiro_token_cache<br/>User ID → (token, region)<br/>TTL: 4min"]
            OAUTH["oauth_pending<br/>State → PKCE verifier<br/>TTL: 10min"]
            MODEL["model_cache<br/>Model metadata<br/>TTL: 1h"]
        end

        subgraph "Persistent Storage"
            DB["PostgreSQL"]
            TABLES["Tables:<br/>users, sessions, api_keys<br/>config, guardrails<br/>provider_tokens"]
        end

        subgraph "External State"
            KIROAPI["Kiro API<br/>Token refresh"]
        end
    end

    APPSTATE --> SESSION
    APPSTATE --> APIKEY
    APPSTATE --> KIROTOK
    APPSTATE --> OAUTH
    APPSTATE --> MODEL

    SESSION -.-> DB
    APIKEY -.-> DB
    KIROTOK -.-> KIROAPI

    DB --> TABLES
```

## Database Schema Overview

```mermaid
erDiagram
    USERS {
        uuid id PK
        string email
        string role
        string auth_method
        string password_hash
        string totp_secret
        datetime created_at
    }

    SESSIONS {
        uuid id PK
        uuid user_id FK
        string ip_address
        datetime created_at
        datetime expires_at
    }

    API_KEYS {
        uuid id PK
        uuid user_id FK
        string key_hash
        string name
        datetime last_used
        datetime created_at
    }

    USER_KIRO_TOKENS {
        uuid user_id PK
        string refresh_token_enc
        string region
        datetime created_at
    }

    CONFIG {
        string key PK
        string value
        datetime updated_at
    }

    GUARDRAIL_PROFILES {
        uuid id PK
        string name
        boolean enabled
        string aws_guardrail_id
    }

    GUARDRAIL_RULES {
        uuid id PK
        string name
        string cel_expression
        string action
    }

    GUARDRAIL_RULE_PROFILES {
        uuid profile_id FK
        uuid rule_id FK
    }

    USERS ||--o{ SESSIONS : has
    USERS ||--o{ API_KEYS : owns
    USERS ||--o| USER_KIRO_TOKENS : has
    GUARDRAIL_PROFILES ||--o{ GUARDRAIL_RULE_PROFILES : contains
    GUARDRAIL_RULES ||--o{ GUARDRAIL_RULE_PROFILES : belongs_to
```

## External Integrations

```mermaid
graph LR
    subgraph "Harbangan"
        GATEWAY["Gateway Core"]
        WEBUI["Web UI"]
    end

    subgraph "Authentication"
        GOOGLE["Google OAuth"]
        AWS_SSO["AWS SSO OIDC"]
    end

    subgraph "LLM Providers"
        KIRO["Kiro<br/>(Claude via AWS)"]
        ANTHRO["Anthropic<br/>(Direct)"]
        OPENAI["OpenAI<br/>(Codex)"]
        COPILOT["GitHub Copilot"]
        QWEN["Alibaba Qwen"]
    end

    subgraph "Infrastructure"
        BEDROCK["AWS Bedrock"]
        POSTGRES[("PostgreSQL")]
        DATADOG["Datadog APM"]
    end

    GATEWAY --> KIRO
    GATEWAY --> ANTHRO
    GATEWAY --> OPENAI
    GATEWAY --> COPILOT
    GATEWAY --> QWEN
    GATEWAY --> BEDROCK
    GATEWAY --> POSTGRES
    GATEWAY --> DATADOG

    WEBUI --> GOOGLE
    WEBUI --> POSTGRES

    KIRO -.-> AWS_SSO
    COPILOT -.-> AWS_SSO
```

## Key Architectural Patterns

| Pattern | Implementation |
|---------|----------------|
| **Layered Architecture** | Middleware → Routes → Pipeline → Providers → External APIs |
| **Dependency Injection** | `AppState` carries all shared dependencies |
| **Trait-Based Abstraction** | `Provider` trait abstracts all LLM backends |
| **Converter Pattern** | Bidirectional format translation between OpenAI/Anthropic/Kiro |
| **Caching Strategy** | `DashMap` for concurrent caches with TTL-based expiration |
| **Streaming with SSE** | `async-stream` for generating SSE from AWS Event Stream |
| **Fail-Open Guardrails** | On engine errors, log warning and allow request through |
| **Background Tasks** | `tokio::spawn` for token refresh, session cleanup |

## File Ownership (per Team Coordination Rules)

| File/Area | Owner Agent |
|-----------|-------------|
| `backend/src/**` | rust-backend-engineer |
| `backend/src/web_ui/config_db.rs` | database-engineer |
| `frontend/src/**` | react-frontend-engineer |
| `docker-compose*.yml`, `**/Dockerfile` | devops-engineer |
| `e2e-tests/**` | frontend-qa |
| `backend/src/**/tests/**` | backend-qa |
