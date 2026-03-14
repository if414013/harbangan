# Profile SECURITY Section: Dynamic Auth-Aware Display

## Context

The Profile page's SECURITY section is currently gated behind `user.auth_method === 'password'`, so Google SSO users never see it. The section should be dynamic based on what auth methods the admin has enabled, and SSO users should be able to self-service set up a password when password auth is enabled.

Two problems to solve:
1. **Frontend**: SECURITY section should show for ALL users when password auth is enabled, not just password-auth users
2. **Backend**: SSO users can't set an initial password — `POST /auth/password/change` requires current password verification, which fails when `password_hash` is NULL

## Backend Change

### `backend/src/web_ui/password_auth.rs` — Modify `change_password` handler

Currently (line 562-566): always verifies current password against stored hash.

Change: If the user's `password_hash` is NULL (SSO user setting password for first time), skip current password verification. If `password_hash` exists, require current password as before.

```rust
// Pseudocode for the change:
if stored_hash is Some:
    verify current_password against stored_hash (existing behavior)
else:
    // SSO user setting initial password — skip verification
    // current_password field can be empty string
```

Also update the `ChangePasswordRequest` to make `current_password` optional or allow empty string for initial setup.

After setting the password, the user's `auth_method` in the session should reflect they now have password capability.

### `backend/src/web_ui/mod.rs` — No changes needed
The `/auth/password/change` endpoint is already session-authenticated (not admin-only).

## Frontend Changes

### `frontend/src/pages/Profile.tsx` — Make SECURITY section auth-aware

Currently: `{user.auth_method === 'password' && ( ... )}`

Change to: Fetch `/_ui/api/status` to check `auth_password_enabled`. Show SECURITY section when password auth is enabled system-wide, regardless of user's current auth method.

Logic:
```
if auth_password_enabled:
  show SECURITY section
  if user has password (auth_method === 'password' or has_password_hash):
    show "$ change password" + "$ reset 2fa" + 2FA status
  else (SSO user, no password yet):
    show "$ set password" button → navigates to /change-password
    (after setting password, they can then set up 2FA)
```

### `frontend/src/lib/api.ts` — Add status fetch function (if not already exported)

Check if `getStatus()` or similar already exists. The Login page already calls `/_ui/api/status` — reuse that function.

### `frontend/src/pages/PasswordChange.tsx` — Update copy for SSO users

Currently says "you must update your password to continue" (forced-change flow). When accessed voluntarily from Profile by an SSO user setting initial password, the copy should say "Set your password" instead.

## File Change Summary

| File | Action | Agent |
|------|--------|-------|
| `backend/src/web_ui/password_auth.rs` | MODIFY (allow NULL password_hash to skip verification) | rust-backend-engineer |
| `frontend/src/pages/Profile.tsx` | MODIFY (dynamic SECURITY section based on system auth config) | react-frontend-engineer |
| `frontend/src/pages/PasswordChange.tsx` | MODIFY (conditional copy for initial vs change) | react-frontend-engineer |

## Key Files

- `backend/src/web_ui/password_auth.rs` — `change_password` handler (line 531-584), `ChangePasswordRequest` struct
- `backend/src/web_ui/google_auth.rs` — `GET /_ui/api/status` handler (line 466-505), returns `auth_password_enabled`
- `frontend/src/pages/Profile.tsx` — SECURITY section (line 54-79)
- `frontend/src/pages/PasswordChange.tsx` — password change page
- `frontend/src/pages/Login.tsx` — reference for how status is fetched (line 148-149)
- `frontend/src/lib/api.ts` — check for existing `getStatus()` function

## Team Composition (`/team-spawn backend-feature`)

| Agent | Role | Scope |
|-------|------|-------|
| `scrum-master` | Create GitHub issue + task breakdown, coordinate agents | Issue management |
| `rust-backend-engineer` | Modify password change handler for NULL password_hash | `backend/src/web_ui/password_auth.rs` |
| `react-frontend-engineer` | Dynamic SECURITY section + PasswordChange copy | `frontend/src/pages/Profile.tsx`, `frontend/src/pages/PasswordChange.tsx`, `frontend/src/lib/api.ts` |
| `backend-qa` | Verify clippy, fmt, tests | `backend/` |
| `frontend-qa` | Verify build, lint | `frontend/` |

### Execution Order
- **Wave 1** (parallel): `rust-backend-engineer` + `react-frontend-engineer` work simultaneously
- **Wave 2** (parallel): `backend-qa` + `frontend-qa` verify their respective domains

## Verification

### Backend
```bash
cd backend && cargo clippy --all-targets   # zero warnings
cd backend && cargo fmt --check            # no diffs
cd backend && cargo test --lib             # zero failures
```

### Frontend
```bash
cd frontend && npm run build   # zero errors
cd frontend && npm run lint    # zero errors
```

### Manual (Playwright)
- Admin enables both Google SSO + password auth in Config
- SSO user (no password) sees SECURITY section with "$ set password" button
- SSO user clicks "$ set password", sets password successfully (no current password required)
- After setting password, user sees "$ change password" + "$ reset 2fa" + 2FA status
- Password-auth user sees full SECURITY section as before
- When admin disables password auth, SECURITY section disappears for all users
