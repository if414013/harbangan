# Plan: Full Removal of MCP Registry/Gateway Feature

## Context

The MCP (Model Context Protocol) registry/gateway feature is being removed from Harbangan. This feature allowed the gateway to act as an MCP server and manage external MCP tool servers — it's no longer needed. The removal spans ~3,500 lines of backend Rust code, a full frontend page, nginx config, E2E tests, and documentation.

Another developer is working on a separate feature on `main`, so this work runs in an isolated git worktree on branch `refactor/remove-mcp-registry`.

**Important:** `.mcp.json` is Claude Code's own MCP server config (deepwiki, playwright, etc.) — DO NOT touch it.

---

## Step 0: Worktree Setup

Before spawning any agents, create the isolated worktree:

```bash
cd /Users/hikennoace/ai-gateway/harbangan
git worktree add ../harbangan-remove-mcp refactor/remove-mcp-registry
```

All agent work happens inside `/Users/hikennoace/ai-gateway/harbangan-remove-mcp/`. The main repo stays untouched for the other developer's feature work.

## Execution: `/team-spawn fullstack`

**Working directory:** `/Users/hikennoace/ai-gateway/harbangan-remove-mcp/`
**Branch:** `refactor/remove-mcp-registry`
**Isolation:** All agents spawn with `isolation: "worktree"` pointing at the worktree path above

When spawning agents via `Agent` tool, set the worktree path so they operate in the isolated copy. After all work is done and PR is merged, clean up:

```bash
cd /Users/hikennoace/ai-gateway/harbangan
git worktree remove ../harbangan-remove-mcp
```

### File Ownership Matrix

| Agent | Files Owned (exclusive) |
|-------|------------------------|
| `rust-backend-engineer` | All `backend/` files |
| `react-frontend-engineer` | All `frontend/src/` files |
| `devops-engineer` | `frontend/nginx.conf` |
| `frontend-qa` | `e2e-tests/` |
| `document-writer` | `CLAUDE.md`, `.claude/agents/`, `.claude/agent-memory/`, `.claude/rules/`, `.claude/skills/`, `gh-pages/` |

---

## Wave 1 — Backend + Frontend + Infra (parallel)

### 1A: Backend Core Removal (`rust-backend-engineer`)

**DELETE entire directory:** `backend/src/mcp/` (13 files, ~3,514 lines)

**Remove module declarations:**
- `backend/src/lib.rs:9` — delete `pub mod mcp;`
- `backend/src/main.rs:13` — delete `mod mcp;`

**Edit `backend/src/main.rs`:**
- Line 309: remove `mcp_manager: None,` from AppState construction
- Lines 345-359: delete MCP Gateway init block
- Lines 387-388: remove `let mcp_manager_ref = app_state.mcp_manager.clone();`
- Lines 417-420: remove MCP shutdown block
- Lines 536-566: remove MCP route registration block in `build_app()`

**Edit `backend/src/routes/state.rs:82-83`:** remove `mcp_manager` field

**Edit `backend/src/routes/pipeline.rs:259-276`:** delete `inject_mcp_tools` function

**Edit `backend/src/routes/openai.rs`:** remove MCP import (line 16) and tool injection block (lines 140-145)

**Edit `backend/src/routes/anthropic.rs`:** remove MCP import (line 16) and tool injection block (lines 121-126)

**Edit `backend/src/config.rs`:**
- Lines 46-51: remove 5 MCP config fields
- Lines 112-116: remove 5 default values

**Edit `backend/src/error.rs`:**
- Lines 82-105: remove 5 MCP error variants
- Lines 237-247: remove 5 HTTP response match arms

**Edit `backend/src/web_ui/mod.rs:115`:** remove `.nest("/admin/mcp", ...)`

**Edit `backend/src/web_ui/routes.rs`:**
- Lines 88-92: remove MCP fields from config JSON response
- Lines 197-231: remove `mcp_*` match arms from `apply_config_field()`
- Lines 285-288, 294: remove MCP from config schema

**Edit `backend/src/web_ui/config_api.rs`:**
- Lines 31-35: remove MCP keys from `classify_config_change()` HotReload list
- Line 102: remove `"mcp_enabled"` from boolean validation
- Lines 144-162: remove MCP timeout validation blocks
- Lines 278-303: remove MCP config schema entries

**Edit `backend/src/web_ui/config_db.rs`:**
- Lines 792-813: remove 5 `mcp_*` config overlay parsing arms
- Keep v5 migration as-is (already ran on deployed DBs)
- Add v16 migration: `DROP TABLE IF EXISTS mcp_clients` + delete MCP config keys from `config` table

**Edit `backend/Cargo.toml`:**
- Remove `tokio-util` dependency (only used by MCP)
- Remove `# MCP Gateway` comment, relabel `async-trait`/`async-stream` under `# Async utilities`

**Edit test AppState constructions** (remove `mcp_manager: None`):
- `backend/src/middleware/mod.rs:328`
- `backend/src/web_ui/google_auth.rs:686`
- `backend/tests/integration_test.rs:85-89` (MCP config fields), `:139` (mcp_manager)

**Verify:**
```bash
cd backend && cargo clippy --all-targets && cargo test --lib && cargo fmt --check
```

### 1B: Frontend Removal (`react-frontend-engineer`)

**DELETE:** `frontend/src/pages/McpClients.tsx` (454 lines)

**Edit `frontend/src/App.tsx`:** remove McpClients import (line 13) and route (line 30)

**Edit `frontend/src/components/Sidebar.tsx:46-48`:** remove MCP nav link

**Edit `frontend/src/lib/api.ts:360-412`:** remove all MCP types and API functions

**Edit `frontend/src/styles/components.css`:**
- Line ~1285: remove light theme MCP status override
- Lines 1389-1506: remove entire `/* ---- MCP Clients ---- */` section

**Verify:**
```bash
cd frontend && npm run build && npm run lint
```

### 1C: Infrastructure (`devops-engineer`)

**Edit `frontend/nginx.conf:61-74`:** remove `/mcp` location block

**Verify:**
```bash
docker compose config --quiet
```

---

## Wave 2 — Test Cleanup (after Wave 1)

### 2A: E2E Tests (`frontend-qa`)

**DELETE:** `e2e-tests/specs/ui/mcp.spec.ts`

**Edit `e2e-tests/specs/ui/navigation.spec.ts`:** remove MCP nav assertions (line 34, lines 54-55)

**DO NOT touch** `theme-toggle.spec.ts` — its `.playwright-mcp` reference is a screenshot directory, not the MCP feature.

### 2B: Integration Tests (handled by `rust-backend-engineer` in Wave 1)

Already covered in 1A — `backend/tests/integration_test.rs` edits.

---

## Wave 3 — Documentation (after Wave 1)

### 3A: Project Docs (`document-writer`)

**Edit `CLAUDE.md`:**
- Remove `mcp_enabled` from runtime config note (keep `guardrails_enabled`)
- Remove `mcp_manager` from AppState listing
- Remove `mcp/` from Key Modules section
- Remove MCP endpoints from API Endpoints (proxy + web UI sections)
- Remove MCP from request flow diagram
- Remove `mcp` scope from commit convention (or keep for git history — team's call)

**Edit `.claude/agents/rust-backend-engineer.md`:** remove MCP module references

**Edit `.claude/agents/document-writer.md`:** remove MCP references

**Edit `.claude/agent-memory/`:** remove MCP notes from `rust-backend-engineer/MEMORY.md` and `frontend-qa/MEMORY.md`

**Edit `gh-pages/` docs:** remove MCP references from `web-ui.md`, `troubleshooting.md`, `modules.md`, `client-setup.md`, `configuration.md`, `deployment.md`, `api-reference.md`, `architecture/request-flow.md`, `architecture/authentication.md`, `architecture/index.md`, `index.md`

---

## Wave 4 — Final Verification

```bash
# Backend quality gates
cd backend && cargo fmt --check
cd backend && cargo clippy --all-targets   # zero warnings
cd backend && cargo test --lib             # zero failures

# Frontend quality gates
cd frontend && npm run build               # zero errors
cd frontend && npm run lint                # zero errors

# Stale reference sweep (exclude .mcp.json and .playwright-mcp)
grep -r "mcp" backend/src/ --include="*.rs" | grep -v ".mcp.json"
grep -r "mcp" frontend/src/ --include="*.ts" --include="*.tsx" --include="*.css"
grep -r "mcp_enabled\|mcp_manager\|McpManager\|mcp_clients" backend/src/ frontend/src/
```

---

## Database Migration (v16)

Keep v5 migration intact (already ran on deployed DBs). Add v16:

```rust
async fn migrate_to_v16(&self) -> Result<()> {
    tracing::info!("Running database migration to version 16 (drop MCP clients)...");
    let mut tx = self.pool.begin().await
        .context("Failed to begin v16 migration transaction")?;

    sqlx::query("DROP TABLE IF EXISTS mcp_clients")
        .execute(&mut *tx).await
        .context("Failed to drop mcp_clients table")?;

    sqlx::query("DELETE FROM config WHERE key LIKE 'mcp_%'")
        .execute(&mut *tx).await
        .context("Failed to remove MCP config keys")?;

    sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
        .bind(16_i32).execute(&mut *tx).await
        .context("Failed to record schema version 16")?;

    tx.commit().await.context("Failed to commit v16 migration")?;
    tracing::info!("Database migration to version 16 complete");
    Ok(())
}
```

Wire it in `run_migrations()` after the v15 block.

---

## PR

**Branch:** `refactor/remove-mcp-registry`
**Title:** `refactor(backend): remove MCP registry/gateway feature`

**Body:**
Remove the MCP (Model Context Protocol) registry/gateway feature entirely.

- Delete `backend/src/mcp/` module (13 files, ~3,500 lines)
- Delete `frontend/src/pages/McpClients.tsx` and associated CSS/routes/API types
- Remove MCP config fields, error variants, route registration, and tool injection
- Add v16 database migration to DROP `mcp_clients` table and clean MCP config keys
- Remove `tokio-util` dependency (only used by MCP)
- Remove `/mcp` nginx location block
- Clean up E2E tests and documentation
