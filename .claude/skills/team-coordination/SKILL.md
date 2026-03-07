---
name: team-coordination
description: Reference guide for team composition patterns, file ownership rules, communication protocols, task coordination, and parallel development strategies.
---

# Team Coordination Reference

## 1. Team Sizing

| Complexity | Team Size | When |
|------------|-----------|------|
| Simple | 2-3 agents | Single-service change, one layer |
| Moderate | 3-4 agents | Cross-layer (backend + frontend) |
| Complex | 4-5 agents | Multi-service, infrastructure changes |

### Sizing Heuristics

- **1 agent per architectural layer** being modified (backend, frontend, infra)
- **+1 agent for QA** if the change affects streaming, auth, or converters
- **Never exceed 5 agents** — split into multiple tracks instead

---

## 2. File Ownership Rules

**One owner per file. No exceptions.**

### rkgw Ownership Examples

| File / Area | Owner | Rationale |
|-------------|-------|-----------|
| `backend/src/routes/mod.rs` | rust-backend-engineer | Central routing and AppState |
| `backend/src/converters/*` | rust-backend-engineer | Format translation |
| `backend/src/streaming/*` | rust-backend-engineer | Event Stream parsing |
| `backend/src/auth/*` | rust-backend-engineer | Auth logic |
| `backend/src/web_ui/*` | rust-backend-engineer | Web UI API handlers |
| `backend/src/guardrails/*` | rust-backend-engineer | Guardrails engine |
| `backend/src/mcp/*` | rust-backend-engineer | MCP Gateway |
| `frontend/src/pages/*` | react-frontend-engineer | UI pages |
| `frontend/src/components/*` | react-frontend-engineer | UI components |
| `frontend/src/lib/*` | react-frontend-engineer | Frontend utilities |
| `frontend/src/styles/*` | react-frontend-engineer | CSS styles |
| `docker-compose*.yml` | devops-engineer | Docker config |
| `**/Dockerfile` | devops-engineer | Container builds |

### Change Request Protocol

When Agent B needs a change in Agent A's file:
1. Agent B sends a DM describing the change needed
2. Agent A evaluates and applies the change
3. Agent A confirms completion
4. Agent B proceeds with dependent task

---

## 3. Communication Protocols

### Direct Message (DM) — Default
For routine: task assignments, status updates, change requests, dependency notifications, blockers.

### Broadcast — Rare
Only for: blocking issues affecting multiple agents, architecture changes, track-level decisions.

### Anti-Patterns
- Never send structured JSON as status messages
- Never broadcast routine updates
- Never reference agents by ID — use names
- Never send empty/acknowledgment-only messages

---

## 4. Task Coordination Strategies

### Dependency Chain (rkgw)

```
Backend Types/Models
       │
       ▼
Business Logic (converters, auth, streaming)
       │
       ▼
Route Handlers
       │
       ▼
Middleware Integration
       │
       ▼
Frontend API Client (apiFetch)
       │
       ▼
React Components/Pages
       │
       ▼
E2E Tests
```

### Parallel Work Opportunities

| Agent A | Agent B | Why It's Safe |
|---------|---------|---------------|
| Backend converter logic | Frontend UI mockup (agreed API shape) | Interface contract isolates work |
| Guardrails engine | MCP client manager | Independent modules, separate files |
| Backend auth changes | Frontend CSS/styling | No runtime dependency |
| Backend unit tests | Frontend E2E test setup | Independent test infrastructure |

### Interface Contracts

Before parallel implementation, agree on API shape:
```typescript
// Backend will return:
{ "metrics": { "total_requests": 100, "avg_latency_ms": 45 } }

// Frontend will consume:
interface Metrics { totalRequests: number; avgLatencyMs: number; }
```

---

## 5. rkgw Team Presets

| Preset | Agents | Use When |
|--------|--------|----------|
| `fullstack` | scrum-master + rust-backend + react-frontend + frontend-qa | Full-stack feature |
| `backend-feature` | scrum-master + rust-backend + backend-qa | Backend-only feature |
| `frontend-feature` | scrum-master + react-frontend + frontend-qa | Frontend-only feature |
| `review` | rust-backend + react-frontend + backend-qa | Code review |
| `debug` | rust-backend + react-frontend + devops | Debugging |
| `infra` | scrum-master + devops + rust-backend | Infrastructure changes |
| `docs` | scrum-master + document-writer | Documentation |

---

## 6. Integration Patterns

### Vertical Slices (Preferred for small features)
Each agent builds complete stack for their feature slice.

### Horizontal Layers (For large features)
Each agent owns one architectural layer across all features.

### Hybrid (Recommended for complex tracks)
Phase 1: Shared infrastructure (horizontal). Phase 2: Feature slices (vertical). Phase 3: Integration testing.

| Factor | Vertical | Horizontal | Hybrid |
|--------|----------|------------|--------|
| Team size | 1-2 | 3+ | 3-5 |
| Coordination | Low | High | Medium |
| Time to first deliverable | Fast | Slow | Medium |
| Typical rkgw use | Bug fix | Large refactor | New feature |

---

## References

- [Messaging Patterns](references/messaging-patterns.md) — 8 structured message templates for inter-agent communication (task assignment, blocker reports, review findings, etc.)
- [Dependency Graphs](references/dependency-graphs.md) — 5 task dependency patterns (independent, sequential, diamond, fork-join, pipeline) with rkgw-specific examples
- [Merge Strategies](references/merge-strategies.md) — 3 integration patterns (direct, sub-branch, trunk-based) with rkgw conflict prevention rules for backend/frontend parallel work
