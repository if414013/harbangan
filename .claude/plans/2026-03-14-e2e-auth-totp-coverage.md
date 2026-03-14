# E2E Auth: Password + TOTP Login & Coverage Expansion

## Phase 0: Ops (leader, sequential before team)

- Purge DB: `docker compose exec db psql -U postgres -d kiro_gateway -c "DELETE FROM totp_recovery_codes; DELETE FROM pending_2fa_logins; DELETE FROM sessions; DELETE FROM api_keys; DELETE FROM users;"`
- Generate TOTP secret, add `INITIAL_ADMIN_TOTP_SECRET=<base32>` to root `.env`
- Add `INITIAL_ADMIN_TOTP_SECRET: ${INITIAL_ADMIN_TOTP_SECRET:-}` to `docker-compose.yml` backend env (~line 39)
- `docker compose up -d --force-recreate backend` — re-seeds admin with TOTP
- Verify: `docker compose logs backend | grep "TOTP pre-configured"`

## Phase 1: E2E Infrastructure (leader, before team)

- `cd e2e-tests && npm install otpauth`
- **`e2e-tests/playwright.config.ts`** — change `dotenv.config()` to `dotenv.config({ path: '../.env' })` to read `INITIAL_ADMIN_EMAIL`, `INITIAL_ADMIN_PASSWORD`, `INITIAL_ADMIN_TOTP_SECRET` from root `.env`
- **`e2e-tests/helpers/auth.ts`** (new) — export `adminLogin(baseUrl)`: POST `/auth/login` → POST `/auth/login/2fa` with `otpauth` TOTP code, return Playwright `StorageState`
- **`e2e-tests/global-setup.ts`** — rewrite: call `adminLogin()`, write `.auth/session.json`; if env vars missing, fall back to existing session file

## Phase 2: Team Spawn (TeamCreate + Agent with team_name)

Use `TeamCreate` then spawn 3 agents from `.claude/agents/` with `team_name` param:

### Task 1 — backend-qa: `e2e-tests/specs/api/password-auth.spec.ts`
- Un-fixme 13 stubs, fix endpoint URLs, implement using `request` fixture + `otpauth`
- Tests: login, 2FA, recovery codes, password change, admin user CRUD

### Task 2 — frontend-qa: 3 UI spec files (24 stubs total)
- `specs/ui/password-login.spec.ts` (9) — password+2FA login flow
- `specs/ui/totp-setup.spec.ts` (8) — QR code, verify, recovery codes
- `specs/ui/password-change.spec.ts` (7) — change/set password flow

### Task 3 — document-writer: documentation updates
- Update CLAUDE.md E2E section (automated password+TOTP auth, no manual `test:setup`)
- Update `.env.example` comments for `INITIAL_ADMIN_TOTP_SECRET`

Tasks 1 & 2 run in parallel (no file overlap). Task 3 runs in parallel with both.

## Verification

```bash
cd e2e-tests && npm test
```
