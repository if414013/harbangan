# .claude/ — Full Documentation

This directory is the AI workflow infrastructure for Harbangan. It provides a fully self-contained multi-agent system optimized for the Harbangan Rust/React architecture.

## Directory Layout

```
.claude/
├── CLAUDE.md                    # Quick reference (structure + skill table)
├── README.md                    # This file (full documentation)
├── settings.json                # Claude Code configuration
├── agents/                      # 7 agent definitions
├── skills/                      # 10 invocable skills
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

## Skills (10 total)

Skills are invocable via `/skill-name [arguments]`.

### Team Skills (6) — Multi-Agent Orchestration

| Skill | Purpose | Key Arguments |
|-------|---------|---------------|
| `/team-plan` | Analyze scope, explore codebase, produce plans | `"description" [--scope path]` |
| `/team-implement` | Full lifecycle: spawn → assign → verify → PR | `"description"` |
| `/team-document` | Update docs with domain agent consultation | `[scope] [--target gh-pages\|readme\|claude\|all]` |
| `/team-review` | Multi-dimensional code review | `[target] [--base branch]` |
| `/team-debug` | Hypothesis-driven debugging | `"error" [--scope path] [--hypotheses N]` |
| `/team-shutdown` | Gracefully terminate a running team | `[team-name]` |

All team skills spawn all 7 domain agents by default. Agents without tasks remain idle and available for ad-hoc work. Use `/team-shutdown` to terminate.

**team-implement sub-commands:**

| Flag | Purpose |
|------|---------|
| `--status team-name` | Show team status (replaces /team-status) |
| `--delegate team-name` | Task assignment dashboard |

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
/team-implement {plan}          →  spawn → assign → verify → PR
TaskList (ephemeral)            →  /team-implement --delegate (assign to agents)
/team-status                    →  monitor progress (TaskList)
Quality Gates (from CLAUDE.md)  →  verify completion
/team-shutdown                  →  terminate agents, clean up resources
```

---

## Settings

`settings.json` configures:

- **Plugins**: playwright (browser automation), Notion (workspace), slack (messaging), commit-commands, rust-analyzer-lsp, context7, frontend-design, agent-teams
- **Environment**: `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` for multi-agent support
- **Teammate mode**: `in-process` (agents run within the main terminal, cycle with Shift+Down)
- **MCP servers**: deepwiki enabled
