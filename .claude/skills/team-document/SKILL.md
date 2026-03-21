---
name: team-document
description: Update project documentation by consulting all domain agents for accuracy. document-writer leads, other agents provide technical review. Use when user says 'update docs', 'write documentation', 'refresh docs', 'document this', or 'sync documentation'.
argument-hint: "[scope-or-topic] [--target gh-pages|readme|claude|all]"
allowed-tools:
  - Bash
  - Read
  - Write
  - Grep
  - Glob
  - SendMessage
  - AskUserQuestion
  - TeamCreate
  - Agent
  - TaskCreate
  - TaskUpdate
  - TaskList
---

# Team Document

Update project documentation with technical accuracy verified by all domain agents. document-writer is the primary author; other agents serve as domain consultants.

## Critical Constraints

- **In-process teammate mode only** — all agents MUST run in-process (`teammateMode: "in-process"`). Never use tmux, iTerm split panes, or any other mode. Cycle between agents with Shift+Down.
- **Always spawn all 7 agents** — document-writer leads, others consult
- **Accuracy first** — document-writer must read actual source code, never guess
- **Domain agents are read-only consultants** — they review docs for accuracy but do not write documentation
- **Agents stay idle after completion** — use `/team-shutdown` to terminate

## Phase 1: Load Context

1. Read `CLAUDE.md` Service Map to identify all services and documentation references
2. Read `.claude/agents/*.md` to understand each agent's domain and ownership
3. Parse `$ARGUMENTS` for scope:
   - If topic provided (e.g., "auth", "streaming"): focus on that topic
   - If `--target` provided: focus on that target
   - Default: scan all documentation for drift

### Documentation Targets

| Target | Path | Content |
|--------|------|---------|
| `gh-pages` | `gh-pages/docs/**` | API reference, architecture, deployment, config, troubleshooting |
| `readme` | `README.md` | Project overview, quick start |
| `claude` | `CLAUDE.md`, `.claude/CLAUDE.md`, `.claude/README.md` | Project instructions, workflow docs |
| `all` | All of the above | Full documentation refresh |

## Phase 2: Spawn All Agents

Spawn all 7 domain agents via `TeamCreate` + `Agent`:

1. rust-backend-engineer
2. react-frontend-engineer
3. database-engineer
4. devops-engineer
5. backend-qa
6. frontend-qa
7. document-writer

## Phase 3: Identify Documentation Drift

document-writer scans existing documentation and compares against current source code:

1. **API endpoints** — compare `gh-pages/docs/api-reference.md` against `backend/src/routes/mod.rs`
2. **Architecture** — compare `gh-pages/docs/architecture/` against actual module structure in `backend/src/`
3. **Configuration** — compare `gh-pages/docs/configuration.md` against `.env.example` and `backend/src/config.rs`
4. **Deployment** — compare `gh-pages/docs/deployment.md` against `docker-compose*.yml`
5. **Web UI** — compare `gh-pages/docs/web-ui.md` against `frontend/src/pages/`
6. **Auth** — compare `gh-pages/docs/architecture/authentication.md` against `backend/src/auth/` and `backend/src/web_ui/`

Produce a drift report listing which docs are stale and what changed.

## Phase 4: Domain Consultation

Send each domain agent the drift report sections relevant to their area and ask them to verify:

| Agent | Reviews | Verifies |
|-------|---------|----------|
| rust-backend-engineer | API reference, architecture, auth, streaming, converters | Endpoint signatures, request/response shapes, error codes |
| react-frontend-engineer | Web UI docs, client setup | Component names, routes, API integration patterns |
| database-engineer | Schema docs, migration guides | Table structures, query patterns, migration steps |
| devops-engineer | Deployment docs, configuration | Docker setup, env vars, ports, health checks |
| backend-qa | Testing docs | Test commands, coverage areas, test patterns |
| frontend-qa | E2E testing docs | Test commands, test environment setup |

Each agent responds with:
- **Confirmed accurate** — docs match implementation
- **Needs update** — specific details that are wrong or missing (with file:line evidence)
- **No impact** — docs not relevant to their domain

## Phase 5: Write Documentation

document-writer updates documentation based on consultation findings:

1. Fix inaccuracies flagged by domain agents
2. Add missing documentation for new features/endpoints
3. Remove documentation for removed features
4. Update code examples to match current implementation
5. Refresh architecture diagrams (Mermaid) if structure changed

### Writing Standards

- Start with a clear **Overview** (1-2 paragraphs max)
- Use code blocks with language tags for all examples
- Include real examples from the codebase, not generic placeholders
- Use tables for structured data (endpoints, config options)
- Use **Mermaid** for diagrams
- For `gh-pages/docs/`: maintain Jekyll frontmatter format

## Phase 6: Cross-Check

After document-writer finishes updates, send the updated docs back to the relevant domain agents for final verification:

- Each agent confirms their domain's docs are now accurate
- If issues remain, document-writer adjusts and re-checks (max 1 round)
- Unresolved discrepancies are noted in the report

## Phase 7: Report

Output a summary of documentation changes:

```
Documentation Update Report
============================
Target: {gh-pages / readme / claude / all}
Scope: {topic or "full refresh"}

Updated:
- gh-pages/docs/api-reference.md — added 3 new endpoints, fixed auth header docs
- gh-pages/docs/architecture/streaming.md — updated event stream diagram
- CLAUDE.md — updated Service Map

Verified By:
- rust-backend-engineer: API reference, auth docs ✓
- react-frontend-engineer: Web UI docs ✓
- database-engineer: no impact
- devops-engineer: deployment docs ✓
- backend-qa: test docs ✓
- frontend-qa: no impact

No Changes Needed:
- gh-pages/docs/configuration.md — already accurate
- README.md — already accurate

Agents remain idle — use /team-shutdown when done.
```
