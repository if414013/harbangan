<div align="center">

# Harbangan

**Multi-user proxy gateway for AI providers**

[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)
[![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)](https://www.rust-lang.org/)

</div>

---

## Why "Harbangan"?

In Batak Toba culture, the *harbangan* is the gate of the traditional house — but it is far more than a physical structure. It is a threshold between worlds: the ordered, protected space of family and community on one side, and the open, unpredictable world on the other. In Batak cosmology, to cross a threshold is to navigate between states of being.

This gateway embodies the same philosophy:

- **Cosmic boundary** — The *harbangan* separates the three realms of Batak cosmology. This gateway sits at the boundary between your client code and multiple provider backends (Kiro, Copilot, Qwen), translating between OpenAI and Anthropic formats on either side.
- **Guardian of social order** — The Batak gate enforces *Dalihan Na Tolu*, the three-pillar kinship system that governs who may enter and how. Harbangan enforces multi-user RBAC: Google SSO, per-user API keys, admin/user roles, and domain allowlisting decide what passes through.
- **Ritual transition** — Crossing a *harbangan* signals a shift in status. Requests crossing this gateway undergo their own transformation: format conversion, content guardrails (CEL rules + AWS Bedrock), and MCP tool injection before reaching the other side.
- **Openness as moral virtue** — In Batak ethics, a gate that is always open signals generosity and communal spirit. This one is open source, and in proxy-only mode, a single container is all you need to open the gate.

> Further reading on Batak Toba philosophy: [Form and Meaning of Batak Toba House](https://repository.petra.ac.id/18044/1/Publikasi1_03007_4499.pdf) · [Dalihan Na Tolu: Vision of Integrity](https://journalppw.com/index.php/jpsp/article/download/12366/8016/14827) · [Batak Cultural Values](https://ojs.unimal.ac.id/mspr/article/download/10948/4863)

## Supported Models

| Model                 | Description                                           |
| --------------------- | ----------------------------------------------------- |
| **Claude Opus 4.6**   | Latest flagship. Adaptive thinking, complex reasoning |
| **Claude Sonnet 4.6** | Latest balanced. Coding, writing, general-purpose     |
| **Claude Haiku 4.5**  | Lightning fast. Quick responses, simple tasks         |

> **Smart Model Resolution:** Use any model name format — `claude-sonnet-4-6`, `claude-sonnet-4.6`, or versioned names like `claude-sonnet-4-20250514`. The gateway normalizes them automatically.

## Features

- OpenAI + Anthropic compatible APIs
- Real-time SSE streaming
- Extended thinking / reasoning
- Multi-user with Google SSO + per-user API keys
- MCP Gateway (external tool servers)
- Content Guardrails (AWS Bedrock)
- Web dashboard with real-time metrics
- Proxy-Only Mode (single container, no DB)
- Optional Datadog APM

## Quick Start

```bash
git clone https://github.com/if414013/harbangan.git && cd harbangan
cp .env.example .env  # edit with your Google OAuth creds
docker compose up -d  # start all services
```

Open `https://your-domain/_ui/` to complete setup.

> For proxy-only mode (no DB/SSO), see the [Quickstart guide](https://if414013.github.io/harbangan/docs/quickstart).

## Documentation

📖 **[Full Documentation](https://if414013.github.io/harbangan/)**

- [Getting Started](https://if414013.github.io/harbangan/docs/getting-started) — Setup walkthrough for both deployment modes
- [Quickstart](https://if414013.github.io/harbangan/docs/quickstart) — Running in under 5 minutes
- [Configuration](https://if414013.github.io/harbangan/docs/configuration) — Environment variables and runtime settings
- [API Reference](https://if414013.github.io/harbangan/docs/api-reference) — Endpoint documentation with examples
- [Architecture](https://if414013.github.io/harbangan/docs/architecture) — System design and component overview
- [Client Setup](https://if414013.github.io/harbangan/docs/client-setup) — Claude Code, Zed, OpenCode, and SDK configs
- [Deployment](https://if414013.github.io/harbangan/docs/deployment) — Production deployment guide
- [Troubleshooting](https://if414013.github.io/harbangan/docs/troubleshooting) — Common issues and solutions

## License

This project is licensed under the **GNU Affero General Public License v3.0 (AGPL-3.0)**.

- You can use, modify, and distribute this software
- **Network use is distribution** — if you run a modified version on a server, you must make the source code available
- Modifications must be released under the same license

See the [LICENSE](LICENSE) file for the full license text.

By submitting a contribution, you agree to the terms of our [Contributor License Agreement (CLA)](CLA.md).

## Disclaimer

This project is not affiliated with, endorsed by, or sponsored by Amazon Web Services (AWS), Anthropic, or Kiro IDE. Use at your own risk and in compliance with the terms of service of the underlying APIs.

---

<div align="center">

**[Back to Top](#harbangan)**

</div>
