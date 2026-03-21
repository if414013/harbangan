# Frontend QA Agent Memory

## Playwright Setup
- All E2E tests in `e2e-tests/` at project root (NOT `frontend/e2e/`)
- Playwright 1.52+ via `@playwright/test` in `e2e-tests/node_modules`
- Main config: `e2e-tests/playwright.config.ts` — 5 projects: api, ui-public, ui-authenticated, api-mutating, ui-admin
- Setup-mode config: `e2e-tests/playwright.setup-mode.config.ts` (no globalSetup, empty DB)
- TOTP debug config: `e2e-tests/totp-debug.config.ts` (backend direct, no proxy)
- Selectors centralized in `e2e-tests/helpers/selectors.ts`
- Test output: `e2e-tests/test-results/`
- URLs: GATEWAY_URL defaults `http://localhost:9999`, BASE_UI_URL defaults `http://localhost:5173/_ui`

## Auth
- Global setup auto-authenticates via password + TOTP using `INITIAL_ADMIN_*` env vars from `../.env`
- Session saved to `e2e-tests/.auth/session.json` (kgw_session + csrf_token cookies)
- `helpers/auth.ts` — global-setup login (raw fetch, returns StorageState)
- `helpers/csrf.ts` — API test login (Playwright APIRequestContext, returns csrfToken)
- Fallback: `npm run test:setup` for interactive session capture via playwright codegen
- SDK tests use `helpers/sdk-clients.ts` (OpenAI + Anthropic client factories)

## Test Commands
- `npm test` — all 5 projects
- `npm run test:api` — api project only
- `npm run test:ui` — ui-public + ui-authenticated + ui-admin
- `npm run test:ui:public` / `test:ui:auth` / `test:ui:admin` — individual UI projects
- `npm run test:setup-mode` — setup-only mode (separate config)
- `npm run test:setup` — interactive session capture (manual fallback)

## Key Patterns
- **colorScheme emulation**: Playwright Chromium defaults to `prefers-color-scheme: light`. Use `browser.newContext({ colorScheme: 'dark' })` for dark mode.
- **baseURL with browser.newContext()**: Pass `baseURL` explicitly when creating contexts manually.
- **Public pages**: Login (`/_ui/login`) testable without auth. Others redirect to login.
- **Theme persistence**: localStorage key `harbangan-theme`.
- **Project ordering**: api-mutating depends on [api, ui-public, ui-authenticated]; ui-admin depends on [api-mutating]

## Test Files (44 total: 22 API + 22 UI)
- See `e2e-tests/specs/api/` and `e2e-tests/specs/ui/` for full list
- `theme-toggle.spec.ts` — Light/dark mode visual verification, persistence
- `provider-oauth.spec.ts` — Provider OAuth on Profile page (28 tests)
- SDK tests: `sdk-anthropic-chat`, `sdk-openai-chat`, `sdk-tool-use`, `sdk-extended-thinking`, etc.

## Mock Gotchas
- **ApiKeyManager** expects `{ keys: [] }` not `[]` from `/_ui/api/keys`
- **Profile page** requires 5 mocks for full render: `auth/me`, `status`, `providers/status`, `kiro/status`, `keys`

## Project Structure Notes
- CRT aesthetic: scanlines (body::before) and vignette (body::after) in dark mode
- CSS variables in `frontend/src/styles/variables.css` with `[data-theme="light"]` overrides
