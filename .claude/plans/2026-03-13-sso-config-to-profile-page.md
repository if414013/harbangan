# Move SSO Config to Per-User Kiro Settings on Profile Page

## Files to modify

- **`backend/src/web_ui/config_db.rs`**
  - Add `migrate_to_v14()`: `ALTER TABLE user_kiro_tokens ADD COLUMN IF NOT EXISTS oauth_start_url TEXT`, backfill from global `config` table
  - Add to `run_migrations()` after v13 block
  - Modify `upsert_user_oauth_client()` (line 1405): add `start_url: &str` param, include `oauth_start_url` in INSERT/ON CONFLICT
  - Add `get_user_sso_config(user_id) -> Result<(String, String)>` returning `(oauth_start_url, oauth_sso_region)` from `user_kiro_tokens`

- **`backend/src/web_ui/user_kiro.rs`**
  - Add `KiroSetupRequest { sso_start_url: String, sso_region: Option<String> }`
  - Modify `kiro_setup` (line 86): accept `Json<KiroSetupRequest>`, read SSO from body instead of global config, pass `start_url` to `upsert_user_oauth_client`
  - Extend `KiroStatusResponse` (line 19): add `sso_start_url: String, sso_region: String`
  - Modify `kiro_status` (line 52): fetch SSO config via `get_user_sso_config()`, return in response
  - Keep `spawn_token_refresh_task` global fallback (line 304) as safety net

- **`backend/src/web_ui/config_api.rs`**
  - Remove `"oauth_start_url" | "oauth_sso_region"` from `classify_config_change()` (line 36-37)
  - Remove from `validate_config_field()` match arm
  - Remove from `get_config_field_descriptions()` if present

- **`backend/src/web_ui/routes.rs`**
  - Remove `oauth_start_url`/`oauth_sso_region` DB reads and JSON fields from `get_config()` (lines 70-84, 111-112)

- **`frontend/src/lib/api.ts`**
  - Add `sso_start_url: string, sso_region: string` to `KiroStatus` interface (line 126)

- **`frontend/src/components/KiroSetup.tsx`**
  - Add `ssoStartUrl`/`ssoRegion` state, populate from status response
  - Add SSO text input fields (Start URL, Region) above action buttons
  - Modify `handleStart` to send `{ sso_start_url, sso_region }` body to `/kiro/setup`
  - Disable setup button when `ssoStartUrl` is empty

- **`frontend/src/pages/Config.tsx`**
  - Remove the `OAuth / SSO` group (lines 82-89)

## Verification
```bash
cd backend && cargo clippy --all-targets && cargo test --lib && cargo fmt --check && cd ../frontend && npm run build && npm run lint
```
