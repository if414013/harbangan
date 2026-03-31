---
layout: default
title: API Reference
nav_order: 6
---

# API Reference
{: .no_toc }

Complete reference for all Harbangan Gateway API endpoints. The gateway exposes OpenAI-compatible and Anthropic-compatible APIs that translate requests to the Kiro (AWS CodeWhisperer) backend.
{: .fs-6 .fw-300 }

<details open markdown="block">
  <summary>Table of contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Base URL

All API endpoints are served by the Rust backend. In development, the base URL is:

```
http://localhost:9999
```

The backend runs on plain HTTP. In production (Kubernetes), TLS is handled by the Ingress controller.

---

## Authentication

The gateway uses two separate authentication systems:

### API Key Auth (for `/v1/*` proxy endpoints)

Each user creates personal API keys through the web dashboard. Clients authenticate using either header format:

**Bearer Token (OpenAI-style):**

```
Authorization: Bearer YOUR_API_KEY
```

**API Key Header (Anthropic-style):**

```
x-api-key: YOUR_API_KEY
```

API keys are created per-user via the web dashboard at `/_ui/`. When a request arrives, the gateway SHA-256 hashes the key, looks up the associated user in cache/DB, and uses that user's Kiro credentials to proxy the request.

### Session Auth (for `/_ui/api/*` web UI endpoints)

Web UI endpoints use session-based authentication. Two login methods are supported, configurable via the admin UI:

- **Google SSO** — PKCE + OpenID Connect flow via Google OAuth
- **Password + TOTP 2FA** — Argon2 password hashing with mandatory TOTP-based two-factor authentication

After signing in, a session cookie (`kgw_session`, 24-hour TTL) authenticates subsequent requests. Mutation endpoints additionally require a CSRF token. See the [Web Dashboard](web-ui.html) docs for details.

### Unauthenticated Endpoints

| Endpoint | Purpose |
|----------|---------|
| `GET /` | Root status check (for load balancers) |
| `GET /health` | Health check |
| `GET /_ui/api/status` | Gateway status (includes which auth methods are enabled) |
| `GET /_ui/api/auth/google` | Google SSO login redirect |
| `GET /_ui/api/auth/google/callback` | Google OAuth callback |
| `POST /_ui/api/auth/login` | Password login (returns session or 2FA challenge) |
| `POST /_ui/api/auth/login/2fa` | TOTP 2FA verification (completes password login) |

### Authentication Errors

If API key authentication fails:

```json
{
  "error": {
    "message": "Invalid or missing API Key",
    "type": "auth_error"
  }
}
```

**HTTP Status:** `401 Unauthorized`

### Setup-Only Mode

On first run (no admin user in the database), the gateway blocks all `/v1/*` proxy endpoints with `503 Service Unavailable` and only serves the web UI for initial setup. Complete setup by signing in at `/_ui/` (via Google SSO or password auth, depending on configuration).

```json
{
  "error": {
    "message": "Setup required. Please complete setup at /_ui/",
    "type": "setup_required"
  }
}
```

**HTTP Status:** `503 Service Unavailable`

---

## Proxy Endpoints

### POST /v1/chat/completions

OpenAI-compatible chat completions endpoint. Supports both streaming and non-streaming responses. Requires API key authentication.

#### Request Body

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `model` | string | Yes | Model name or alias (e.g. `claude-sonnet-4-20250514`, `claude-sonnet-4.5`). The gateway resolves aliases to canonical Kiro model IDs automatically. |
| `messages` | array | Yes | Array of message objects. Must not be empty. |
| `stream` | boolean | No | Whether to stream the response via SSE. Default: `false`. |
| `temperature` | float | No | Sampling temperature (0.0–2.0). |
| `top_p` | float | No | Nucleus sampling parameter. |
| `max_tokens` | integer | No | Maximum tokens to generate. |
| `max_completion_tokens` | integer | No | Alternative to `max_tokens` (OpenAI-compatible). |
| `stop` | string or array | No | Stop sequence(s). |
| `presence_penalty` | float | No | Presence penalty (-2.0 to 2.0). |
| `frequency_penalty` | float | No | Frequency penalty (-2.0 to 2.0). |
| `tools` | array | No | Tool/function definitions for function calling. |
| `tool_choice` | string or object | No | How the model should use tools (`auto`, `none`, or specific tool). |
| `stream_options` | object | No | Streaming options. Set `{"include_usage": true}` to receive token usage in the final chunk (default: `true`). |
| `n` | integer | No | Accepted for compatibility but only `1` is supported. |
| `user` | string | No | Accepted for compatibility, not forwarded. |
| `seed` | integer | No | Accepted for compatibility, not forwarded. |

#### Message Object

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `role` | string | Yes | One of: `system`, `user`, `assistant`, `tool`. |
| `content` | string or array | Yes | Message content. Can be a string or array of content blocks. |
| `name` | string | No | Optional name for the message author. |
| `tool_calls` | array | No | Tool calls made by the assistant (role: `assistant`). |
| `tool_call_id` | string | No | ID of the tool call this message responds to (role: `tool`). |

#### Tool Object

```json
{
  "type": "function",
  "function": {
    "name": "get_weather",
    "description": "Get the current weather for a location",
    "parameters": {
      "type": "object",
      "properties": {
        "location": { "type": "string", "description": "City name" }
      },
      "required": ["location"]
    }
  }
}
```

#### Non-Streaming Response

```json
{
  "id": "chatcmpl-abc123",
  "object": "chat.completion",
  "created": 1709000000,
  "model": "claude-sonnet-4-20250514",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": "Hello! How can I help you today?"
      },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 12,
    "completion_tokens": 9,
    "total_tokens": 21
  }
}
```

#### Streaming Response

When `stream: true`, the response is delivered as Server-Sent Events (SSE):

```
Content-Type: text/event-stream
Cache-Control: no-cache
Connection: keep-alive
```

Each event is a JSON chunk:

```
data: {"id":"chatcmpl-abc123","object":"chat.completion.chunk","created":1709000000,"model":"claude-sonnet-4-20250514","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}

data: {"id":"chatcmpl-abc123","object":"chat.completion.chunk","created":1709000000,"model":"claude-sonnet-4-20250514","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}

data: {"id":"chatcmpl-abc123","object":"chat.completion.chunk","created":1709000000,"model":"claude-sonnet-4-20250514","choices":[{"index":0,"delta":{"content":"!"},"finish_reason":null}]}

data: {"id":"chatcmpl-abc123","object":"chat.completion.chunk","created":1709000000,"model":"claude-sonnet-4-20250514","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
```

When extended thinking is enabled, streaming chunks may include `reasoning_content` in the delta:

```json
{
  "delta": {
    "reasoning_content": "Let me think about this..."
  }
}
```

If `stream_options.include_usage` is `true` (the default), the final chunk before `[DONE]` includes a `usage` field with token counts.

#### Examples

**curl:**

```bash
curl -X POST https://your-domain/v1/chat/completions \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4-20250514",
    "messages": [
      {"role": "system", "content": "You are a helpful assistant."},
      {"role": "user", "content": "What is the capital of France?"}
    ],
    "max_tokens": 100
  }'
```

**curl (streaming):**

```bash
curl -X POST https://your-domain/v1/chat/completions \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4-20250514",
    "messages": [
      {"role": "user", "content": "Write a haiku about programming."}
    ],
    "stream": true
  }'
```

**Python (openai library):**

```python
from openai import OpenAI

client = OpenAI(
    base_url="https://your-domain/v1",
    api_key="YOUR_API_KEY",
)

# Non-streaming
response = client.chat.completions.create(
    model="claude-sonnet-4-20250514",
    messages=[
        {"role": "system", "content": "You are a helpful assistant."},
        {"role": "user", "content": "What is the capital of France?"},
    ],
    max_tokens=100,
)
print(response.choices[0].message.content)

# Streaming
stream = client.chat.completions.create(
    model="claude-sonnet-4-20250514",
    messages=[{"role": "user", "content": "Write a haiku about programming."}],
    stream=True,
)
for chunk in stream:
    if chunk.choices[0].delta.content:
        print(chunk.choices[0].delta.content, end="")
```

**Node.js (openai library):**

```javascript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://your-domain/v1",
  apiKey: "YOUR_API_KEY",
});

// Non-streaming
const response = await client.chat.completions.create({
  model: "claude-sonnet-4-20250514",
  messages: [
    { role: "system", content: "You are a helpful assistant." },
    { role: "user", content: "What is the capital of France?" },
  ],
  max_tokens: 100,
});
console.log(response.choices[0].message.content);

// Streaming
const stream = await client.chat.completions.create({
  model: "claude-sonnet-4-20250514",
  messages: [{ role: "user", content: "Write a haiku about programming." }],
  stream: true,
});
for await (const chunk of stream) {
  process.stdout.write(chunk.choices[0]?.delta?.content || "");
}
```

---

### POST /v1/messages

Anthropic-compatible messages endpoint. Supports both streaming and non-streaming responses. Requires API key authentication.

#### Request Headers

| Header | Required | Description |
|--------|----------|-------------|
| `x-api-key` or `Authorization: Bearer` | Yes | Your per-user API key. |
| `anthropic-version` | No | API version string (e.g. `2023-06-01`). Accepted for compatibility logging but not enforced. |
| `Content-Type` | Yes | Must be `application/json`. |

#### Request Body

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `model` | string | Yes | Model name or alias. |
| `messages` | array | Yes | Array of message objects. Must not be empty. |
| `max_tokens` | integer | Yes | Maximum tokens to generate. Must be positive. |
| `system` | string or array | No | System prompt. Can be a string or array of content blocks with optional `cache_control`. |
| `stream` | boolean | No | Whether to stream the response. Default: `false`. |
| `temperature` | float | No | Sampling temperature (0.0–1.0). |
| `top_p` | float | No | Nucleus sampling parameter. |
| `top_k` | integer | No | Top-k sampling parameter. |
| `stop_sequences` | array | No | Custom stop sequences. |
| `tools` | array | No | Tool definitions for tool use. |
| `tool_choice` | object | No | Tool choice configuration (`auto`, `any`, or specific tool). |
| `metadata` | object | No | Request metadata (accepted but not forwarded). |

#### Anthropic Message Object

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `role` | string | Yes | Either `user` or `assistant`. |
| `content` | string or array | Yes | Message content. Can be a string or array of content blocks (`text`, `image`, `tool_use`, `tool_result`, `thinking`). |

#### Anthropic Tool Object

```json
{
  "name": "get_weather",
  "description": "Get the current weather for a location",
  "input_schema": {
    "type": "object",
    "properties": {
      "location": { "type": "string", "description": "City name" }
    },
    "required": ["location"]
  }
}
```

#### Non-Streaming Response

```json
{
  "id": "msg_abc123",
  "type": "message",
  "role": "assistant",
  "content": [
    {
      "type": "text",
      "text": "The capital of France is Paris."
    }
  ],
  "model": "claude-sonnet-4-20250514",
  "stop_reason": "end_turn",
  "stop_sequence": null,
  "usage": {
    "input_tokens": 25,
    "output_tokens": 12
  }
}
```

#### Streaming Response

When `stream: true`, the response is delivered as Anthropic-format SSE events:

```
event: message_start
data: {"type":"message_start","message":{"id":"msg_abc123","type":"message","role":"assistant","content":[],"model":"claude-sonnet-4-20250514","usage":{"input_tokens":25,"output_tokens":0}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"The capital"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" of France is Paris."}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":12}}

event: message_stop
data: {"type":"message_stop"}
```

Thinking blocks appear as separate content blocks with `type: "thinking"` and deltas with `type: "thinking_delta"`.

#### Examples

**curl:**

```bash
curl -X POST https://your-domain/v1/messages \
  -H "x-api-key: YOUR_API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4-20250514",
    "max_tokens": 1024,
    "messages": [
      {"role": "user", "content": "What is the capital of France?"}
    ]
  }'
```

**Python (anthropic library):**

```python
import anthropic

client = anthropic.Anthropic(
    base_url="https://your-domain",
    api_key="YOUR_API_KEY",
)

# Non-streaming
message = client.messages.create(
    model="claude-sonnet-4-20250514",
    max_tokens=1024,
    messages=[
        {"role": "user", "content": "What is the capital of France?"}
    ],
)
print(message.content[0].text)

# Streaming
with client.messages.stream(
    model="claude-sonnet-4-20250514",
    max_tokens=1024,
    messages=[{"role": "user", "content": "Write a haiku about programming."}],
) as stream:
    for text in stream.text_stream:
        print(text, end="")
```

**Node.js (anthropic library):**

```javascript
import Anthropic from "@anthropic-ai/sdk";

const client = new Anthropic({
  baseURL: "https://your-domain",
  apiKey: "YOUR_API_KEY",
});

// Non-streaming
const message = await client.messages.create({
  model: "claude-sonnet-4-20250514",
  max_tokens: 1024,
  messages: [
    { role: "user", content: "What is the capital of France?" },
  ],
});
console.log(message.content[0].text);

// Streaming
const stream = client.messages.stream({
  model: "claude-sonnet-4-20250514",
  max_tokens: 1024,
  messages: [{ role: "user", content: "Write a haiku about programming." }],
});
for await (const event of stream) {
  if (event.type === "content_block_delta" && event.delta.type === "text_delta") {
    process.stdout.write(event.delta.text);
  }
}
```

---

### GET /v1/models

List all available models. Returns models in OpenAI-compatible format. Requires API key authentication.

#### Response

```json
{
  "object": "list",
  "data": [
    {
      "id": "claude-sonnet-4-20250514",
      "object": "model",
      "created": 1709000000,
      "owned_by": "anthropic",
      "description": "Claude model via Kiro API"
    },
    {
      "id": "claude-haiku-4-20250414",
      "object": "model",
      "created": 1709000000,
      "owned_by": "anthropic",
      "description": "Claude model via Kiro API"
    }
  ]
}
```

#### Examples

**curl:**

```bash
curl -H "Authorization: Bearer YOUR_API_KEY" \
  https://your-domain/v1/models
```

**Python:**

```python
from openai import OpenAI

client = OpenAI(
    base_url="https://your-domain/v1",
    api_key="YOUR_API_KEY",
)

models = client.models.list()
for model in models.data:
    print(f"{model.id} (owned by {model.owned_by})")
```

---

## Infrastructure Endpoints

### GET /health

Health check endpoint. Does not require authentication — designed for load balancers and monitoring systems.

#### Response

```json
{
  "status": "healthy",
  "timestamp": "2025-03-01T12:00:00.000Z",
  "version": "1.0.8"
}
```

#### Example

```bash
curl https://your-domain/health
```

### GET /

Root endpoint. Returns a simple status check. No authentication required.

#### Response

```json
{
  "status": "ok",
  "message": "Kiro Gateway is running",
  "version": "1.0.8"
}
```

---

## Web UI API Endpoints

> **Full Deployment only.** The `/_ui/api/*` endpoints are not available in Proxy-Only Mode — the Web UI and all session-based routes are disabled when `GATEWAY_MODE=proxy`.

All web UI API endpoints are under `/_ui/api/`. See the [Web Dashboard](web-ui.html) documentation for full details.

### Public (No Authentication)

| Method | Path | Description |
|---|---|---|
| `GET` | `/_ui/api/status` | Gateway status and setup state |
| `GET` | `/_ui/api/auth/google` | Initiate Google SSO |
| `GET` | `/_ui/api/auth/google/callback` | OAuth callback |
| `POST` | `/_ui/api/auth/login` | Password login |
| `POST` | `/_ui/api/auth/login/2fa` | TOTP 2FA verification |
| `GET` | `/_ui/api/providers/:provider/relay-script` | Provider OAuth relay script |
| `POST` | `/_ui/api/providers/:provider/relay` | Provider OAuth relay callback |

### Session-Authenticated (requires `kgw_session` cookie)

| Method | Path | Description |
|---|---|---|
| `GET` | `/_ui/api/auth/me` | Current user info |
| `GET` | `/_ui/api/system` | System info |
| `GET` | `/_ui/api/models` | Available models |
| `GET` | `/_ui/api/usage` | Per-user usage stats |
| `GET` | `/_ui/api/auth/google/link` | Google account linking redirect |
| `GET` | `/_ui/api/auth/2fa/setup` | TOTP 2FA setup (generate secret + QR) |
| `POST` | `/_ui/api/auth/2fa/verify` | Enable TOTP 2FA |
| `POST` | `/_ui/api/auth/password/change` | Change password |
| `GET` | `/_ui/api/providers/registry` | Provider registry |
| `GET` | `/_ui/api/providers/status` | Provider connection status |
| `GET` | `/_ui/api/providers/:provider/connect` | Initiate provider OAuth |
| `DELETE` | `/_ui/api/providers/:provider` | Disconnect provider |
| `GET` | `/_ui/api/providers/priority` | Get provider priority |
| `POST` | `/_ui/api/providers/priority` | Update provider priority |
| `GET` | `/_ui/api/providers/:provider/accounts` | List user provider accounts |
| `GET` | `/_ui/api/providers/rate-limits` | Provider rate limit info |
| `GET` | `/_ui/api/models/registry` | Model registry list |
| `PATCH` | `/_ui/api/models/registry/:id` | Toggle model enabled/disabled |
| `DELETE` | `/_ui/api/models/registry/:id` | Delete registry model |
| `POST` | `/_ui/api/models/registry/populate` | Auto-populate models from providers |

### Mutations (Session + CSRF Token)

| Method | Path | Description |
|---|---|---|
| `POST` | `/_ui/api/auth/logout` | End session |
| `*` | `/_ui/api/kiro/*` | Kiro token management (status/setup/poll/delete) |
| `*` | `/_ui/api/keys/*` | API key management (list/create/delete) |
| `*` | `/_ui/api/copilot/*` | GitHub Copilot device flow (device-code/device-poll/status/disconnect) |

### Admin-Only (Session + CSRF + Admin Role)

| Method | Path | Description |
|---|---|---|
| `GET` | `/_ui/api/config` | Get configuration |
| `PUT` | `/_ui/api/config` | Update configuration |
| `GET` | `/_ui/api/config/schema` | Config field metadata |
| `GET` | `/_ui/api/config/history` | Config change history |
| `*` | `/_ui/api/domains/*` | Domain allowlist (list/add/remove) |
| `*` | `/_ui/api/users/*` | User management (list/detail/role/delete) |
| `POST` | `/_ui/api/admin/users/create` | Create user with password |
| `POST` | `/_ui/api/admin/users/:id/reset-password` | Reset user password |
| `GET` | `/_ui/api/admin/usage` | Global usage stats |
| `GET` | `/_ui/api/admin/usage/users` | Per-user usage breakdown |
| `*` | `/_ui/api/admin/pool/*` | Provider pool accounts (CRUD) |
| `*` | `/_ui/api/guardrails/*` | Guardrails profile/rule management + test/validate |

---

## Error Responses

All errors follow a consistent JSON format:

```json
{
  "error": {
    "message": "Human-readable error description",
    "type": "error_type"
  }
}
```

### Error Types and Status Codes

| HTTP Status | Error Type | Description |
|-------------|-----------|-------------|
| `400` | `validation_error` | Invalid request body, missing required fields, or invalid parameter values. |
| `400` | `invalid_model` | The requested model name could not be resolved. |
| `401` | `auth_error` | Missing or invalid API key. |
| `429` | `kiro_api_error` | Rate limit exceeded on the upstream Kiro API. |
| `500` | `internal_error` | Unexpected server error. The actual error message is logged server-side; clients receive a generic message. |
| `500` | `config_error` | Server configuration issue (e.g. missing database). |
| `503` | `setup_required` | Initial setup has not been completed. Visit `/_ui/` to configure the gateway. |
| Various | `kiro_api_error` | Upstream Kiro API returned an error. The HTTP status is forwarded from the upstream response. |
| `403` | `guardrail_blocked` | Content blocked by guardrail policy. Response includes violation details. |
| `200` | `guardrail_warning` | Content was redacted by guardrail (e.g. PII masking). Includes redacted content. |

---

## Model Name Resolution

The gateway includes a model resolver that maps common model aliases to canonical Kiro model IDs. You can use any of the following naming patterns:

- Canonical Kiro model IDs (e.g. `claude-sonnet-4-20250514`)
- Short aliases (e.g. `claude-sonnet-4.5`, `claude-haiku-4`)
- OpenAI-style names (e.g. `claude-3-5-sonnet`)

The resolver checks the model cache (populated at startup from the Kiro API) and falls back to best-effort matching. Use `GET /v1/models` to see all available model IDs.

---

## CORS

The gateway allows all origins, methods, and headers via permissive CORS configuration. This means you can call the API directly from browser-based applications without encountering CORS errors.

Response headers on all requests:

```
Access-Control-Allow-Origin: *
Access-Control-Allow-Methods: *
Access-Control-Allow-Headers: *
```

OPTIONS preflight requests are handled automatically.

---

## Rate Limiting

The gateway itself does not enforce rate limits. However, the upstream Kiro API has its own rate limits. When the upstream returns a `429 Too Many Requests` response, the gateway forwards it to the client as a `kiro_api_error`.

The gateway's HTTP client includes automatic retry logic with configurable parameters:

| Setting | Default | Description |
|---------|---------|-------------|
| `http_max_retries` | 3 | Maximum retry attempts for failed requests. |
| `http_connect_timeout` | 30s | Connection timeout. |
| `http_request_timeout` | 300s | Overall request timeout. |
| `first_token_timeout` | 15s | Timeout waiting for the first token in a streaming response. |

---

## Truncation Recovery

The gateway includes automatic truncation recovery for responses that are cut off mid-stream. When enabled (default: `true`), the gateway injects recovery instructions into the conversation context and detects truncated responses to trigger retries.

This feature can be toggled via the `truncation_recovery` configuration option in the Web UI.

---

## Extended Thinking / Reasoning

The gateway supports extended thinking (reasoning) for models that support it. In the OpenAI-compatible endpoint, reasoning content appears in the `reasoning_content` field of streaming deltas. In the Anthropic-compatible endpoint, thinking blocks appear as `thinking` content blocks.

The `fake_reasoning_enabled` configuration option (default: `true`) controls whether the gateway extracts and surfaces reasoning blocks from the model's response. The `fake_reasoning_max_tokens` setting (default: `4000`) controls the maximum token budget for reasoning output.
