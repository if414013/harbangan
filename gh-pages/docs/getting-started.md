---
layout: default
title: Getting Started
nav_order: 2
---

# Getting Started
{: .no_toc }

This guide walks you through installing, configuring, and running Kiro Gateway for the first time. By the end, you will have a working gateway that translates OpenAI and Anthropic API calls into Kiro (AWS CodeWhisperer) backend requests.

<details open markdown="block">
  <summary>Table of contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## What is Kiro Gateway?

Kiro Gateway is a Rust proxy server built with Axum and Tokio. It exposes industry-standard OpenAI (`/v1/chat/completions`) and Anthropic (`/v1/messages`) endpoints, translating every request on the fly into the Kiro API format used by AWS CodeWhisperer. This means any tool or library that speaks the OpenAI or Anthropic protocol can use Kiro models without modification.

Key capabilities:

- Bidirectional format translation (OpenAI/Anthropic to Kiro and back)
- Streaming responses via Server-Sent Events (SSE)
- Automatic token refresh using AWS SSO OIDC
- Built-in TLS with auto-generated self-signed certificates
- Web-based dashboard and configuration UI at `/_ui/`
- PostgreSQL-backed configuration persistence
- Model alias resolution (use familiar model names like `claude-sonnet-4`)

---

## Prerequisites

Before you begin, make sure you have the following installed on your system.

### For building from source

| Requirement | Minimum version | How to check |
|:---|:---|:---|
| Rust toolchain | 1.75+ (2021 edition) | `rustc --version` |
| Cargo | (bundled with Rust) | `cargo --version` |
| PostgreSQL | 14+ | `psql --version` |
| Node.js | 18+ (for building the web UI) | `node --version` |
| npm | 9+ | `npm --version` |
| Git | any recent version | `git --version` |

Install Rust via [rustup](https://rustup.rs/) if you don't have it:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

### For Docker deployment

| Requirement | Minimum version | How to check |
|:---|:---|:---|
| Docker | 20.10+ | `docker --version` |
| Docker Compose | 2.0+ (V2 plugin) | `docker compose version` |

### AWS SSO / Kiro account

You need an active AWS Builder ID or AWS SSO identity that has access to Kiro / Amazon Q Developer. You will authenticate through the Web UI setup wizard using the OAuth device code flow.

---

## Installation

Choose one of the two installation methods below.

### Option A: Build from source

1. Clone the repository:

   ```bash
   git clone https://github.com/if414013/rkgw.git
   cd rkgw
   ```

2. Build the web UI frontend (React + Vite):

   ```bash
   cd web-ui
   npm install
   npm run build
   cd ..
   ```

   This produces the `web-ui/dist/` directory, which gets embedded into the Rust binary at compile time via `rust-embed`.

3. Build the gateway in release mode:

   ```bash
   cargo build --release
   ```

   The compiled binary will be at `target/release/kiro-gateway`.

4. Set up PostgreSQL:

   Create a database for the gateway to store its configuration:

   ```bash
   # Connect to PostgreSQL as a superuser
   psql -U postgres

   # Create the database and user
   CREATE USER kiro WITH PASSWORD 'your_secure_password';
   CREATE DATABASE kiro_gateway OWNER kiro;
   \q
   ```

   The gateway automatically creates the required tables on first connection.

5. Create a `.env` file (or export the variables):

   ```bash
   cp .env.example .env
   ```

   Edit `.env` and set at minimum:

   ```bash
   # Required
   DATABASE_URL=postgres://kiro:your_secure_password@localhost:5432/kiro_gateway
   KIRO_REGION=us-east-1
   ```

   `PROXY_API_KEY` will be configured through the Web UI setup wizard on first launch.

6. Run the gateway:

   ```bash
   cargo run --bin kiro-gateway --release
   ```

   Or run the compiled binary directly:

   ```bash
   ./target/release/kiro-gateway
   ```

   You should see output like:

   ```
   Kiro Gateway starting...
   Connected to PostgreSQL database
   Setup not complete — starting in setup-only mode
   Visit the web UI to complete initial setup
   TLS enabled (self-signed certificate)
   Server listening on https://127.0.0.1:8000
   ```

### Option B: Docker Compose (recommended for servers)

Docker Compose handles PostgreSQL, TLS, and the gateway in a single command.

1. Clone the repository:

   ```bash
   git clone https://github.com/if414013/rkgw.git
   cd rkgw
   ```

2. Create your environment file:

   ```bash
   cp .env.example .env
   ```

   Edit `.env` and set a strong `PROXY_API_KEY` and optionally change `KIRO_REGION`. Do **not** set `SERVER_HOST`, `DATABASE_URL`, `TLS_CERT`, or `TLS_KEY` in `.env` — these are managed by `docker-compose.yml`.

   You can also set a custom PostgreSQL password:

   ```bash
   POSTGRES_PASSWORD=my_strong_db_password
   ```

   If omitted, it defaults to `kiro_secret`.

3. Build and start:

   ```bash
   docker compose up -d --build
   ```

   The first build compiles the Rust binary and the React frontend inside Docker, which takes a few minutes. Subsequent builds are much faster thanks to Docker layer caching.

4. Watch the logs:

   ```bash
   docker compose logs -f gateway
   ```

   Wait until you see:

   ```
   Setup not complete — starting in setup-only mode
   Server listening on https://0.0.0.0:9001
   ```

   The Docker setup uses port **9001** by default (configurable via `SERVER_PORT` in `.env`).

The `docker-compose.yml` starts two services:

| Service | Image | Purpose |
|:---|:---|:---|
| `db` | `postgres:16-alpine` | PostgreSQL database for config persistence |
| `gateway` | `kiro-gateway:latest` (built locally) | The Kiro Gateway proxy |

---

## First-Time Setup Wizard

On first launch, the gateway starts in **setup-only mode**. API endpoints are disabled until you complete the setup through the Web UI.

### Step 1: Open the Web UI

Navigate to the gateway's Web UI in your browser:

- **From source (localhost):** `https://localhost:8000/_ui/`
- **Docker:** `https://your-server:9001/_ui/`

Your browser will warn about the self-signed TLS certificate. This is expected — accept the warning to proceed.

### Step 2: Complete the OAuth setup

The setup wizard guides you through authenticating with AWS SSO using the **OAuth device code flow**. This is the same mechanism used by the Kiro CLI.

You will need to provide:

1. **Gateway password** (`PROXY_API_KEY`) — Choose a strong password. This protects all API endpoints. Clients must include it as `Authorization: Bearer <password>` in every request.

2. **AWS SSO Start URL** — Your organization's AWS SSO portal URL (e.g., `https://my-org.awsapps.com/start`). If you use an AWS Builder ID, you can use the default.

3. **AWS Region** — The region where your Kiro/Q Developer endpoint lives. Defaults to `us-east-1`.

The wizard will:
- Register an OAuth client with AWS SSO OIDC
- Display a device code and a verification URL
- Ask you to open the verification URL in your browser and enter the code
- Poll for authorization completion
- Store the refresh token securely in PostgreSQL

### Step 3: Verify setup completion

Once the OAuth flow completes, the Web UI redirects to the dashboard. The gateway is now fully operational.

---

## Setup Flow Diagram

The following diagram shows the complete setup and request flow:

```mermaid
sequenceDiagram
    participant User as User / Browser
    participant GW as Kiro Gateway
    participant DB as PostgreSQL
    participant SSO as AWS SSO OIDC

    Note over GW: First launch — setup mode
    GW->>DB: Connect & create tables
    DB-->>GW: Ready

    User->>GW: Open /_ui/ (setup wizard)
    GW-->>User: Setup form (password, region, SSO URL)

    User->>GW: POST /api/oauth/start (device flow)
    GW->>SSO: RegisterClient
    SSO-->>GW: client_id, client_secret
    GW->>SSO: StartDeviceAuthorization
    SSO-->>GW: device_code, user_code, verification_uri
    GW-->>User: Show user_code + verification URL

    User->>SSO: Open verification URL, enter code
    SSO-->>User: Authorize

    loop Poll every few seconds
        GW->>SSO: CreateToken (device_code)
        SSO-->>GW: authorization_pending / tokens
    end

    SSO-->>GW: access_token + refresh_token
    GW->>DB: Save credentials (encrypted)
    GW-->>User: Setup complete — redirect to dashboard

    Note over GW: Normal operation mode
    User->>GW: POST /v1/chat/completions
    GW->>GW: Validate Bearer token
    GW->>GW: Convert OpenAI → Kiro format
    GW->>SSO: Refresh access token (if needed)
    GW->>GW: POST to Kiro API
    GW-->>User: SSE stream (Kiro → OpenAI format)
```

---

## Verifying the Installation

Once setup is complete, verify that everything is working.

### Health check

```bash
# Use -k to accept the self-signed certificate
curl -k https://localhost:8000/health
```

Expected response:

```json
{"status":"ok"}
```

### List available models

```bash
curl -k -H "Authorization: Bearer YOUR_PROXY_API_KEY" \
  https://localhost:8000/v1/models
```

This returns a JSON list of all models available through your Kiro account.

### Send a test chat request (OpenAI format)

```bash
curl -k -X POST https://localhost:8000/v1/chat/completions \
  -H "Authorization: Bearer YOUR_PROXY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4",
    "messages": [
      {"role": "user", "content": "Hello! What can you do?"}
    ],
    "stream": true
  }'
```

You should see a streaming SSE response with the model's reply.

### Send a test chat request (Anthropic format)

```bash
curl -k -X POST https://localhost:8000/v1/messages \
  -H "x-api-key: YOUR_PROXY_API_KEY" \
  -H "Content-Type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -d '{
    "model": "claude-sonnet-4",
    "max_tokens": 1024,
    "messages": [
      {"role": "user", "content": "Hello! What can you do?"}
    ],
    "stream": true
  }'
```

### Check the Web UI dashboard

Open `https://localhost:8000/_ui/` in your browser to see:

- Real-time request metrics (latency, token counts)
- System resource usage (CPU, memory)
- Live log viewer
- Configuration management

---

## Connecting AI Tools

Once the gateway is running, point your favorite AI tools at it.

### Cursor / VS Code extensions

Set the API base URL to your gateway:

```
https://localhost:8000/v1
```

Use your `PROXY_API_KEY` as the API key.

### OpenAI Python SDK

```python
from openai import OpenAI

client = OpenAI(
    base_url="https://localhost:8000/v1",
    api_key="YOUR_PROXY_API_KEY",
    # For self-signed certs:
    http_client=httpx.Client(verify=False),
)

response = client.chat.completions.create(
    model="claude-sonnet-4",
    messages=[{"role": "user", "content": "Hello!"}],
)
print(response.choices[0].message.content)
```

### Anthropic Python SDK

```python
import anthropic

client = anthropic.Anthropic(
    base_url="https://localhost:8000",
    api_key="YOUR_PROXY_API_KEY",
)

message = client.messages.create(
    model="claude-sonnet-4",
    max_tokens=1024,
    messages=[{"role": "user", "content": "Hello!"}],
)
print(message.content[0].text)
```

---

## Next Steps

- [Quickstart](quickstart.html) — Get running in under 5 minutes with Docker
- [Configuration Reference](configuration.html) — Full list of environment variables, CLI arguments, and TLS options
