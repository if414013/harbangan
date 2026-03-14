# Plan: Provider Config DB Migration + Proxy-Only Removal

## Consultation Summary

- **Backend**: Proxy-only mode wired into `main.rs` (7 branch points), `config.rs` (6 fields + `is_proxy_only()`), `middleware/mod.rs` (auth bypass), `auth/manager.rs` (`new_from_env()` + `bootstrap_proxy_credentials()`). Provider OAuth client IDs read from env in 4 locations with hardcoded defaults.
- **Frontend**: `Config.tsx` already supports `password` field type. Adding a "Provider OAuth" group is straightforward via `CONFIG_GROUPS` array.
- **Infrastructure**: `docker-compose.gateway.yml` (56 lines) + `backend/entrypoint.sh` (197 lines) are proxy-only — clean deletion.
- **Database**: Config table already has `value_type` column. No DDL changes needed for encryption support — store `nonce+ciphertext+tag` as base64 in value column with `value_type='encrypted'`.

## Interface Contracts

New config keys (all `HotReload`, plain `string` type — these are public device-flow IDs, not secrets):

| Key | Default |
|-----|---------|
| `qwen_oauth_client_id` | `f0304373b74a44d2b584a3fb70ca9e56` |
| `anthropic_oauth_client_id` | `9d1c250a-e61b-44d9-88ed-5944d1962f5e` |
| `openai_oauth_client_id` | `app_EMoamEEZ73f0CkXaXp7hrann` |

New env var: `CONFIG_ENCRYPTION_KEY` (optional, base64 32-byte AES-256 key for future encrypted fields).

Removed env vars: `PROXY_API_KEY`, `KIRO_REFRESH_TOKEN`, `KIRO_CLIENT_ID`, `KIRO_CLIENT_SECRET`, `KIRO_SSO_URL`, `KIRO_SSO_REGION`.

## File Manifest

| File | Action | Owner | Wave |
|------|--------|-------|------|
| `backend/src/config.rs` | modify | rust-backend-engineer | 0+2 |
| `backend/src/main.rs` | modify | rust-backend-engineer | 0 |
| `backend/src/middleware/mod.rs` | modify | rust-backend-engineer | 0 |
| `backend/src/auth/manager.rs` | modify | rust-backend-engineer | 0 |
| `backend/src/providers/registry.rs` | modify | rust-backend-engineer | 0 |
| `backend/src/bin/probe_limits.rs` | modify | rust-backend-engineer | 0 |
| `backend/entrypoint.sh` | delete | devops-engineer | 0 |
| `docker-compose.gateway.yml` | delete | devops-engineer | 0 |
| `backend/Dockerfile` | modify | devops-engineer | 0 |
| `backend/src/web_ui/crypto.rs` | create | rust-backend-engineer | 1 |
| `backend/src/web_ui/mod.rs` | modify | rust-backend-engineer | 1 |
| `backend/Cargo.toml` | modify | rust-backend-engineer | 1 |
| `backend/src/web_ui/config_db.rs` | modify | database-engineer | 0+1+2 |
| `backend/src/web_ui/config_api.rs` | modify | rust-backend-engineer | 2 |
| `backend/src/web_ui/routes.rs` | modify | rust-backend-engineer | 2 |
| `backend/src/web_ui/provider_oauth.rs` | modify | rust-backend-engineer | 2 |
| `backend/src/web_ui/qwen_auth.rs` | modify | rust-backend-engineer | 2 |
| `backend/src/providers/qwen.rs` | modify | rust-backend-engineer | 2 |
| `frontend/src/pages/Config.tsx` | modify | react-frontend-engineer | 2 |
| `.env.example` | modify | devops-engineer | 3 |
| `docker-compose.yml` | modify | devops-engineer | 3 |

## Wave 0: Proxy-Only Removal
- [ ] Remove 6 proxy-only fields from `Config` struct, `is_proxy_only()`, env loading, `validate()` exception (rust-backend-engineer)
- [ ] Remove all `is_proxy_only` branching from `main.rs` — 7 conditional blocks (rust-backend-engineer)
- [ ] Remove proxy-only auth bypass in `middleware/mod.rs` lines 54-91 (rust-backend-engineer)
- [ ] Remove `new_from_env()` and `bootstrap_proxy_credentials()` from `auth/manager.rs` (rust-backend-engineer)
- [ ] Remove `proxy_api_key` handling in `config_db.rs`, clean dead code (database-engineer)
- [ ] Delete `backend/entrypoint.sh`, `docker-compose.gateway.yml`, update Dockerfile (devops-engineer)
- [ ] Update `bin/probe_limits.rs` to use generic `API_KEY` env var (rust-backend-engineer)

## Wave 1: Encryption Infrastructure
- [ ] Create `backend/src/web_ui/crypto.rs` — AES-256-GCM encrypt/decrypt, master key from `CONFIG_ENCRYPTION_KEY` env var (rust-backend-engineer)
- [ ] Add `aes-gcm = "0.10"` to `Cargo.toml` (rust-backend-engineer)
- [ ] Add `set_encrypted()` / `get_decrypted()` helpers to `config_db.rs`, update `load_into_config` to decrypt `value_type='encrypted'` (database-engineer)

## Wave 2: Provider Config to DB
- [ ] Add 3 provider OAuth fields to `Config` struct with defaults + env loading (rust-backend-engineer)
- [ ] Add 3 keys to `load_into_config()` in `config_db.rs` (database-engineer)
- [ ] Add validation, descriptions, classification for 3 keys in `config_api.rs` (rust-backend-engineer)
- [ ] Expose 3 fields in `get_config()` + `apply_config_field()` in `routes.rs` (rust-backend-engineer)
- [ ] Change `provider_oauth.rs` to read client IDs from Config instead of env (rust-backend-engineer)
- [ ] Change `qwen_auth.rs` to read client ID from Config instead of env (rust-backend-engineer)
- [ ] Change `QwenProvider::new()` to accept client_id parameter (rust-backend-engineer)
- [ ] Add "Provider OAuth" group to `CONFIG_GROUPS` in `Config.tsx` (react-frontend-engineer)

## Wave 3: Cleanup
- [ ] Update `.env.example` — remove proxy-only vars, add `CONFIG_ENCRYPTION_KEY`, annotate DB-configurable vars (devops-engineer)
- [ ] Update `docker-compose.yml` — keep `QWEN_OAUTH_CLIENT_ID` pass-through as bootstrap default (devops-engineer)

## Verification
```bash
cd backend && cargo clippy --all-targets && cargo fmt --check && cargo test --lib
cd frontend && npm run build && npm run lint
docker compose config --quiet
grep -r "proxy_api_key\|is_proxy_only\|PROXY_API_KEY" backend/src/ --include="*.rs"  # expect zero results
```

## Recommended Preset
`/team-implement --preset fullstack` — 4 agents: rust-backend-engineer, database-engineer, react-frontend-engineer, devops-engineer
