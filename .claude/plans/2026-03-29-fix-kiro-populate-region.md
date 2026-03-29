# Plan: Fix Kiro model populate using API region instead of SSO region

## Context

The Kiro model populate fails with DNS error `q.ap-southeast-1.amazonaws.com: Name not known` because the populate fallback path incorrectly uses the user's **SSO region** (`oauth_sso_region` from `user_kiro_tokens`) as the **API region** for constructing the Amazon Q endpoint URL. Amazon Q (`q.{region}.amazonaws.com`) is only available in certain regions (e.g., `us-east-1`), but the user's SSO region can be any AWS region (e.g., `ap-southeast-1`).

The codebase already has two separate region concepts:
- `kiro_region` (Config) — API region, default `us-east-1`, used for `q.{region}.amazonaws.com`
- `oauth_sso_region` (per-user) — SSO region, used for `oidc.{region}.amazonaws.com`

The bug is at `model_registry.rs:307-310` where the fallback path uses `sso_region` for the API call.

## Fix

### File: `backend/src/web_ui/model_registry.rs`

**Change 1: Add `kiro_api_region` parameter to `populate_provider`** (line 282)

```rust
// Before:
pub async fn populate_provider(
    provider_id: &str,
    db: &Arc<ConfigDb>,
    http_client: &crate::http_client::KiroHttpClient,
    auth_manager: Option<&crate::auth::AuthManager>,
) -> Result<usize> {

// After:
pub async fn populate_provider(
    provider_id: &str,
    db: &Arc<ConfigDb>,
    http_client: &crate::http_client::KiroHttpClient,
    auth_manager: Option<&crate::auth::AuthManager>,
    kiro_api_region: &str,
) -> Result<usize> {
```

**Change 2: Use `kiro_api_region` in the Kiro fallback path** (line 307-310)

```rust
// Before:
Ok(Some((access_token, sso_region))) => {
    let region = sso_region.as_deref().unwrap_or("us-east-1");
    fetch_kiro_models_with_token(http_client, &access_token, region)

// After:
Ok(Some((access_token, _sso_region))) => {
    tracing::debug!(region = kiro_api_region, "kiro: using API region for model fetch");
    fetch_kiro_models_with_token(http_client, &access_token, kiro_api_region)
```

### File: `backend/src/web_ui/model_registry_handlers.rs`

**Change 3: Pass `kiro_region` from AppState config to `populate_provider`** (line 204)

```rust
// Read kiro_region from config
let kiro_api_region = {
    let config = state.config.read().await;
    config.kiro_region.clone()
};

let result = crate::web_ui::model_registry::populate_provider(
    provider_id,
    &db,
    &state.http_client,
    auth_manager_ref.as_deref(),
    &kiro_api_region,
)
```

### File: `backend/src/main.rs`

**Change 4: Update startup `populate_all_providers` call** to pass `kiro_region`

Find where `populate_provider` is called at startup and pass the config's `kiro_region`.

## File Manifest

| File | Action | Change |
|------|--------|--------|
| `backend/src/web_ui/model_registry.rs` | modify | Add `kiro_api_region` param, use it in Kiro fallback |
| `backend/src/web_ui/model_registry_handlers.rs` | modify | Pass `kiro_region` from config to `populate_provider` |
| `backend/src/main.rs` | modify | Pass `kiro_region` to startup populate call |

## Verification

1. `cd backend && cargo clippy --all-targets` — zero warnings
2. `cd backend && cargo test --lib model_registry::` — existing tests pass
3. `docker compose build && docker compose up -d` — rebuild and restart
4. Click "populate all" on providers page → check backend logs for `kiro: using API region for model fetch region=us-east-1`
5. Verify Kiro models appear (if AWS Q endpoint is reachable from Docker)

## Branch

`fix/kiro-populate-region`
