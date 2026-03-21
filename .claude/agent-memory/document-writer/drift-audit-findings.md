# Drift Audit Findings (2026-03-21)

## Summary
- 16 gh-pages docs audited against source code
- 7 major drift, 5 needs update, 4 accurate

## Major Drift Files
1. `architecture/request-flow.md` — Gemini refs, missing Step 3, OpenAI naming, route structure
2. `architecture/index.md` — Gemini in diagrams, stale AppState, port
3. `modules.md` — nonexistent gemini.rs, wrong AppState, 8+ missing modules, 2 missing converters
4. `web-ui.md` — "no password login" (line 89), missing Providers/Usage/TotpSetup/PasswordChange pages
5. `architecture/streaming.md` — fabricated binary parser, wrong function names, missing error event
6. `configuration.md` — proxy-only provider support wrong, Google OAuth env vars wrong, missing env vars
7. `getting-started.md` — Google OAuth env var instructions wrong, contradictory setup, wrong port

## Needs Update Files
8. `api-reference.md` — 4 phantom endpoints, 15 undocumented endpoints
9. `architecture/authentication.md` — SameSite Lax→Strict, missing rate limiting/sliding sessions
10. `quickstart.md` — port 8000→9999, Google OAuth env vars
11. `research-notes.md` — 3 of 6 providers, wrong Qwen URL
12. `architecture/converters.md` — 2 missing modules, direct provider conversion undocumented

## Accurate Files
13. deployment.md (minor: port mapping 5173:5173→5173:80, missing tables)
14. client-setup.md
15. troubleshooting.md
16. converter-routing-summary.md

## P0 Fixes (Critical)
1. Google OAuth env var instructions (3 docs)
2. Streaming parser architecture (fabricated)
3. "No password login" claim (web-ui.md)
4. Proxy-only provider support (configuration.md)

## P1 Fixes (High)
5. Port inconsistencies (multiple docs)
6. AppState fields (modules.md, index.md)
7. Gemini references (4 docs)
8. API reference gaps (api-reference.md)
9. Session cookie SameSite (authentication.md)
