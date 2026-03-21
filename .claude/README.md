# .claude/ — Full Documentation

This directory is the AI workflow infrastructure for Harbangan. It provides a fully self-contained multi-agent system optimized for the Harbangan Rust/React architecture.

## Directory Layout

```
.claude/
├── CLAUDE.md                    # Quick reference (structure + skill table)
├── README.md                    # This file (full documentation)
├── settings.json                # Claude Code configuration
├── agents/                      # 7 agent definitions
├── skills/                      # 8 invocable skills
├── agent-memory/                # Persistent per-agent memory
├── rules/                       # Coding standards + plan mode rules
└── plans/                       # Implementation plans
```

---

## Agents (7 total)

Each agent is a `.md` file with YAML frontmatter defining its name, description, tools, model, memory scope, `permissionMode`, and `maxTurns`. The body contains domain-specific context. All agents run with `permissionMode: bypassPermissions` for autonomous execution.

### Implementation Agents (5)

| Agent | Service | Stack | maxTurns |
|-------|---------|-------|----------|
| `rust-backend-engineer` | Backend (`backend/`) | Rust, Axum 0.7, Tokio, sqlx, PostgreSQL | 100 |
| `react-frontend-engineer` | Frontend (`frontend/`) | React 19, TypeScript 5.9, Vite 7 | 100 |
| `database-engineer` | Database (`config_db.rs`) | PostgreSQL 16, sqlx 0.8, migrations | 80 |
| `devops-engineer` | Infrastructure | Docker, deployment | 80 |
| `document-writer` | Documentation | Notion API, Slack API, Markdown | 60 |

### Quality Agents (2)

| Agent | Scope | Focus | maxTurns |
|-------|-------|-------|----------|
| `backend-qa` | `backend/src/` tests | cargo test, 395+ unit tests, tokio::test | 80 |
| `frontend-qa` | `frontend/` | Playwright E2E tests, browser testing | 80 |

---

## Skills (8 total)

Skills are invocable via `/skill-name [arguments]`.

### Team Skills (4) — Multi-Agent Orchestration

| Skill | Purpose | Key Arguments |
|-------|---------|---------------|
| `/team-plan` | Analyze scope, explore codebase, produce plans | `"description" [--scope path]` |
| `/team-implement` | Full lifecycle: spawn → assign → verify → PR → shutdown | `"description" [--preset name] [--worktree]` |
| `/team-review` | Multi-dimensional code review | `[target] [--preset name] [--base branch]` |
| `/team-debug` | Hypothesis-driven debugging | `"error" [--scope path] [--hypotheses N]` |

**team-implement sub-commands:**

| Flag | Purpose |
|------|---------|
| `--status team-name` | Show team status (replaces /team-status) |
| `--delegate team-name` | Task assignment dashboard (replaces /team-delegate) |
| `--shutdown team-name` | Graceful team termination (replaces /team-shutdown) |

**Team presets:**

| Preset | Composition | Use When |
|--------|-------------|----------|
| `fullstack` | coordinator + all service agents + QA agents | Full-stack feature |
| `backend-feature` | coordinator + backend + database + backend-qa | Backend-only feature |
| `frontend-feature` | coordinator + frontend + frontend-qa | Frontend-only feature |
| `infra` | coordinator + infra + backend | Infrastructure changes |
| `docs` | coordinator + document-writer | Documentation |
| `research` | 3 general-purpose agents | Codebase exploration |
| `security` | 4 reviewer agents | Security audit |
| `migration` | coordinator + 2 service + 1 reviewer | Data/schema migration |
| `refactor` | coordinator + 2 service + 1 reviewer | Code refactoring |
| `hotfix` | 1 service + 1 QA agent | Urgent bug fix |

### Git Operations (1) — PR Lifecycle

| Skill | Purpose | Execution | Key Arguments |
|-------|---------|-----------|---------------|
| `/merge-pr` | Squash-merge PR, cleanup branches, return to main | Inline | `[pr-number]` |

`/merge-pr` has `disable-model-invocation: true` (destructive — user-only).

### Utility Skills (2)

| Skill | Purpose |
|-------|---------|
| `/humanizer` | Remove signs of AI-generated writing from text |
| `/rename-plan` | Rename plan files to datetime-prefixed descriptive names |

Note: Team coordination guidance (file ownership, communication protocols, team sizing) is now in `.claude/rules/team-coordination.md` and auto-loaded into all agent sessions.

---

## How Plan Mode and Team Skills Connect

**Plan mode owns the plan, team skills own the people.**

### Planning to Execution Flow

```
/team-plan (explore + design)   →  produce plan in .claude/plans/
/team-implement {plan}          →  spawn → assign → verify → PR → shutdown
TaskList (ephemeral)            →  /team-implement --delegate (assign to agents)
/team-implement --status        →  monitor progress (TaskList)
Quality Gates (from CLAUDE.md)  →  verify completion
/team-implement --shutdown      →  clean up ephemeral state
```

---

## Settings

`settings.json` configures:

- **Plugins**: playwright (browser automation), Notion (workspace), slack (messaging), commit-commands, rust-analyzer-lsp, context7, frontend-design, agent-teams
- **Environment**: `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` for multi-agent support
- **Teammate mode**: `in-process` (agents run within the main terminal, cycle with Shift+Down)
- **MCP servers**: deepwiki enabled
