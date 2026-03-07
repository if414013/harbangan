# Integration and Merge Strategies

Patterns for integrating parallel work streams in rkgw and preventing conflicts between backend and frontend agents.

## Integration Patterns

### Pattern 1: Direct Integration

All agents commit to the same branch. Integration happens naturally through strict file ownership.

```
feature/mcp-health
  <- rust-backend-engineer commits (backend/src/mcp/*)
  <- react-frontend-engineer commits (frontend/src/pages/*, frontend/src/components/*)
  <- devops-engineer commits (docker-compose*.yml)
```

**When to use**: Small teams (2-3 agents), strict file ownership, no overlapping file changes.

**rkgw suitability**: Good for most features. Backend and frontend have clear directory separation. Risk is low when file ownership rules from team-coordination are followed.

---

### Pattern 2: Sub-Branch Integration

Each agent works on a sub-branch. The scrum-master merges them sequentially following the dependency graph.

```
feature/guardrails-v2
  ├── feature/guardrails-v2-backend     <- rust-backend-engineer
  ├── feature/guardrails-v2-frontend    <- react-frontend-engineer
  ├── feature/guardrails-v2-tests       <- backend-qa
  └── feature/guardrails-v2-e2e         <- frontend-qa
```

Merge order: backend (foundation) -> frontend (consumes API) -> tests -> e2e.

**When to use**: Larger teams (4+), overlapping concerns, need for review gates between merges.

**rkgw suitability**: Best for complex features touching multiple backend modules (e.g., guardrails + MCP + auth changes). Allows review at each merge point.

---

### Pattern 3: Trunk-Based with Feature Flags

All agents commit to `main` behind a runtime feature flag. The flag is managed via the web UI config system.

```
main <- all agents commit
     <- feature gated by config flag in PostgreSQL
     <- web UI toggle at /_ui/ settings page
```

**When to use**: CI/CD environments, features that can be incrementally enabled, continuous deployment.

**rkgw suitability**: Natural fit. rkgw already uses runtime config flags (`mcp_enabled`, `guardrails_enabled`) stored in PostgreSQL and toggled via the web UI. New features can follow the same pattern:

```rust
// In route handler
if config.read().await.feature_x_enabled {
    // new behavior
} else {
    // existing behavior
}
```

---

## rkgw Conflict Prevention

### Backend / Frontend Parallel Work

The most common parallel pattern in rkgw is simultaneous backend and frontend development. These rules prevent conflicts:

| Rule | Rationale |
|------|-----------|
| rust-backend-engineer owns all `backend/src/**` files | Clear boundary |
| react-frontend-engineer owns all `frontend/src/**` files | Clear boundary |
| API contract agreed before implementation starts | Both sides code against the same shape |
| `frontend/nginx.conf` changes require devops-engineer | Shared infrastructure |
| `docker-compose*.yml` changes require devops-engineer | Shared infrastructure |

### Shared File Protocol

Some files are touched by multiple concerns. Handle them with single ownership + change requests:

| Shared File | Owner | Others Request Changes Via |
|-------------|-------|---------------------------|
| `backend/src/routes/mod.rs` | rust-backend-engineer | DM with route spec |
| `backend/Cargo.toml` | rust-backend-engineer | DM with dependency name + version |
| `frontend/package.json` | react-frontend-engineer | DM with package name + version |
| `docker-compose.yml` | devops-engineer | DM with service/port changes |
| `.env.example` | devops-engineer | DM with new variable name + description |

### Merge Conflict Hotspots in rkgw

These files are most likely to cause merge conflicts when multiple agents work in parallel:

1. **`backend/src/routes/mod.rs`** — AppState struct, route registration. Mitigate by batching route additions.
2. **`backend/Cargo.toml`** — Dependency additions. Mitigate by having rust-backend-engineer batch dependency changes.
3. **`frontend/src/App.tsx`** — Route definitions. Mitigate by having react-frontend-engineer add all routes in one commit.
4. **`frontend/src/lib/api.ts`** — API client functions. Mitigate by appending new functions (don't reorder existing ones).

---

## Integration Verification Checklist

After all agents complete their work, verify in this order:

1. **Build check**
   ```bash
   cd backend && cargo build
   cd frontend && npm run build
   ```

2. **Lint check**
   ```bash
   cd backend && cargo clippy
   cd backend && cargo fmt -- --check
   cd frontend && npm run lint
   ```

3. **Unit tests**
   ```bash
   cd backend && cargo test --lib
   ```

4. **Integration tests**
   ```bash
   cd backend && cargo test --features test-utils
   ```

5. **Interface verification**: Confirm API response shapes match what the frontend expects. Check `frontend/src/lib/api.ts` type definitions against actual backend handler return types.

6. **Docker build**: Full image build to catch missing dependencies.
   ```bash
   docker compose build
   ```

---

## Conflict Resolution Hierarchy

When conflicts arise, resolve in this order:

1. **Contract wins**: If code doesn't match the agreed API contract, the code is wrong. Fix the code.
2. **Tests decide**: The implementation that passes all existing tests is correct.
3. **Scrum-master arbitrates**: For ambiguous cases, the scrum-master decides which approach to keep.
4. **Manual merge**: For complex conflicts (rare with strict file ownership), the file owner merges by hand.
