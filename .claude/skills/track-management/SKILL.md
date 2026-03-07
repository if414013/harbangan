---
name: track-management
description: Reference guide for creating, managing, and completing Conductor tracks — the logical work units for features, bugs, and refactors.
---

# Track Management Reference

## 1. Track Concept

A **track** is the atomic unit of planned work in Conductor. Each track represents a single deliverable — a feature, bug fix, chore, or refactor — with its own specification, implementation plan, and completion criteria.

Tracks live under `conductor/tracks/{track-id}/` and contain:

| File | Purpose |
|------|---------|
| `spec.md` | Requirements, acceptance criteria, scope boundaries |
| `plan.md` | Phased implementation plan with atomic tasks |
| `metadata.json` | Machine-readable track state |

---

## 2. Track Types

| Type | When to Use | rkgw Examples |
|------|-------------|---------------|
| `feature` | New capability or user-facing functionality | Add MCP tool caching, new guardrails rule type, Anthropic vision support |
| `bug` | Incorrect behavior that needs correction | Streaming parser truncation on large responses, token cache race condition |
| `chore` | Maintenance, dependency updates, config changes | Upgrade Axum 0.8, add new model alias, rotate API keys |
| `refactor` | Structural improvement without behavior change | Extract converter shared logic, refactor auth module, consolidate error types |

---

## 3. Track ID Format

Format: `{shortname}_{YYYYMMDD}` where `{shortname}` is a lowercase hyphenated slug derived from the track title (2-4 key words, under 30 characters) and `{YYYYMMDD}` is the creation date. Examples: `mcp-tool-caching_20260306`, `streaming-truncation_20260305`. IDs are never reused. On collision, append a numeric suffix (e.g., `-2`).

---

## 4. Track Lifecycle

```
draft → in_progress → completed / abandoned
```

---

## 5. Status Markers

| Marker | Name | Meaning | Example |
|--------|------|---------|---------|
| `[ ]` | Pending | Not started | `[ ] 1.1: Add streaming event type` |
| `[~]` | In Progress | Currently being worked | `[~] 1.2: Implement converter logic` |
| `[x]` | Complete | Finished — **must include commit SHA** | `[x] 1.3: Add unit tests \`a1b2c3d\`` |
| `[-]` | Skipped | Intentionally not done — **must include reason** | `[-] 1.4: Add Docker healthcheck (not needed)` |
| `[!]` | Blocked | Waiting on dependency — **must include blocker** | `[!] 2.1: Frontend integration (waiting on 1.3)` |

### Rules

- Only **one** task should be `[~]` per agent at a time.
- Never leave a task `[~]` across sessions.
- The commit SHA in `[x]` entries enables precise rollback.

---

## 6. Spec Quality Checklist

- [ ] **Requirements are testable**
- [ ] **Scope is clear** — explicit in/out of scope sections
- [ ] **Dependencies identified**
- [ ] **Risks addressed** — failure modes, rollback strategy
- [ ] **API contract defined** — request/response shapes for new endpoints
- [ ] **Migration strategy** — for existing data if schema changes

---

## 7. Plan Quality Checklist

- [ ] **Tasks are atomic** — each is a single committable unit
- [ ] **Phases are logical** — grouped by layer (backend → frontend → infra → QA)
- [ ] **Verification after each phase**
- [ ] **All spec requirements covered**
- [ ] **Dependencies ordered correctly**
- [ ] **No ambiguous tasks**

---

## 8. Track Sizing Guidelines

| Metric | Target Range |
|--------|-------------|
| Duration | 1-5 working days |
| Phases | 2-4 |
| Tasks per phase | 3-6 |
| Total tasks | 8-20 |

---

## 9. Common Track Patterns for rkgw

### Feature Track (Full-Stack)

```
Phase 1: Backend
  [ ] 1.1: Add request/response types
  [ ] 1.2: Implement business logic
  [ ] 1.3: Add route handler
  [ ] 1.4: Write unit tests

Phase 2: Frontend
  [ ] 2.1: Add API integration
  [ ] 2.2: Build UI component/page
  [ ] 2.3: Add styling

Phase 3: QA
  [ ] 3.1: Backend test coverage
  [ ] 3.2: E2E browser tests
```

### Bug Fix Track

```
Phase 1: Reproduction
  [ ] 1.1: Write failing test
  [ ] 1.2: Identify root cause

Phase 2: Fix & Verification
  [ ] 2.1: Implement fix
  [ ] 2.2: Verify test passes
  [ ] 2.3: Add regression guard
```

### Refactor Track

```
Phase 1: Characterization
  [ ] 1.1: Write characterization tests
  [ ] 1.2: Document current architecture

Phase 2: Incremental Refactor
  [ ] 2.1: Extract interface/abstraction
  [ ] 2.2: Migrate consumers
  [ ] 2.3: Verify tests pass

Phase 3: Cleanup
  [ ] 3.1: Remove old code
  [ ] 3.2: Update documentation
```

---

## 10. metadata.json Schema

```json
{
  "id": "mcp-tool-caching_20260306",
  "title": "Add MCP tool caching",
  "type": "feature",
  "status": "in_progress",
  "created_at": "2026-03-06T10:00:00Z",
  "updated_at": "2026-03-06T14:30:00Z",
  "completed_at": null,
  "priority": "high",
  "tags": ["mcp", "caching", "backend"],
  "phases": {
    "total": 3,
    "completed": 1,
    "current": 2
  },
  "tasks": {
    "total": 10,
    "completed": 4,
    "in_progress": 1,
    "pending": 5,
    "skipped": 0,
    "blocked": 0
  },
  "commits": ["a1b2c3d", "e4f5g6h"],
  "checkpoints": {},
  "dependencies": [],
  "blockers": []
}
```
