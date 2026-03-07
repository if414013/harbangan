---
name: conductor-setup
description: Initialize or update conductor project artifacts.
argument-hint: "[--refresh] [--add-service name] [--resume]"
allowed-tools:
  - Bash
  - Read
  - Write
  - Edit
  - Glob
  - Grep
  - AskUserQuestion
---

# Conductor Setup

Initialize or update the conductor orchestration layer for the rkgw Gateway project.

## Modes

1. **Full Init** (no flags) — First-time setup. Creates all conductor/ artifacts.
2. **Refresh** (`--refresh`) — Re-scan codebase for changes, update markdown files.
3. **Add Service** (`--add-service <name>`) — Register a new service in tech-stack.md.
4. **Resume** (`--resume`) — Continue an interrupted setup from where it left off.

---

## Step 1 — Detect Current State

Read `conductor/setup_state.json`. If complete, default to Refresh. If missing/incomplete, Full Init.

---

## Step 2 — Full Init Mode

### 2.1 — Gather Project Information

| Field | Auto-detect | Default |
|-------|-------------|---------|
| Project name | — | rkgw Gateway |
| Base branch | `git symbolic-ref refs/remotes/origin/HEAD` | main |
| Repository root | `git rev-parse --show-toplevel` | /Users/hikennoace/ai-gateway/rkgw |

### 2.2 — Auto-detect Services

| Indicator | Service | Path |
|-----------|---------|------|
| `backend/Cargo.toml` with `axum` | Backend | backend/ |
| `frontend/package.json` with `react` | Frontend | frontend/ |
| `docker-compose.yml` | Infrastructure | . |

### 2.3 — Create Directory Structure

```
conductor/
├── index.md
├── product.md
├── product-guidelines.md
├── tech-stack.md
├── workflow.md
├── tracks.md
├── setup_state.json
└── code_styleguides/
    ├── rust.md
    └── typescript.md
```

### 2.4-2.5 — Generate `product.md`
Platform purpose (AI API gateway), target users (developers, teams), capabilities (format conversion, streaming, guardrails, MCP).

### 2.6 — Generate `product-guidelines.md` (Interactive Q&A)

**CRITICAL: Ask ONE question per turn.** Use AskUserQuestion for each question, wait for the response, then proceed to the next. Maximum 4 questions.

Use the template at `.claude/skills/conductor-setup/references/product-guidelines-template.md` as the structural foundation. Ask:

1. **Voice & Tone**: "What is the product's voice? (e.g., technical and precise, friendly and casual, authoritative). How should tone shift for errors vs. success messages vs. onboarding?"
2. **Design Principles**: "Name 2-3 core design principles that guide product decisions. For each, what should the team always do vs. never do?"
3. **Accessibility Requirements**: "What accessibility standard are you targeting? (e.g., WCAG 2.1 AA). Any specific requirements like keyboard navigation, screen reader support, or color contrast?"
4. **Error Handling Patterns**: "How should errors be communicated to users? What structure should error messages follow? (e.g., what happened + why + how to fix)"

For brownfield projects, auto-detect existing patterns:
- Scan `frontend/src/styles/` for design tokens and aesthetic conventions
- Scan error handling in `backend/src/error.rs` and `backend/src/routes/` for existing patterns
- Present detected patterns and ask the user to confirm or adjust

Generate `conductor/product-guidelines.md` with the collected answers, following the template structure.

### 2.7 — Generate `tech-stack.md`

**Services table:**

| Service | Key | Path | Language | Framework | Agent | Verify Command |
|---------|-----|------|----------|-----------|-------|----------------|
| Backend | backend | backend/ | Rust | Axum 0.7 | rust-backend-engineer | `cd backend && cargo clippy --all-targets && cargo test --lib` |
| Frontend | frontend | frontend/ | TypeScript | React 19 | react-frontend-engineer | `cd frontend && npm run build && npm run lint` |
| Infrastructure | infra | . | Docker | docker-compose | devops-engineer | `docker compose build` |

**Infrastructure table:**

| Component | Technology | Purpose |
|-----------|------------|---------|
| Database | PostgreSQL 16 | Primary data store |
| Web Server | nginx | TLS termination, reverse proxy |
| Certificates | Let's Encrypt | Auto-renewal TLS certs |
| Runtime | Docker | Containerized deployment |

### 2.8 — Generate `workflow.md`

**Commit format:** `type(scope): description`
- Types: feat, fix, refactor, chore, test, docs, style, perf
- Scopes: proxy, streaming, auth, converter, model, middleware, guardrails, mcp, metrics, web-ui, config, docker

**TDD Policy:**
- Required: Streaming parser, auth token refresh, converter bidirectional, middleware auth chain, guardrails engine
- Recommended: Route handlers, HTTP client, model cache, resolver
- Skip: Docker config, static UI, CSS-only, env variable additions, documentation

**Verification commands:**
- Backend: `cd backend && cargo clippy --all-targets && cargo test --lib`
- Frontend: `cd frontend && npm run build && npm run lint`

### 2.9 — Generate `tracks.md`

```markdown
# rkgw Gateway — Track Index

| ID | Title | Type | Status | Services | Created |
|----|-------|------|--------|----------|---------|
```

### 2.10 — Generate Style Guides

#### `conductor/code_styleguides/rust.md`
Imports (std → external → crate::), error handling (thiserror/anyhow), logging (tracing macros), testing (#[cfg(test)], #[tokio::test]), async patterns (Arc/RwLock, DashMap).

#### `conductor/code_styleguides/typescript.md`
React 19 patterns, strict mode, CSS custom properties, apiFetch/useSSE, named exports, interface props.

### 2.11 — Write `setup_state.json`

```json
{
  "status": "complete",
  "initialized_at": "<ISO timestamp>",
  "last_refreshed_at": "<ISO timestamp>",
  "services_detected": ["backend", "frontend", "infra"],
  "styleguides_generated": ["rust", "typescript"],
  "version": "1.0.0"
}
```

### 2.12 — Report

```
Conductor initialized for rkgw Gateway
  Artifacts: index.md, product.md, product-guidelines.md, tech-stack.md, workflow.md, tracks.md
  Services detected: 2 local + 1 infrastructure
  Style guides: rust.md, typescript.md

  Next: Use conductor-new-track to create your first development track.
```

---

## Step 3 — Refresh Mode (`--refresh`)

Re-scan for services, compare against tech-stack.md, regenerate style guides if configs changed, update setup_state.json.

---

## Step 4 — Add Service Mode (`--add-service <name>`)

Ask for service details (key, name, path, language, framework, agent, verify command), add to tech-stack.md.

---

## Error Handling

- If `conductor/` exists during Full Init, ask to overwrite or switch to Refresh.
- Never delete existing track data during any mode.
