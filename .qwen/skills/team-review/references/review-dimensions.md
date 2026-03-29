# Review Dimensions for Harbangan

Use this for `team-review --code`. The fixed reviewer dimensions always run, but each reviewer should consult only the relevant read-only domain advisors and should return an explicit no-op result if nothing in its dimension applies.

## Hybrid topology

Code review is dimension-led:

1. Spawn 5 fixed reviewers: security, performance, architecture, testing, accessibility.
2. Each fixed reviewer consults the relevant domain advisors from `.codex/agents/`.
3. The fixed reviewer owns the final finding text for its dimension.
4. Consolidation merges duplicates across dimensions and across consulted advisors.

Each code-review finding should include:

- `Dimension:`
- `Consulted domain agent(s):`
- `Location:`
- `Description:`
- `Impact:`
- `Recommended fix path:`

If a dimension has no relevant surface area, report `no findings in my dimension` instead of fabricating low-value commentary.

## Domain advisor mapping

- `rust-backend-engineer`: backend correctness, auth, API, streaming, middleware, guardrails, runtime behavior
- `react-frontend-engineer`: frontend correctness, UI architecture, API integration, accessibility surface
- `database-engineer`: schema, migration, query, and data integrity risk
- `devops-engineer`: compose, Docker, environment, deployment, operational risk
- `backend-qa`: backend test coverage and regression gaps
- `frontend-qa`: frontend/E2E/accessibility regression gaps
- `document-writer`: operator-facing docs, config communication, release-note or runbook implications when relevant

## Security

Focus on externally reachable risk first.

- API keys and tokens are never logged or stored in plaintext.
- SQL uses parameterized `sqlx` queries rather than string interpolation.
- Session and auth flows validate state, CSRF, and cookie attributes where applicable.
- User-controlled model or provider values are resolved and validated rather than blindly forwarded.
- CORS and forwarded headers are limited to expected inputs.
- Secrets stay in environment variables and out of committed files, Docker build args, and screenshots.

Hot areas:

- `backend/src/auth/`
- `backend/src/middleware/`
- `backend/src/web_ui/`
- `.env.example`
- `docker-compose*.yml`

Primary advisors:

- `rust-backend-engineer`
- `devops-engineer`
- `react-frontend-engineer` when browser/session/UI auth behavior is touched

## Performance

Look for hot-path inefficiencies and concurrency mistakes.

- No blocking work is held across `.await`.
- Shared state uses the expected async primitives and does not hold borrowed values across await points.
- Connection pools and HTTP clients are reused rather than recreated per request.
- Streaming and SSE flows flush incrementally instead of buffering whole responses.
- React paths avoid obvious unnecessary re-renders and expensive work in render.

Hot areas:

- `backend/src/streaming/`
- `backend/src/http_client.rs`
- `backend/src/cache.rs`
- `frontend/src/lib/useSSE.ts`

Primary advisors:

- `rust-backend-engineer`
- `database-engineer` when data access or migration behavior matters
- `react-frontend-engineer`
- `devops-engineer` when container/runtime settings affect performance

## Architecture

Check that changes fit existing boundaries and patterns.

- Shared types live in the right modules instead of being duplicated.
- New behavior respects existing backend and frontend module boundaries.
- Errors use the repo's existing patterns rather than ad hoc `unwrap` or silent failure.
- Middleware ordering and route integration are coherent.
- Model resolution goes through the resolver instead of hardcoded identifiers.
- Frontend work goes through `apiFetch` and existing styling/token patterns.

Hot areas:

- `backend/src/routes/`
- `backend/src/models/`
- `backend/src/error.rs`
- `frontend/src/lib/api.ts`
- `frontend/src/styles/`

Primary advisors:

- `rust-backend-engineer`
- `react-frontend-engineer`
- `database-engineer` when schema boundaries matter
- `devops-engineer` when deployment/config structure is part of the design

## Testing

Look for missing coverage, not just missing files.

- Critical behavior changes have direct tests.
- Edge cases and failure paths are exercised, especially for auth, converters, streaming, and config persistence.
- Tests are deterministic and assert concrete outcomes.
- Backend async tests use `#[tokio::test]` where needed.
- Frontend verification covers user-visible state changes, error states, and cleanup paths.

Hot areas:

- `backend/src/**`
- `frontend/src/**`
- existing test modules at the bottom of touched Rust files

Primary advisors:

- `backend-qa`
- `frontend-qa`

## Accessibility

Apply this when frontend/UI behavior changes.

- Interactive controls use semantic elements.
- Forms have labels and errors are visible and understandable.
- Keyboard navigation and focus treatment remain usable.
- Dynamic UI updates do not become screen-reader noise.
- Color is not the only indicator of state.
- The CRT theme still preserves readable contrast.

Hot areas:

- `frontend/src/pages/`
- `frontend/src/components/`
- `frontend/src/styles/components.css`

Primary advisors:

- `react-frontend-engineer`
- `frontend-qa`

## Severity guide

- `critical`: security breach, auth bypass, data loss, or complete feature failure
- `high`: likely user-facing breakage, major contract mismatch, or serious missing validation
- `medium`: real bug or coverage gap with partial impact
- `low`: minor issue with limited impact
- `info`: useful observation without immediate breakage

## Reporting rules

- Lead with concrete findings and cite locations.
- Skip style-only notes unless they hide a real risk.
- If multiple reviewers find the same issue, merge it into one finding.
- If multiple domain advisors informed one dimension finding, list them together in `Consulted domain agent(s):`.
- If a dimension finds nothing relevant, return a concise no-op result instead of filler commentary.
