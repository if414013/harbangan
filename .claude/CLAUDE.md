# .claude/ — AI Workflow Infrastructure

This directory contains the multi-agent workflow system for Harbangan. See [README.md](README.md) for full details.

## Structure

```
.claude/
├── settings.json                # Plugin toggles, MCP servers, env vars
├── agents/                      # 7 agent definitions (domain-specific AI roles)
│   ├── rust-backend-engineer.md # Axum/Tokio backend (converters, auth, streaming)
│   ├── react-frontend-engineer.md # React 19 web UI (pages, SSE, CRT aesthetic)
│   ├── database-engineer.md     # PostgreSQL schema, migrations, query optimization
│   ├── devops-engineer.md       # Docker, deployment, infrastructure
│   ├── backend-qa.md            # Rust unit/integration tests
│   ├── frontend-qa.md           # Playwright E2E tests
│   └── document-writer.md       # Notion, Slack, documentation
├── skills/                      # 8 invocable skills (/skill-name)
│   ├── team-plan/               # Scope analysis and implementation planning
│   ├── team-implement/          # Full lifecycle: spawn → assign → verify → PR → shutdown
│   ├── team-status/             # Show team health and agent activity
│   ├── team-review/             # Multi-dimensional code review
│   ├── team-debug/              # Hypothesis-driven debugging
│   ├── merge-pr/                # Squash-merge PR, cleanup branches, return to main
│   ├── rename-plan/             # Rename plan files to datetime-prefixed names
│   └── humanizer/               # AI writing cleanup
├── agent-memory/                # Persistent agent-specific memory
├── rules/
│   ├── backend.md               # Backend coding standards
│   ├── web-ui.md                # Frontend coding standards
│   ├── plan-mode.md             # Plan mode agent-awareness rules
│   └── team-coordination.md     # Team sizing, file ownership, communication protocols
└── plans/
    └── google-sso-multi-user-rbac.md  # Auth migration plan
```

## Git Workflow

All agent work must follow the PR flow — never commit directly to `main`.

- Create a branch (`feat/`, `fix/`, `refactor/`, `chore/`) before making changes
- Open a PR via `gh pr create` when work is ready for review
- `main` requires 1 approving review, stale reviews are auto-dismissed
- Agents using `/team-feature` should work on feature branches and open PRs upon completion

## Quick Reference

| Action | Skill |
|--------|-------|
| Plan a feature | `/team-plan "description"` |
| Implement a feature | `/team-implement "description"` |
| Check team health | `/team-status [team-name]` |
| Code review | `/team-review --diff` |
| Debug an issue | `/team-debug "error message"` |
| Merge & cleanup | `/merge-pr` |
| Rename plan file | `/rename-plan my-feature-name` |
