---
name: workflow-patterns
description: Reference guide for implementing tasks according to Conductor's TDD workflow, handling phase checkpoints, managing git commits, and understanding the verification protocol.
---

# Workflow Patterns Reference

## 1. TDD Task Lifecycle

Every task follows this 11-step lifecycle. Steps marked with * are conditional on TDD policy.

1. **Select next pending task** from plan.md
2. **Mark task as `[~]`** in plan.md
3. **Write failing test (RED)*** — create test, run it, confirm failure
4. **Implement minimum code (GREEN)*** — make test pass
5. **Refactor for clarity (REFACTOR)*** — clean up, tests still pass
6. **Run verification command**
7. **Document any deviations**
8. **Commit implementation**
9. **Record commit SHA** in plan.md
10. **Update metadata.json**
11. **Check if phase is complete** — initiate Phase Checkpoint if so

---

## 2. rkgw TDD Policy

### Required TDD

Write the failing test BEFORE implementation for:

- **Streaming parser** — AWS Event Stream binary parsing, event variant handling. Parsing bugs silently corrupt responses.
- **Auth token refresh** — Kiro token lifecycle, cache TTL, refresh-before-expiry. Auth bugs cause cascading failures.
- **Converter bidirectional** — OpenAI↔Kiro and Anthropic↔Kiro translation. Format bugs affect all API consumers.
- **Middleware auth chain** — API key SHA-256 hash lookup, cache/DB fallback. Security-critical path.
- **Guardrails engine** — CEL rule evaluation, input/output validation. Safety-critical logic.

### Recommended TDD

Write tests alongside or immediately after:

- **Route handlers** — Request validation, response format
- **HTTP client** — Kiro API communication, error handling
- **Model cache** — TTL behavior, refresh logic
- **Resolver** — Model alias mapping, fallback behavior

### Skip TDD

Tests can be written after implementation:

- **Docker config** — docker-compose.yml, Dockerfile changes
- **Static UI components** — Pure presentational React components
- **CSS-only changes** — Styling updates via CSS custom properties
- **Environment variable additions** — .env.example updates
- **Documentation** — README, CLAUDE.md, comments

---

## 3. Phase Checkpoint Protocol

1. **Verify all tasks complete** — every task is `[x]` or `[-]`
2. **Run verification command**:

   | Service | Command |
   |---------|---------|
   | Backend | `cd /Users/hikennoace/ai-gateway/rkgw/backend && cargo clippy --all-targets && cargo test --lib` |
   | Frontend | `cd /Users/hikennoace/ai-gateway/rkgw/frontend && npm run build && npm run lint` |

3. **Generate phase summary**
4. **WAIT for explicit user approval.** Never auto-advance.
5. **If approved**: checkpoint commit, update metadata
6. **If rejected**: fix, re-verify, re-submit

---

## 4. Quality Gates

### Backend Quality Gates

| Gate | Command | Must Pass |
|------|---------|-----------|
| Lint | `cargo clippy --all-targets` | Zero warnings |
| Format | `cargo fmt --check` | No diffs |
| Unit tests | `cargo test --lib` | Zero failures |
| No regressions | Full suite | No previously-passing tests fail |

### Frontend Quality Gates

| Gate | Command | Must Pass |
|------|---------|-----------|
| Build | `npm run build` | Zero errors |
| Lint | `npm run lint` | Zero errors |
| No regressions | Full build | No new errors |

---

## 5. Git Integration

### Commit Message Format

```
type(scope): short description
```

### Types

| Type | When to Use |
|------|-------------|
| `feat` | New feature or capability |
| `fix` | Bug fix |
| `refactor` | Code restructuring without behavior change |
| `test` | Adding or updating tests only |
| `docs` | Documentation changes only |
| `chore` | Maintenance, config, dependency updates |
| `style` | Code formatting, whitespace, naming |
| `perf` | Performance improvement |

### Scopes (rkgw-Specific)

| Scope | Covers |
|-------|--------|
| `proxy` | Proxy endpoint handlers (/v1/*) |
| `streaming` | SSE streaming, Event Stream parsing |
| `auth` | Kiro auth, token refresh, API key auth |
| `converter` | Format converters (OpenAI/Anthropic ↔ Kiro) |
| `model` | Model types, resolver, cache |
| `middleware` | CORS, auth middleware, debug logging |
| `guardrails` | CEL rules, Bedrock API, content validation |
| `mcp` | MCP Gateway, tool servers, client manager |
| `metrics` | Request latency, token tracking |
| `web-ui` | Web UI API handlers, Google SSO, sessions |
| `config` | Configuration management, DB persistence |
| `docker` | Docker, nginx, deployment, certs |

### SHA Recording

Every completed task must include the commit SHA:
```markdown
[x] 2.3: Add streaming event parser `e4f5a6b`
```

---

## 6. Handling Deviations

Record in plan.md deviations section with type (Scope Addition/Reduction/Technical Change/Dependency Discovery) and rationale.

---

## 7. Error Recovery

### Failed Tests
1. Investigate — don't skip
2. Fix implementation, not test (unless test is wrong)
3. Re-run full suite before proceeding

### Checkpoint Rejection
1. Identify tasks needing rework
2. Create new commits (don't amend)
3. Re-verify and re-submit

### Dependency Blocker
Mark `[!]`, notify scrum-master, continue with unblocked tasks.

### Lint Failures
Fix all before committing. Pre-existing issues in touched files: fix in separate `style` commit.
