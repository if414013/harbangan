<div align="center">

# 🦀 rkgw — Rust Kiro Gateway

**High-performance proxy gateway for Kiro API (AWS CodeWhisperer)**

[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)
[![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)](https://www.rust-lang.org/)

_A Rust rewrite of [kiro-gateway](https://github.com/jwadow/kiro-gateway) — Use Claude models through any OpenAI or Anthropic compatible tool_

[Features](#-features) • [Quick Start](#-quick-start) • [Configuration](#-configuration) • [API Reference](#-api-reference)

</div>

---

## 🙏 Attribution

This project is a Rust rewrite of the original [kiro-gateway](https://github.com/jwadow/kiro-gateway) by [@Jwadow](https://github.com/jwadow). The original project is written in Python using FastAPI.

**Why Rust?**

- ⚡ Faster startup time and lower memory footprint
- 🔒 Memory safety without garbage collection
- 📦 Single binary deployment — no runtime dependencies

---

## 🤖 Supported Models

| Model                    | Description                                               |
| ------------------------ | --------------------------------------------------------- |
| 🧠 **Claude Opus 4.6**   | Latest flagship. 1M context (beta), 128K output, adaptive thinking |
| 🧠 **Claude Opus 4.5**   | Most powerful. Complex reasoning, deep analysis, research |
| 🚀 **Claude Sonnet 4.5** | Balanced. Coding, writing, general-purpose                |
| ⚡ **Claude Haiku 4.5**  | Lightning fast. Quick responses, simple tasks             |
| 📦 **Claude Sonnet 4**   | Previous generation. Reliable for most use cases          |
| 📦 **Claude 3.7 Sonnet** | Legacy model. Backward compatibility                      |

> 💡 **Smart Model Resolution:** Use any model name format — `claude-sonnet-4-5`, `claude-sonnet-4.5`, or versioned names like `claude-sonnet-4-5-20250929`. The gateway normalizes them automatically.

---

## ✨ Features

| Feature                         | Description                                                                                |
| ------------------------------- | ------------------------------------------------------------------------------------------ |
| 🔌 **OpenAI-compatible API**    | Works with any OpenAI-compatible tool                                                      |
| 🔌 **Anthropic-compatible API** | Native `/v1/messages` endpoint                                                             |
| 🧠 **Extended Thinking**        | Reasoning support                                                                          |
| 👁️ **Vision Support**           | Send images to model                                                                       |
| 🛠️ **Tool Calling**             | Function calling support                                                                   |
| 💬 **Full message history**     | Complete conversation context                                                              |
| 📡 **Streaming**                | Full SSE streaming support                                                                 |
| 🔄 **Retry Logic**              | Automatic retries on errors                                                                |
| 🔐 **Smart token management**   | Automatic refresh before expiration                                                        |
| 🔒 **HTTPS / TLS**              | Built-in TLS support with auto-generated self-signed certificates or custom cert/key files |
| 📊 **Live Dashboard**           | Real-time TUI with metrics, logs, and token usage (toggle with `--dashboard` or press `d`) |

---

## 🚀 Quick Start

### Prerequisites

- [Kiro CLI](https://kiro.dev/cli/) installed and logged in with AWS SSO (Builder ID)

### Installation via Homebrew (Recommended)

```bash
# Add the tap
brew tap if414013/tvps

# Install kiro-gateway
brew install kiro-gateway

# Run (interactive setup on first run)
kiro-gateway
```

### Installation from Source

Requires Rust 1.75+ (install via [rustup](https://rustup.rs/))

```bash
# Clone the repository
git clone https://github.com/if414013/rkgw.git
cd rkgw

# Build release binary
cargo build --release

# Run
./target/release/kiro-gateway
```

The server will be available at `http://localhost:8000`.

🔒 **Default Binding:** The gateway defaults to `127.0.0.1` (localhost only) for security. To allow network access, use `--host 0.0.0.0 --tls`. See the [Security](#-security) section for details.

To enable HTTPS with an auto-generated self-signed certificate:

```bash
kiro-gateway --tls
```

The server will be available at `https://localhost:8000`.

> **Note:** The auto-generated self-signed certificate only covers `localhost`, `127.0.0.1`, and `::1`. If you bind to a network address with `--host 0.0.0.0` and clients connect via a LAN IP or hostname, they will see certificate name mismatch errors. For network access, provide your own certificate with `--tls-cert` and `--tls-key` that includes the appropriate SANs.

---

## ⚙️ Configuration

On first run, `kiro-gateway` will guide you through an interactive setup if no `.env` file is found. It will:

- Prompt for a password to protect your gateway
- Auto-detect your kiro-cli database location
- Let you choose the AWS region
- Optionally save the configuration to a `.env` file

### Manual Configuration

Create a `.env` file in the project root:

```env
# Required - Path to kiro-cli SQLite database
KIRO_CLI_DB_FILE="~/Library/Application Support/kiro-cli/data.sqlite3"

# Password to protect YOUR proxy server
PROXY_API_KEY="my-super-secret-password-123"

# Optional
KIRO_REGION="us-east-1"

# Server binding (optional, defaults to 127.0.0.1:8000 for local-only access)
# For network access: SERVER_HOST=0.0.0.0 (requires --tls flag)
# SERVER_HOST=127.0.0.1
# SERVER_PORT=8000

# TLS / HTTPS (optional)
# TLS_ENABLED=true
# TLS_CERT=/path/to/cert.pem
# TLS_KEY=/path/to/key.pem
```

### HTTPS / TLS

The gateway supports HTTPS out of the box with three usage modes:

**1. Auto-generated self-signed certificate** — just add `--tls`:

```bash
kiro-gateway --tls
```

A self-signed certificate is generated automatically and saved to `~/.kiro-gateway/tls/` for reuse across restarts. Certificates are valid for 365 days and are automatically regenerated before expiry.

> ⚠️ Self-signed certificates are not trusted by browsers and clients by default. Use `--tls-cert` and `--tls-key` for production deployments.

**2. Custom certificate** — provide your own PEM files:

```bash
kiro-gateway --tls --tls-cert /path/to/cert.pem --tls-key /path/to/key.pem
```

**3. Auto-enable via cert/key** — TLS activates automatically when both paths are provided:

```bash
kiro-gateway --tls-cert /path/to/cert.pem --tls-key /path/to/key.pem
```

All options also work via environment variables: `TLS_ENABLED`, `TLS_CERT`, `TLS_KEY`.

| CLI Flag       | Environment Variable | Description                                      |
| -------------- | -------------------- | ------------------------------------------------ |
| `--tls`        | `TLS_ENABLED`        | Enable HTTPS (auto-generates cert if none given) |
| `--tls-cert`   | `TLS_CERT`           | Path to TLS certificate file (PEM format)        |
| `--tls-key`    | `TLS_KEY`            | Path to TLS private key file (PEM format)        |

### Kiro CLI Database Locations

The gateway auto-detects the kiro-cli database from these common locations:

| Platform        | Path                                                  |
| --------------- | ----------------------------------------------------- |
| **macOS**       | `~/Library/Application Support/kiro-cli/data.sqlite3` |
| **Linux**       | `~/.local/share/kiro-cli/data.sqlite3`                |
| **macOS (old)** | `~/Library/Application Support/kiro-cli/data.db`      |
| **Legacy**      | `~/.kiro/data.db`                                     |

The gateway reads credentials from the kiro-cli SQLite database and automatically refreshes tokens before expiration.

---

## 🔒 Security

### Server Binding

**Local-only use (127.0.0.1 - Recommended for personal use)**

When you bind to `127.0.0.1` (localhost), the gateway is only accessible from your local machine. This is the safest option for personal development and testing.

```bash
kiro-gateway --host 127.0.0.1
```

✅ **Secure by Default:** The gateway defaults to `127.0.0.1` (localhost only). This is the recommended setting for personal use.

**Network-accessible use (0.0.0.0 - Use with caution)**

Binding to `0.0.0.0` exposes the gateway to all network interfaces, making it accessible from other devices on your local network or potentially the internet if your firewall allows it.

```bash
kiro-gateway --host 0.0.0.0
```

⚠️ **Security implications:**
- Anyone on your network can access the gateway if they know your IP address and port
- Traffic is unencrypted by default (HTTP), exposing your API key and data in plain text
- The `PROXY_API_KEY` is your only protection against unauthorized access
- If port-forwarded or exposed to the internet, anyone can attempt to access your gateway

**If you must use 0.0.0.0, always enable TLS:**

```bash
kiro-gateway --host 0.0.0.0 --tls
```

This encrypts all traffic between clients and the gateway, protecting your credentials and data from network sniffing.

🛡️ **Enforcement:** The gateway enforces TLS for non-localhost bindings. If you attempt to start with `0.0.0.0` or another non-localhost address without TLS, the gateway will refuse to start with an error:

> Error: TLS is required when binding to non-localhost addresses (current: 0.0.0.0). Either enable TLS with --tls flag, or bind to localhost with --host 127.0.0.1

This validation prevents accidental exposure of unencrypted traffic to your network.

### Trusting Self-Signed Certificates (macOS)

When using `--tls` without custom certificates, the gateway generates a self-signed certificate saved to `~/.kiro-gateway/tls/`. Clients will not trust this certificate by default, resulting in connection errors.

**GUI Method (Keychain Access):**

1. Open **Keychain Access** application
2. Select the **System** keychain in the left sidebar
3. Drag the certificate file from `~/.kiro-gateway/tls/` into the Keychain Access window
4. Double-click the imported certificate entry
5. Expand the **Trust** section
6. Set **When using this certificate** to **Always Trust**
7. Close the dialog and enter your admin password when prompted

**CLI Method (Terminal):**

```bash
sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain ~/.kiro-gateway/tls/cert.pem
```

Replace `cert.pem` with the actual certificate filename in your `~/.kiro-gateway/tls/` directory.

> 💡 **Tip:** After trusting the certificate, restart your client application to ensure it picks up the new trust settings. For curl, you can also use the `-k` flag to skip certificate verification during testing.

---

## 🏗️ Architecture

<details>
<summary>View architecture documentation</summary>

For detailed architecture documentation including component diagrams, data flows, and implementation details, see **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)**

</details>

---

## 💡 API Usage Examples

<details>
<summary>View API usage examples</summary>

### OpenAI API

```bash
curl http://localhost:8000/v1/chat/completions \
  -H "Authorization: Bearer my-super-secret-password-123" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4-5",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

### Anthropic API

```bash
curl http://localhost:8000/v1/messages \
  -H "x-api-key: my-super-secret-password-123" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4-5",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

> 💡 **Using HTTPS?** Replace `http://` with `https://` in the URLs above. If using a self-signed certificate, add `-k` to curl to skip certificate verification.

</details>

---

## 🖥️ OpenCode Setup

<details>
<summary>View OpenCode configuration</summary>

To use this gateway with [OpenCode](https://opencode.ai), add the following provider configuration to your global config file at `~/.config/opencode/opencode.json`. This makes the Kiro provider available across all your projects.

For more details on OpenCode configuration, see the [OpenCode Config Documentation](https://opencode.ai/docs/config/).

https://github.com/user-attachments/assets/7a3ab9ba-15b4-4b96-95df-158602ed08b0

```json
{
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "kiro": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "Kiro Proxy",
      "options": {
        "baseURL": "http://127.0.0.1:8000/v1",
        "apiKey": "your-proxy-api-key"
      },
      "auto": {
        "name": "Auto"
      },
      "claude-haiku-4.5": {
        "name": "Claude Haiku 4.5",
        "limit": {
          "context": 180000, // NOTE: 0.9x limit for earlier auto compaction
          "output": 64000
        },
        "modalities": {
          "input": ["text", "image"],
          "output": ["text"]
        }
      },
      "claude-opus-4.5": {
        "name": "Claude Opus 4.5",
        "limit": {
          "context": 180000, // NOTE: 0.9x limit for earlier auto compaction
          "output": 64000
        },
        "modalities": {
          "input": ["text", "image"],
          "output": ["text"]
        },
        "variants": {
          "low": {
            "thinkingConfig": {
              "thinkingBudget": 4096
            }
          },
          "max": {
            "thinkingConfig": {
              "thinkingBudget": 32768
            }
          }
        }
      },
      "claude-opus-4.6": {
        "name": "Claude Opus 4.6",
        "limit": {
          "context": 980000, // NOTE: 0.98x limit for earlier auto compaction
          "output": 128000
        },
        "modalities": {
          "input": ["text", "image"],
          "output": ["text"]
        },
        "variants": {
          "low": {
            "thinkingConfig": { "type": "adaptive", "effort": "low" }
          },
          "max": {
            "thinkingConfig": { "type": "adaptive", "effort": "max" }
          }
        }
      },
      "claude-sonnet-4": {
        "name": "Claude Sonnet 4",
        "limit": {
          "context": 180000, // NOTE: 0.9x limit for earlier auto compaction
          "output": 64000
        },
        "modalities": {
          "input": ["text", "image"],
          "output": ["text"]
        }
      },
      "claude-sonnet-4.5": {
        "name": "Claude Sonnet 4.5",
        "limit": {
          "context": 180000, // NOTE: 0.9x limit for earlier auto compaction
          "output": 64000
        },
        "modalities": {
          "input": ["text", "image"],
          "output": ["text"]
        }
      }
    }
  }
}
```

> **Note:** Replace `your-proxy-api-key` with the value of your `PROXY_API_KEY` environment variable. The default port is `8000`, but can be changed via the interactive setup prompt or `SERVER_PORT` in your `.env` file. If using HTTPS, change `http://` to `https://` in the `baseURL`.

</details>

---

## 🖥️ Claude Code CLI Setup

<details>
<summary>View Claude Code CLI configuration</summary>

https://github.com/user-attachments/assets/f404096e-b326-41e5-a4b3-3f94a73d2ece

To use this gateway with [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code), set the following environment variables:

**One-liner:**

```bash
ANTHROPIC_BASE_URL=http://127.0.0.1:8000 ANTHROPIC_AUTH_TOKEN=your-proxy-api-key CLAUDE_CODE_ENABLE_TELEMETRY=0 DISABLE_PROMPT_CACHING=1 DISABLE_NON_ESSENTIAL_MODEL_CALLS=1 CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 claude
```

> 💡 **Using HTTPS?** Replace `http://` with `https://` in `ANTHROPIC_BASE_URL`. If using a self-signed certificate, prefer trusting the cert in your OS keychain or pointing `NODE_EXTRA_CA_CERTS` to your cert file. As a last-resort local-only workaround you may temporarily set `NODE_TLS_REJECT_UNAUTHORIZED=0` for the `claude` command, but **never** use this in production or add it to your shell profile.

**Or add to your shell profile** (`~/.bashrc`, `~/.zshrc`, etc.):

```bash
export ANTHROPIC_BASE_URL=http://127.0.0.1:8000
export ANTHROPIC_AUTH_TOKEN=your-proxy-api-key
export CLAUDE_CODE_ENABLE_TELEMETRY=0
export DISABLE_PROMPT_CACHING=1
export DISABLE_NON_ESSENTIAL_MODEL_CALLS=1
export CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1
```

| Variable                                   | Description                                       |
| ------------------------------------------ | ------------------------------------------------- |
| `ANTHROPIC_BASE_URL`                       | Points Claude Code to your gateway                |
| `ANTHROPIC_AUTH_TOKEN`                     | Your `PROXY_API_KEY` value                        |
| `CLAUDE_CODE_ENABLE_TELEMETRY`             | Disable telemetry                                 |
| `DISABLE_PROMPT_CACHING`                   | Disable prompt caching (not supported by gateway) |
| `DISABLE_NON_ESSENTIAL_MODEL_CALLS`        | Reduce unnecessary API calls                      |
| `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` | Disable non-essential network traffic             |

> **Note:** Replace `your-proxy-api-key` with the value of your `PROXY_API_KEY`. The default port is `8000`, but can be changed via the interactive setup prompt or `SERVER_PORT` in your `.env` file. If using HTTPS, change `http://` to `https://` in `ANTHROPIC_BASE_URL`.

</details>

---

## 🖥️ Zed Editor Setup

<details>
<summary>View Zed Editor configuration</summary>

To use this gateway with the [Zed Editor](https://zed.dev/)'s ACP Claude Agent, add the following configuration to your Zed settings file at `~/.config/zed/settings.json`:

```json
{
  "agent_servers": {
    "claude": {
      "env": {
        "ANTHROPIC_BASE_URL": "http://127.0.0.1:8000",
        "ANTHROPIC_AUTH_TOKEN": "your-proxy-api-key",
        "CLAUDE_CODE_ENABLE_TELEMETRY": "0",
        "DISABLE_PROMPT_CACHING": "1",
        "DISABLE_NON_ESSENTIAL_MODEL_CALLS": "1",
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1"
      }
    }
  }
}
```

> **Note:** Replace `your-proxy-api-key` with the value of your `PROXY_API_KEY`. The default port is `8000`, but can be changed via the interactive setup prompt or `SERVER_PORT` in your `.env` file. If using HTTPS, change `http://` to `https://` in `ANTHROPIC_BASE_URL`. After updating the settings, restart Zed for the changes to take effect.

</details>

---

## 🔧 Building

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Run tests
cargo test

# Run benchmarks
cargo bench
```

---

## 📜 License

This project is licensed under the **GNU Affero General Public License v3.0 (AGPL-3.0)**.

This means:

- ✅ You can use, modify, and distribute this software
- ✅ You can use it for commercial purposes
- ⚠️ **You must disclose source code** when you distribute the software
- ⚠️ **Network use is distribution** — if you run a modified version on a server, you must make the source code available
- ⚠️ Modifications must be released under the same license

See the [LICENSE](LICENSE) file for the full license text.

### Contributor License Agreement (CLA)

By submitting a contribution to this project, you agree to the terms of our [Contributor License Agreement (CLA)](CLA.md).

---

## ⚠️ Disclaimer

This project is not affiliated with, endorsed by, or sponsored by Amazon Web Services (AWS), Anthropic, or Kiro IDE. Use at your own risk and in compliance with the terms of service of the underlying APIs.

---

<div align="center">

**[⬆ Back to Top](#-rkgw--rust-kiro-gateway)**

</div>
