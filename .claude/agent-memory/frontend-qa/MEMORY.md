# Frontend QA Agent Memory

## Playwright Setup
- Playwright 1.58.2 installed in `frontend/node_modules`
- Main config: `frontend/e2e/playwright.config.ts` (baseURL: `https://localhost/_ui/`, 3 projects: public, authenticated, admin)
- Dev config: `frontend/e2e/playwright.dev.config.ts` (baseURL: `http://localhost:5173/_ui/`, no auth required)
- Selectors centralized in `frontend/e2e/helpers/selectors.ts`
- Screenshots/artifacts saved to `.playwright-mcp/` (gitignored)

## Key Patterns
- **colorScheme emulation**: Playwright Chromium defaults to `prefers-color-scheme: light` regardless of OS setting. Use `browser.newContext({ colorScheme: 'dark' })` to emulate dark mode. This matters because ThemeProvider reads `prefers-color-scheme` when no localStorage value exists.
- **baseURL with browser.newContext()**: When creating contexts manually (e.g., for colorScheme), pass `baseURL` explicitly since it won't inherit from config's `use` block.
- **Public pages**: Login page (`/_ui/login`) is testable without auth. All other pages redirect to login.
- **Theme persistence**: ThemeProvider uses localStorage key `rkgw-theme`. Tests that set localStorage persist within the same browser context across navigations.

## Test Files
- `theme-toggle.spec.ts` — Light/dark mode visual verification, persistence, CSS variable checks
- `provider-oauth.spec.ts` — Provider OAuth on Profile page (28 tests: structure, status badges, connect modal, polling, disconnect, removal verification)
- See also: `login.spec.ts`, `auth-redirect.spec.ts` (public project), `dashboard.spec.ts`, `profile.spec.ts`, etc.

## Mock Gotchas
- **ApiKeyManager** expects `{ keys: [] }` not `[]` from `/_ui/api/keys` — bare array crashes React
- **Profile page** requires 5 mocks for full render: `auth/me`, `status`, `providers/status`, `kiro/status`, `keys`

## Project Structure Notes
- CRT aesthetic: scanlines (body::before) and vignette (body::after) visible in dark mode, hidden via `display: none` in light mode
- CSS variables defined in `frontend/src/styles/variables.css` with `[data-theme="light"]` overrides
- Light mode component overrides at bottom of `components.css` and `global.css`
