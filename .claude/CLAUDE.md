# .claude/ — AI Workflow Infrastructure

This directory contains the multi-agent workflow system for the rkgw Gateway. See [README.md](README.md) for full details.

## Structure

```
.claude/
├── settings.local.json          # Plugin toggles, MCP servers, env vars
├── agents/                      # 8 agent definitions (domain-specific AI roles)
│   ├── scrum-master.md          # Workflow coordinator (orchestrates all agents)
│   ├── rust-backend-engineer.md # Axum/Tokio backend (converters, auth, streaming)
│   ├── react-frontend-engineer.md # React 19 web UI (pages, SSE, CRT aesthetic)
│   ├── devops-engineer.md       # Docker, nginx, deployment, certs
│   ├── backend-qa.md            # Rust unit/integration tests
│   ├── frontend-qa.md           # Playwright E2E tests
│   ├── conductor-validator.md   # Conductor artifact auditor (read-only)
│   └── document-writer.md       # Notion, Slack, documentation
├── skills/                      # 16 invocable skills (/skill-name)
│   ├── conductor-*/             # 6 project management skills (tracks, plans, status)
│   ├── team-*/                  # 7 multi-agent orchestration skills
│   ├── track-management/        # Reference: track lifecycle, status markers
│   ├── workflow-patterns/       # Reference: TDD, phase checkpoints, git
│   └── team-coordination/       # Reference: file ownership, communication
├── agent-memory/                # Persistent agent-specific memory
├── rules/
│   └── web-ui.md                # Frontend coding standards
└── plans/
    └── google-sso-multi-user-rbac.md  # Auth migration plan
```

## Git Workflow

All agent work must follow the PR flow — never commit directly to `main`.

- Create a branch (`feat/`, `fix/`, `refactor/`, `chore/`) before making changes
- Open a PR via `gh pr create` when work is ready for review
- `main` requires 1 approving review, stale reviews are auto-dismissed
- Agents using `/team-feature` or `/conductor-implement` should work on feature branches and open PRs upon completion

## Two Skill Families

- **Conductor skills** (`/conductor-*`) — Manage WHAT to do: tracks, specs, plans, status
- **Team skills** (`/team-*`) — Manage WHO does it: spawn agents, delegate, review, debug

## Quick Reference

| Action | Skill |
|--------|-------|
| Create a feature track | `/conductor-new-track "title"` |
| Start implementing | `/conductor-implement TRACK-0001` |
| Check progress | `/conductor-status` |
| Spawn a team | `/team-spawn fullstack` |
| Full feature orchestration | `/team-feature "description"` |
| Code review | `/team-review --diff` |
| Debug an issue | `/team-debug "error message"` |
