# Team Coordination Rules

Applies to all multi-agent team sessions.

## Team Sizing

| Complexity | Team Size | When |
|------------|-----------|------|
| Simple | 2-3 agents | Single-service change, one layer |
| Moderate | 3-4 agents | Cross-layer (backend + frontend) |
| Complex | 4-5 agents | Multi-service, infrastructure changes |

Heuristics: 1 agent per architectural layer + 1 for QA if touching streaming/auth/converters. Never exceed 5 agents — split into multiple tracks instead.

## File Ownership

One owner per file. No exceptions.

| File / Area | Owner |
|-------------|-------|
| `backend/src/**` | rust-backend-engineer |
| `backend/src/web_ui/config_db.rs` (DDL) | database-engineer |
| `frontend/src/**` | react-frontend-engineer |
| `docker-compose*.yml`, `**/Dockerfile` | devops-engineer |

When Agent B needs a change in Agent A's file: B sends a DM describing the change → A evaluates and applies → A confirms → B proceeds.

### Shared File Protocol

| Shared File | Owner | Others Request Via |
|-------------|-------|--------------------|
| `backend/src/routes/mod.rs` | rust-backend-engineer | DM with route spec |
| `backend/Cargo.toml` | rust-backend-engineer | DM with dependency + version |
| `frontend/package.json` | react-frontend-engineer | DM with package + version |
| `docker-compose.yml` | devops-engineer | DM with service/port changes |
| `.env.example` | devops-engineer | DM with new variable + description |

### Merge Conflict Hotspots

1. `backend/src/routes/mod.rs` — batch route additions
2. `backend/Cargo.toml` — batch dependency changes
3. `frontend/src/App.tsx` — add all routes in one commit
4. `frontend/src/lib/api.ts` — append new functions, don't reorder

## Communication Protocols

- DM (default): task assignments, status updates, change requests, blockers
- Broadcast (rare): blocking issues affecting multiple agents, architecture changes
- Anti-patterns: no JSON status messages, no broadcast for routine updates, use agent names not IDs, no empty acknowledgments

## Task Coordination

### Dependency Chain (Harbangan)

```
Backend Types → Business Logic → Route Handlers → Middleware → Frontend API Client → React Pages → E2E Tests
```

### Parallel Work Opportunities

| Agent A | Agent B | Safe Because |
|---------|---------|-------------|
| Backend converter logic | Frontend UI mockup | Interface contract isolates work |
| Guardrails engine | Config persistence | Independent modules |
| Backend auth changes | Frontend CSS/styling | No runtime dependency |
| Backend unit tests | Frontend E2E setup | Independent test infra |

Before parallel implementation, agree on API shape (interface contracts).

## Dependency Graph Patterns

| Pattern | Shape | Parallelism | Use When |
|---------|-------|-------------|----------|
| Independent | A,B,C → Join | Maximum | Separate modules, no overlap |
| Sequential | A → B → C → D | None | Each step needs previous output |
| Diamond | A → B,C → D | B,C parallel | Shared foundation (types/API contract) |
| Fork-Join | Phase 1 ∥ → Phase 2 ∥ → Phase 3 | Within phases | Natural phases (build, test, deploy) |
| Pipeline | A → B → C; A → D → E | Two chains | Independent feature branches from common base |

Anti-patterns: circular dependencies (deadlock), unnecessary sequencing, star bottleneck, over-sequencing backend→frontend (use diamond after types are defined).

## Integration Patterns

| Pattern | When | Harbangan Fit |
|---------|------|---------------|
| Direct (same branch) | 2-3 agents, strict ownership | Most features |
| Sub-branch | 4+ agents, overlapping concerns | Complex multi-module features |
| Trunk-based + flags | CI/CD, incremental rollout | Natural — uses existing config flags |
| Worktree isolation | Parallel teams, overlapping files | Multi-feature development |

Conflict resolution hierarchy: contract wins → tests decide → file owner merges manually.

## Agent Health & Respawn

Context exhaustion detection: 3+ consecutive idle notifications + in_progress task + no file edits = exhausted.

Respawn protocol: check git log → note remaining tasks → kill agent → respawn with same name → send handoff summary.

Prevention: max 4-5 subtasks per agent per wave, prefer many small tasks, proactive respawn after phase completion.

Lazy spawning: Wave 1 agents spawn immediately, Wave 2+ deferred until dependencies resolve.
