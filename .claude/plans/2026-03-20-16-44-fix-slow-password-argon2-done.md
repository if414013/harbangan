# Fix: Slow Password Save/Create Freezing Frontend

## Context

Every password operation (change, create user, reset) takes several seconds and makes the frontend appear frozen. Root cause: `Argon2::default()` uses OWASP's highest-cost params (19MB memory, 2 iterations) which is CPU-expensive — especially in Docker on macOS. Password **change** is worst because it runs TWO sequential Argon2 operations (verify old + hash new).

The login rate limiter (5 attempts / 15-min lockout at `password_auth.rs:25-27`) already mitigates brute force, so we can safely reduce Argon2 cost without meaningful security regression.

## Changes

### 1. Tune Argon2 parameters — `backend/src/web_ui/password_auth.rs`

Replace `Argon2::default()` with a shared helper using reduced-but-secure params:

```rust
use argon2::{Argon2, Algorithm, Version, Params};

/// Build an Argon2id hasher with tuned parameters.
/// OWASP minimum: m=19456,t=2,p=1 (default). We use m=12288,t=3,p=1
/// (OWASP second recommendation) for faster hashing while maintaining
/// security — brute force is already rate-limited (5 attempts / 15min).
fn argon2_hasher() -> Argon2<'static> {
    let params = Params::new(12_288, 3, 1, None)
        .expect("valid Argon2 params");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}
```

**Call sites to update** (replace `Argon2::default()` with `argon2_hasher()`):
- `hash_password()` — line 34
- `verify_password()` — line 51

**Callers that benefit** (no changes needed in these, they call the functions above):
- `main.rs:111` — initial admin seed
- `password_auth.rs:267` — login verify
- `password_auth.rs:572` — change password verify
- `password_auth.rs:582` — change password hash
- `password_auth.rs:634` — admin create user
- `password_auth.rs:687` — admin reset password

**Backward compatibility**: Argon2 stores params in the hash string (`$argon2id$v=19$m=12288,t=3,p=1$...`). The `verify_password` function reads params from the stored hash, so existing passwords hashed with old defaults will still verify correctly. No migration needed.

### 2. Disable form inputs during submission — `frontend/src/pages/PasswordChange.tsx`

Add `disabled={submitting}` to all three `<input>` elements (lines ~83, ~133, ~182) so the form looks clearly inactive during the API wait. The submit button already has `disabled={submitting}`.

### 3. Disable form inputs in admin flows

- **`frontend/src/pages/Admin.tsx`** — add `disabled={creating}` to email, name, password, role inputs in the create user form
- **`frontend/src/components/UserTable.tsx`** — add `disabled={resetting}` to the reset password input

## Verification

1. **Backend tests**: `cd backend && cargo test --lib password_auth::` — existing tests (`test_hash_and_verify_password`, `test_hash_password_produces_different_hashes`, `test_verify_password_invalid_hash`) must pass with new params
2. **Clippy**: `cd backend && cargo clippy --all-targets` — zero warnings
3. **Frontend build**: `cd frontend && npm run build && npm run lint` — zero errors
4. **Manual test**: Open web UI, change password — should complete in <1s instead of 3-5s, inputs should be disabled during submission
5. **Cross-check old hashes**: If any existing users have passwords hashed with old defaults, verify they can still log in (Argon2 reads params from stored hash)
