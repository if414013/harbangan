# .claude/ — Full Documentation

This directory is the AI workflow infrastructure for the rkgw Gateway. It replaces the external `agent-teams@claude-code-workflows` plugin with a fully self-contained system optimized for the rkgw Rust/React architecture.

## Directory Layout

```
.claude/
├── CLAUDE.md                    # Quick reference (structure + skill table)
├── README.md                    # This file (full documentation)
├── settings.local.json          # Claude Code configuration
├── agents/                      # 7 agent definitions
├── skills/                      # 16 invocable skills
├── agent-memory/                # Persistent per-agent memory
├── rules/                       # Coding standards
└── plans/                       # Implementation plans
```

---

## Agents (7 total)

Each agent is a `.md` file with YAML frontmatter defining its name, description, tools, model, and memory scope. The body contains domain-specific context.

### Implementation Agents (5)

| Agent | Service | Stack | Model |
|-------|---------|-------|-------|
| `rust-backend-engineer` | Backend (`backend/`) | Rust, Axum 0.7, Tokio, sqlx, PostgreSQL | inherit |
| `react-frontend-engineer` | Frontend (`frontend/`) | React 19, TypeScript 5.9, Vite 7 | inherit |
| `devops-engineer` | Infrastructure | Docker, nginx, Let's Encrypt | inherit |
| `document-writer` | Documentation | Notion API, Slack API, Markdown | inherit |

### Quality Agents (2)

| Agent | Scope | Focus |
|-------|-------|-------|
| `backend-qa` | `backend/src/` tests | cargo test, 395+ unit tests, tokio::test |
| `frontend-qa` | `frontend/` | Playwright E2E tests, browser testing |

### Orchestration Agent (1)

| Agent | Role | Model |
|-------|------|-------|
| `scrum-master` | Workflow coordinator — creates tracks, decomposes tasks, spawns teams, monitors progress | opus |

---

## Skills (16 total)

Skills are invocable via `/skill-name [arguments]`.

### Conductor Skills (6) — Project Management

| Skill | Purpose | Key Arguments |
|-------|---------|---------------|
| `/conductor-new-track` | Create track with spec, phased plan, and metadata | `"title" [--type feature\|bug\|refactor\|chore]` |
| `/conductor-implement` | Execute tasks from a track's plan | `TRACK-0001 [--phase N] [--task N.M] [--delegate]` |
| `/conductor-status` | Display project status | `[TRACK-0001] [--tracks] [--teams] [--full]` |
| `/conductor-manage` | Track lifecycle management | `TRACK-0001 [--action ...]` |
| `/conductor-revert` | Git-aware undo | `TRACK-0001 [--task N.M] [--phase N] [--preview]` |
| `/conductor-setup` | Initialize conductor artifacts | `[--refresh] [--add-service name] [--resume]` |

### Team Skills (7) — Multi-Agent Orchestration

| Skill | Purpose | Key Arguments |
|-------|---------|---------------|
| `/team-spawn` | Spawn team from presets | `[preset] [--delegate]` |
| `/team-feature` | Full feature orchestration | `"description" [--preset name] [--plan-first]` |
| `/team-delegate` | Task assignment dashboard | `team-name [--assign\|--message\|--broadcast]` |
| `/team-status` | Show team status | `[team-name] [--tasks] [--members] [--json]` |
| `/team-review` | Multi-dimensional code review | `[target] [--preset name] [--base branch]` |
| `/team-debug` | Hypothesis-driven debugging | `"error" [--scope path] [--hypotheses N]` |
| `/team-shutdown` | Graceful team termination | `team-name [--force] [--keep-config]` |

**Team presets:**

| Preset | Members | Use Case |
|--------|---------|----------|
| `fullstack` | scrum-master + rust-backend + react-frontend + frontend-qa | Full-stack feature |
| `backend-feature` | scrum-master + rust-backend + backend-qa | Backend-only feature |
| `frontend-feature` | scrum-master + react-frontend + frontend-qa | Frontend-only feature |
| `review` | rust-backend + react-frontend + backend-qa | Code review |
| `debug` | rust-backend + react-frontend + devops | Debugging |
| `infra` | scrum-master + devops + rust-backend | Infrastructure changes |
| `docs` | scrum-master + document-writer | Documentation |

### Reference Knowledge Skills (3)

| Skill | Purpose |
|-------|---------|
| `track-management` | Track lifecycle, status markers, sizing guidelines, metadata schema |
| `workflow-patterns` | TDD task lifecycle, rkgw TDD policy, phase checkpoints, git integration |
| `team-coordination` | File ownership rules, communication protocols, team sizing, integration patterns |

---

## How Conductor and Team Skills Connect

**Conductor skills own the plan, team skills own the people.**

### Conductor to Team (forward flow)

```
/conductor-new-track  →  suggests  →  /team-spawn {preset}
                      →  or        →  /team-feature {title}
/conductor-implement --delegate  →  SendMessage to assigned agent
/conductor-status --teams        →  reads ~/.claude/teams/
/conductor-manage --action complete  →  suggests  →  /team-shutdown
```

### Team to Conductor (reverse flow)

```
/team-feature    →  reads conductor/ artifacts for context
/team-spawn      →  suggests creating track via /conductor-new-track
/team-review     →  references conductor/code_styleguides/
/team-debug      →  references conductor/tech-stack.md, tracks.md
```

### Scrum Master Workflow (full loop)

```
1. Read conductor/product.md        — does this align with product goals?
2. Check conductor/tracks.md        — does a related track exist?
3. /conductor-new-track             — create spec + plan
4. Decompose into agent tasks       — using task breakdown patterns
5. /team-spawn {preset}             — spawn the right team
6. /team-delegate                   — assign tasks with dependencies
7. /team-status + /conductor-status — monitor progress
8. Verify against Definition of Done — from conductor/workflow.md
9. Update conductor/tracks.md       — mark track complete
```

---

## Conductor Artifacts

These live in `conductor/` at the repo root. Created via `/conductor-setup`.

| Artifact | Purpose |
|----------|---------|
| `product.md` | Product definition — vision, goals, target users |
| `tech-stack.md` | Services table, languages, frameworks, infrastructure |
| `workflow.md` | TDD policy, commit conventions, verification, Definition of Done |
| `tracks.md` | Track registry — all work items with status |
| `code_styleguides/rust.md` | Rust/Axum conventions, error handling, testing |
| `code_styleguides/typescript.md` | React 19, TypeScript, CSS custom properties |
| `setup_state.json` | Setup completion record |
| `index.md` | Navigation hub |

---

## Settings

`settings.local.json` configures:

- **Plugins**: playwright (browser automation), Notion (workspace), slack (messaging), commit-commands, rust-analyzer-lsp, context7, frontend-design, conductor, agent-teams
- **Environment**: `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` for multi-agent support
- **Teammate mode**: `iterm2` (agents spawn as iTerm2 tabs with distinct colors)
- **MCP servers**: deepwiki enabled

---

## Status Markers

| Marker | Name | Meaning |
|--------|------|---------|
| `[ ]` | Pending | Not started |
| `[~]` | In Progress | Currently being worked |
| `[x]` | Complete | Finished (include commit SHA) |
| `[-]` | Skipped | Intentionally not done (include reason) |
| `[!]` | Blocked | Waiting on dependency (include blocker) |
