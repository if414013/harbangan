---
layout: default
title: Client Setup
nav_order: 7
---

# Client Setup
{: .no_toc }

Configure your AI tools and SDKs to use Harbangan as their backend. Each client points at your gateway's base URL and authenticates with a personal API key.

<details open markdown="block">
  <summary>Table of contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Before You Start

Your configuration depends on your deployment mode:

| | Proxy-Only | Full Deployment |
|---|---|---|
| **API Key** | `PROXY_API_KEY` from `.env.proxy` | Personal key from Web UI (`/_ui/`) |
| **Providers** | Kiro only | Kiro + Anthropic, OpenAI Codex, Copilot, Qwen |
| **Models** | `claude-*` names, `auto` | + `anthropic/`, `openai_codex/`, `copilot/`, `qwen/` prefixes |
| **Web UI** | Not available | Available at `/_ui/` for OAuth setup |

---

## Claude Code CLI

The fastest way to get started. Claude Code works out of the box with the Anthropic-compatible `/v1/messages` endpoint.

**Demo:**

<video src="https://github.com/user-attachments/assets/f404096e-b326-41e5-a4b3-3f94a73d2ece" controls width="100%"></video>

### One-liner

```bash
ANTHROPIC_BASE_URL=https://gateway.example.com \
ANTHROPIC_AUTH_TOKEN=YOUR_API_KEY \
CLAUDE_CODE_ENABLE_TELEMETRY=0 \
DISABLE_PROMPT_CACHING=1 \
DISABLE_NON_ESSENTIAL_MODEL_CALLS=1 \
CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
claude
```

### Shell profile (persistent)

Add the following to your `~/.bashrc`, `~/.zshrc`, or equivalent:

```bash
export ANTHROPIC_BASE_URL=https://gateway.example.com
export ANTHROPIC_AUTH_TOKEN=YOUR_API_KEY
export CLAUDE_CODE_ENABLE_TELEMETRY=0
export DISABLE_PROMPT_CACHING=1
export DISABLE_NON_ESSENTIAL_MODEL_CALLS=1
export CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1
```

### Environment variables

| Variable | Value | Purpose |
|:---------|:------|:--------|
| `ANTHROPIC_BASE_URL` | `https://gateway.example.com` | Points Claude Code at your gateway |
| `ANTHROPIC_AUTH_TOKEN` | `YOUR_API_KEY` | API key from the Web UI or `PROXY_API_KEY` |
| `CLAUDE_CODE_ENABLE_TELEMETRY` | `0` | Disables telemetry (calls that would fail through the proxy) |
| `DISABLE_PROMPT_CACHING` | `1` | Disables prompt caching (not supported by Kiro backend) |
| `DISABLE_NON_ESSENTIAL_MODEL_CALLS` | `1` | Prevents background model calls that waste tokens |
| `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` | `1` | Prevents non-essential network requests |

{: .note }
Replace `gateway.example.com` with your actual domain. For the API key: use a personal key from the Web UI (`/_ui/ → API Keys`) in full deployment, or your `PROXY_API_KEY` from `.env.proxy` in proxy-only mode.

---

## Zed Editor

Add the following to `~/.config/zed/settings.json`:

```json
{
  "agent_servers": {
    "claude": {
      "env": {
        "ANTHROPIC_BASE_URL": "https://gateway.example.com",
        "ANTHROPIC_AUTH_TOKEN": "YOUR_API_KEY",
        "CLAUDE_CODE_ENABLE_TELEMETRY": "0",
        "DISABLE_PROMPT_CACHING": "1",
        "DISABLE_NON_ESSENTIAL_MODEL_CALLS": "1",
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1"
      }
    }
  }
}
```

{: .note }
Restart Zed after saving the configuration for changes to take effect.

---

## OpenCode

{: .note }
The configuration below uses Kiro models (works in both modes). In full deployment, you can add direct provider models using the `provider/model` prefix — see [Model Naming](#model-naming).

### Tested model limits

| Model | Context Window | Max Output |
|:------|:---------------|:-----------|
| Opus 4.6 | ~195K tokens | Unknown |
| Sonnet 4.6 | ~195K tokens | Unknown |
| Haiku 4.5 | ~195K tokens | Unknown |

{: .note }
Thinking mode is supported. The standard output token limit is 8192 tokens when thinking is not enabled.

### Configuration

Create or edit `~/.config/opencode/opencode.json`:

```json
{
  "provider": {
    "kiro": {
      "name": "Kiro Gateway",
      "type": "anthropic",
      "api_key": "YOUR_API_KEY",
      "url": "https://gateway.example.com",
      "models": {
        "auto": {
          "name": "Auto (Gateway Default)",
          "max_tokens": 8192,
          "context_window": 195000,
          "supports_reasoning": true,
          "reasoning": {
            "budget_tokens": 10000
          }
        },
        "haiku-4.5": {
          "name": "Claude Haiku 4.5",
          "max_tokens": 8192,
          "context_window": 195000,
          "supports_reasoning": true,
          "reasoning": {
            "budget_tokens": 10000
          }
        },
        "sonnet-4.6": {
          "name": "Claude Sonnet 4.6",
          "max_tokens": 8192,
          "context_window": 195000,
          "supports_reasoning": true,
          "reasoning": {
            "budget_tokens": 10000
          }
        },
        "opus-4.6": {
          "name": "Claude Opus 4.6",
          "max_tokens": 8192,
          "context_window": 195000,
          "supports_reasoning": true,
          "reasoning": {
            "budget_tokens": 10000
          }
        },
        "sonnet-4.6-thinking": {
          "name": "Claude Sonnet 4.6 (Thinking)",
          "max_tokens": 16000,
          "context_window": 195000,
          "supports_reasoning": true,
          "reasoning": {
            "budget_tokens": 10000
          }
        },
        "opus-4.6-thinking": {
          "name": "Claude Opus 4.6 (Thinking)",
          "max_tokens": 16000,
          "context_window": 195000,
          "supports_reasoning": true,
          "reasoning": {
            "budget_tokens": 10000
          }
        }
      }
    }
  }
}
```

---

## Cursor / VS Code

1. Open settings and find the AI provider configuration.
2. Set the API base URL to:
   ```
   https://your-domain.com/v1
   ```
3. Enter your personal API key from the Web UI.
4. Select any supported Claude model.

---

## OpenAI Python SDK

The gateway's `/v1/chat/completions` endpoint is OpenAI-compatible, so the standard SDK works directly:

```python
from openai import OpenAI

client = OpenAI(
    base_url="https://your-domain.com/v1",
    api_key="YOUR_API_KEY",
)

response = client.chat.completions.create(
    model="claude-sonnet-4",
    messages=[{"role": "user", "content": "Hello!"}],
)

print(response.choices[0].message.content)
```

---

## Anthropic Python SDK

The gateway's `/v1/messages` endpoint is Anthropic-compatible:

```python
import anthropic

client = anthropic.Anthropic(
    base_url="https://your-domain.com",
    api_key="YOUR_API_KEY",
)

message = client.messages.create(
    model="claude-sonnet-4",
    max_tokens=1024,
    messages=[{"role": "user", "content": "Hello!"}],
)

print(message.content[0].text)
```

---

## Model Naming

### Kiro pipeline (default)

Use Claude model names directly. The gateway normalizes variants automatically:

| You Send | Gateway Resolves |
|----------|-----------------|
| `claude-sonnet-4-6` | `claude-sonnet-4.6` |
| `claude-3-7-sonnet-20250219` | `claude-3.7-sonnet` |
| `claude-haiku-4-5-latest` | `claude-haiku-4.5` |
| `auto` | Gateway picks the best available model |

### Direct providers (full deployment only)

Prefix with the provider name to bypass Kiro and route to a direct API:

| Model String | Routes To |
|-------------|-----------|
| `anthropic/claude-opus-4-6` | Anthropic API directly |
| `openai_codex/gpt-4` | OpenAI Codex |
| `qwen/qwen-coder` | Qwen |
| `copilot/gpt-4` | GitHub Copilot |

Direct providers require per-user OAuth tokens configured in the Web UI **Providers** page. Without OAuth tokens, requests fall back to Kiro automatically.

---

## Known Limitations

### Web Search in Claude Code

Web search does not work through the proxy. The Kiro backend does not support the `tool_use` / `tool_result` round-trip that Claude Code's built-in web search relies on.

**Workaround:** Use local MCP servers for web access instead. Add the following to `~/.claude.json`:

```json
{
  "mcpServers": {
    "mcp-server-fetch": {
      "command": "uvx",
      "args": ["mcp-server-fetch"]
    },
    "exa-mcp-server": {
      "command": "npx",
      "args": ["-y", "exa-mcp-server"],
      "env": {
        "EXA_API_KEY": "YOUR_EXA_API_KEY"
      }
    }
  }
}
```

| MCP Server | Description |
|:-----------|:------------|
| `mcp-server-fetch` | Fetches and extracts content from URLs |
| `exa-mcp-server` | AI-powered web search via the [Exa](https://exa.ai) API (requires `EXA_API_KEY`) |
