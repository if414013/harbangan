# Document Writer Memory

## Documentation Drift Audit (2026-03-21)
- See `drift-audit-findings.md` for full consolidated report
- 7 major drift, 5 needs update, 4 accurate out of 16 gh-pages docs
- 6 cross-cutting issues: Gemini removed, AppState refactored, password auth undocumented, OpenAI→OpenAI Codex, Google OAuth via Web UI not env vars, port confusion (8000 vs 9999)

## Key Source Code Facts
- AppState defined in `backend/src/routes/state.rs` (NOT `routes/mod.rs` as some docs claim)
- ProviderId enum: Kiro, Anthropic, OpenAICodex, Copilot, Qwen, Custom (NO Gemini)
- Providers stored as `providers: ProviderMap` (not individual fields)
- Session cookie: `SameSite=Strict` (not Lax)
- Streaming parser: `SseParser` with text-based JSON extraction (NOT binary AWS Event Stream)
- Google OAuth configured via Web UI admin panel, NOT env vars
- Password auth: `web_ui/password_auth.rs` (802 lines) with TOTP 2FA, rate limiting
- Default port: 8000 in code; docker-compose overrides to 9999
- Frontend runs Nginx on internal port 80, mapped to host 5173
- Qwen URL: `dashscope-intl.aliyuncs.com/compatible-mode` (not `chat.qwen.ai`)

## gh-pages Documentation Structure
- Location: `gh-pages/docs/`
- 16 doc files total
- Jekyll site with `_config.yml`
- Architecture subdirectory: authentication.md, converters.md, converter-routing-summary.md, index.md, request-flow.md, streaming.md
- Top-level: api-reference.md, client-setup.md, configuration.md, deployment.md, getting-started.md, modules.md, quickstart.md, research-notes.md, troubleshooting.md, web-ui.md

## Writing Patterns
- Always verify claims against source code before documenting
- Check `routes/state.rs` for AppState, `providers/types.rs` for ProviderId enum
- Check `docker-compose.yml` AND `config.rs` for port/config — they may differ
- Check `.env.example` for actual env var support (not just what docs claim)
