---
title: Home
layout: default
nav_order: 1
---

<div class="hero" markdown="0">
  <h1>Kiro Gateway</h1>
  <p class="tagline">
    A high-performance Rust proxy that lets you use OpenAI and Anthropic client libraries
    with the Kiro API (AWS CodeWhisperer) backend. Built with Axum and Tokio.
  </p>
  <div class="badges">
    <span class="badge">Rust</span>
    <span class="badge">Axum 0.7</span>
    <span class="badge">OpenAI Compatible</span>
    <span class="badge">Anthropic Compatible</span>
    <span class="badge">Streaming</span>
  </div>
</div>

## How It Works

Kiro Gateway sits between your existing AI client code and the Kiro API. Send requests in OpenAI or Anthropic format -- the gateway translates them on the fly, handles authentication, and streams responses back in the format your client expects.

```mermaid
flowchart LR
    subgraph Clients
        OAI["OpenAI Client"]
        ANT["Anthropic Client"]
    end

    subgraph GW["Kiro Gateway"]
        MW["Middleware\n(CORS, Auth)"]
        CONV["Format\nConverters"]
        STREAM["Stream\nParser"]
    end

    subgraph Backend
        KIRO["Kiro API\n(CodeWhisperer)"]
        SSO["AWS SSO\nOIDC"]
    end

    OAI --> MW
    ANT --> MW
    MW --> CONV
    CONV --> KIRO
    KIRO --> STREAM
    STREAM --> OAI
    STREAM --> ANT
    GW -.-> SSO
```

## Features

<div class="features" markdown="0">
  <div class="feature-card">
    <h3>OpenAI Compatible</h3>
    <p>Drop-in replacement for the OpenAI API. Use any OpenAI client library -- just point it at the gateway.</p>
  </div>
  <div class="feature-card">
    <h3>Anthropic Compatible</h3>
    <p>Full support for the Anthropic Messages API, including system prompts, tool use, and content blocks.</p>
  </div>
  <div class="feature-card">
    <h3>Real-time Streaming</h3>
    <p>Parses Kiro's AWS Event Stream binary format and converts to standard SSE in real time.</p>
  </div>
  <div class="feature-card">
    <h3>Auto Authentication</h3>
    <p>Manages OAuth tokens via AWS SSO OIDC with automatic refresh before expiry. Zero manual token handling.</p>
  </div>
  <div class="feature-card">
    <h3>Extended Thinking</h3>
    <p>Extracts reasoning blocks from model responses and maps them to native thinking/reasoning content fields.</p>
  </div>
  <div class="feature-card">
    <h3>Web Dashboard</h3>
    <p>Built-in web UI for configuration, monitoring, and real-time log streaming. Optional TUI dashboard too.</p>
  </div>
</div>

## Quick Start

```bash
# Clone and build
git clone https://github.com/if414013/rkgw.git
cd rkgw
cargo build --release

# Configure
export PROXY_API_KEY="your-secret-key"
export DATABASE_URL="postgres://user:pass@localhost:5432/kiro_gateway"

# Run
cargo run --bin kiro-gateway --release
```

Then point your OpenAI or Anthropic client at `http://localhost:8000`.

## Documentation

<div class="nav-cards" markdown="0">
  <a href="{{ site.baseurl }}/docs/getting-started" class="nav-card">
    <span class="icon">&#128640;</span>
    Getting Started
  </a>
  <a href="{{ site.baseurl }}/docs/architecture" class="nav-card">
    <span class="icon">&#127959;</span>
    Architecture
  </a>
  <a href="{{ site.baseurl }}/docs/api-reference" class="nav-card">
    <span class="icon">&#128214;</span>
    API Reference
  </a>
  <a href="{{ site.baseurl }}/docs/modules" class="nav-card">
    <span class="icon">&#128230;</span>
    Modules
  </a>
  <a href="{{ site.baseurl }}/docs/deployment" class="nav-card">
    <span class="icon">&#128225;</span>
    Deployment
  </a>
  <a href="{{ site.baseurl }}/docs/troubleshooting" class="nav-card">
    <span class="icon">&#128295;</span>
    Troubleshooting
  </a>
</div>

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/chat/completions` | POST | OpenAI-compatible chat completions |
| `/v1/messages` | POST | Anthropic-compatible messages |
| `/v1/models` | GET | List available models |
| `/health` | GET | Health check |
| `/_ui/` | GET | Web dashboard |
