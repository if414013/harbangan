---
layout: default
title: API Reference
nav_order: 6
---

# API Reference
{: .no_toc }

Complete reference for all Kiro Gateway API endpoints. The gateway exposes OpenAI-compatible and Anthropic-compatible APIs that translate requests to the Kiro (AWS CodeWhisperer) backend.
{: .fs-6 .fw-300 }

<details open markdown="block">
  <summary>Table of contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Base URL

All API endpoints are served over HTTPS. The default base URL is:

```
https://your-server:8000
```

The port is configurable via the `SERVER_PORT` environment variable (default: `8000`). TLS is always enabled — the gateway generates a self-signed certificate automatically if no custom certificate is provided.

---

## Authentication

All `/v1/*` endpoints require authentication. The gateway supports two authentication methods:

### Bearer Token (OpenAI-style)

```
Authorization: Bearer YOUR_PROXY_API_KEY
```

### API Key Header (Anthropic-style)

```
x-api-key: YOUR_PROXY_API_KEY
```

The `PROXY_API_KEY` is the password you set during initial setup (via the Web UI wizard or environment variable). Both methods are accepted on all authenticated endpoints — use whichever matches your client library.

### Unauthenticated Endpoints

The following endpoints do not require authentication:

| Endpoint | Purpose |
|----------|---------|
| `GET /` | Root health check (for load balancers) |
| `GET /health` | Detailed health check |
| `/_ui/*` | Web dashboard (setup and config-read routes are public) |

### Authentication Errors

If authentication fails, the gateway returns:

```json
{
  "error": {
    "message": "Invalid or missing API Key",
    "type": "auth_error"
  }
}
```

**HTTP Status:** `401 Unauthorized`

---

## Endpoints

### POST /v1/chat/completions

OpenAI-compatible chat completions endpoint. Supports both streaming and non-streaming responses.

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
curl -k -X POST https://localhost:8000/v1/chat/completions \
  -H "Authorization: Bearer YOUR_PROXY_API_KEY" \
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
curl -k -X POST https://localhost:8000/v1/chat/completions \
  -H "Authorization: Bearer YOUR_PROXY_API_KEY" \
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
    base_url="https://localhost:8000/v1",
    api_key="YOUR_PROXY_API_KEY",
    # For self-signed certs:
    http_client=__import__("httpx").Client(verify=False),
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
  baseURL: "https://localhost:8000/v1",
  apiKey: "YOUR_PROXY_API_KEY",
  // For self-signed certs in Node.js:
  // Set NODE_TLS_REJECT_UNAUTHORIZED=0 in env
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

Anthropic-compatible messages endpoint. Supports both streaming and non-streaming responses.

#### Request Headers

| Header | Required | Description |
|--------|----------|-------------|
| `x-api-key` or `Authorization: Bearer` | Yes | Your proxy API key. |
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
curl -k -X POST https://localhost:8000/v1/messages \
  -H "x-api-key: YOUR_PROXY_API_KEY" \
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

**curl (streaming):**

```bash
curl -k -X POST https://localhost:8000/v1/messages \
  -H "x-api-key: YOUR_PROXY_API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4-20250514",
    "max_tokens": 1024,
    "stream": true,
    "messages": [
      {"role": "user", "content": "Write a haiku about programming."}
    ]
  }'
```

**Python (anthropic library):**

```python
import anthropic

client = anthropic.Anthropic(
    base_url="https://localhost:8000",
    api_key="YOUR_PROXY_API_KEY",
    # For self-signed certs:
    http_client=__import__("httpx").Client(verify=False),
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
  baseURL: "https://localhost:8000",
  apiKey: "YOUR_PROXY_API_KEY",
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

List all available models. Returns models in OpenAI-compatible format.

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
curl -k -H "Authorization: Bearer YOUR_PROXY_API_KEY" \
  https://localhost:8000/v1/models
```

**Python:**

```python
from openai import OpenAI

client = OpenAI(
    base_url="https://localhost:8000/v1",
    api_key="YOUR_PROXY_API_KEY",
    http_client=__import__("httpx").Client(verify=False),
)

models = client.models.list()
for model in models.data:
    print(f"{model.id} (owned by {model.owned_by})")
```

**Node.js:**

```javascript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://localhost:8000/v1",
  apiKey: "YOUR_PROXY_API_KEY",
});

const models = await client.models.list();
for (const model of models.data) {
  console.log(`${model.id} (owned by ${model.owned_by})`);
}
```

---

### GET /health

Detailed health check endpoint. Does not require authentication — designed for load balancers and monitoring systems.

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
curl -k https://localhost:8000/health
```

### GET /

Root endpoint. Returns a simple status check.

#### Response

```json
{
  "status": "ok",
  "message": "Kiro Gateway is running",
  "version": "1.0.8"
}
```

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

### Validation Error Examples

**Empty messages array:**

```json
{
  "error": {
    "message": "messages cannot be empty",
    "type": "validation_error"
  }
}
```

**Invalid max_tokens (Anthropic endpoint):**

```json
{
  "error": {
    "message": "max_tokens must be positive",
    "type": "validation_error"
  }
}
```

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

## HSTS

All responses include the `Strict-Transport-Security` header since TLS is always enabled:

```
Strict-Transport-Security: max-age=31536000
```

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
