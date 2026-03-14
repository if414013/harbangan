# Username/Password Auth + Mandatory 2FA

## Wave 1: Backend (rust-backend-engineer)

- `backend/Cargo.toml` — add `argon2 = "0.5"`, `totp-rs = { version = "5", features = ["qr", "gen_secret"] }`
- `backend/src/web_ui/config_db.rs` — add `migrate_to_v15()`: ALTER users ADD password_hash, totp_secret, totp_enabled, auth_method, must_change_password; CREATE totp_recovery_codes + pending_2fa_logins tables. Add DB methods: create_password_user, get_user_by_email_with_auth, update_password, enable_totp, disable_totp, store_recovery_codes, use_recovery_code, create/get/delete_pending_2fa, cleanup_expired_2fa, reset_user_2fa
- `backend/src/routes/state.rs` — extend `SessionInfo` with auth_method, totp_enabled, must_change_password fields. Add `login_rate_limiter: Arc<DashMap<String, (u32, Instant)>>` to AppState
- `backend/src/error.rs` — add InvalidCredentials (401), AccountLocked (429), TwoFactorRequired, TwoFactorSetupRequired variants
- **NEW** `backend/src/web_ui/password_auth.rs` — login_handler (POST /auth/login → validate password, return needs_2fa + login_token), login_2fa_handler (POST /auth/login/2fa → validate TOTP/recovery code, create session via `config_db.create_session()`), setup_2fa_handler, verify_2fa_handler, change_password_handler, admin_create_user_handler, admin_reset_password_handler. Reuse cookie helpers from google_auth.rs (make pub(crate)). Rate limit: 5 failures/15min per email
- `backend/src/web_ui/google_auth.rs` — extract `session_cookie()`/`csrf_cookie()` as pub(crate). Add auth_google_enabled/auth_password_enabled to `status` response. Guard `google_auth_redirect` with auth_google_enabled check. Extend session_middleware to populate new SessionInfo fields + enforce 2FA setup for password users (allow only /auth/2fa/*, /auth/logout, /auth/password/change)
- `backend/src/web_ui/session.rs` — add cleanup_expired_2fa to cleanup task
- `backend/src/web_ui/routes.rs` — register password_auth routes (public: /auth/login, /auth/login/2fa; session: /auth/2fa/setup, /auth/2fa/verify, /auth/password/change; admin: /admin/users/create, /admin/users/:id/reset-password). Add auth config keys to validate_config_field (boolean, at least one must stay true)
- `backend/src/web_ui/mod.rs` — add `pub mod password_auth;`
- `backend/src/main.rs` — on startup if no admin exists + INITIAL_ADMIN_EMAIL/INITIAL_ADMIN_PASSWORD env vars set, seed admin user with must_change_password=true

## Wave 2: Frontend (react-frontend-engineer)

- `frontend/src/lib/api.ts` — add login/2fa/totp/admin API functions + types. Extend User interface with auth_method
- `frontend/src/pages/Login.tsx` — rewrite as state machine: fetch status for enabled methods → show password form + Google button (or one) → 2FA code entry on needs_2fa → redirect on success
- **NEW** `frontend/src/pages/TotpSetup.tsx` — QR code display (from /auth/2fa/setup), verification input, recovery codes display with copy/download
- **NEW** `frontend/src/pages/PasswordChange.tsx` — forced password change form (current + new password)
- `frontend/src/components/SessionGate.tsx` — handle must_change_password → /change-password, totp_setup_required → /setup-2fa redirects
- `frontend/src/App.tsx` — add routes: /setup-2fa → TotpSetup, /change-password → PasswordChange
- `frontend/src/pages/Admin.tsx` — add "Create Password User" form (email, name, password, role)
- `frontend/src/components/UserTable.tsx` — add auth_method column, "Reset Password" action
- `frontend/src/pages/Config.tsx` — add Authentication config group (auth_google_enabled, auth_password_enabled toggles)
- `frontend/src/styles/components.css` — styles for auth-divider, totp-qr, recovery-codes, totp-input

## Wave 3: Testing (backend-qa + frontend-qa)

- Backend unit tests in password_auth.rs: password hash round-trip, TOTP validation, login flow, rate limiting, recovery codes, config validation
- E2E tests: password login + 2FA happy path, admin create user, forced password change, forced TOTP setup, config toggles

## Verification

```bash
cd backend && cargo clippy --all-targets && cargo test --lib && cd ../frontend && npm run build && npm run lint
```
