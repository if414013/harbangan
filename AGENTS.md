# Harbangan Codex and Qwen Code CLI Guide

This repository keeps Claude, Codex, and Qwen Code CLI workflow files side by side.

Codex and Qwen Code CLI support in this repo is intentionally narrow: the supported repo workflows are review and debugging only.

## Qwen Code CLI paths

- `.qwen/settings.json` is the project configuration for Qwen Code CLI (model providers, agents settings).
- `.qwen/agents/` contains project-scoped custom subagents (TOML format).
- `.qwen/skills/` contains project-scoped custom skills (SKILL.md format).

## Codex-specific paths

- `AGENTS.md` is the always-on instruction layer for Codex in this repository.
- `.codex/agents/` contains project-scoped custom subagents.
- `.codex/skills/` contains the actual project-scoped Codex skill files.
- `.agents/skills/` is a compatibility symlink so Codex still discovers the repo skills from its standard scan path.
- `.codex/plans/` stores plan files when you want Codex to review or keep them locally.

## Guardrails

- Do not modify `.claude/` unless the user explicitly asks. Treat it as the Claude-side source material and preserve any in-flight user changes there.
- Preserve unrelated local changes. Never revert work you did not make.
- Prefer `rg`/`rg --files` for search.
- Avoid destructive Git commands unless the user explicitly asks.
- Never write real credentials to the repository. Follow `.claude/rules/secrets.md`.

## Project workflow

- Use PR-style Git hygiene: make focused changes, verify them, and summarize risks.
- Follow the commit format in `.claude/rules/commit.md` when a commit message is needed.
- `team-review` supports explicit `--plan <path>` plan review and `--code [target]` code review.
- `team-debug` is the companion workflow for read-only root-cause investigation.

## Domain references

- For backend work, read `.claude/rules/backend.md` before editing `backend/`.
- For frontend work, read `.claude/rules/web-ui.md` before editing `frontend/`.
- For multi-agent coordination, ownership, or shared-file boundaries, read `.claude/rules/team-coordination.md`.
- For planning conventions, read `.claude/rules/plan-mode.md` if the user asks for a formal plan.

## Verification defaults

- Backend: `cd backend && cargo clippy --all-targets && cargo test --lib && cargo fmt --check`
- Frontend: `cd frontend && npm run build && npm run lint`
- Infrastructure: `docker compose config --quiet`

## Delegation defaults

- Prefer the custom agents in `.qwen/agents/` (Qwen Code CLI) or `.codex/agents/` (Codex) when you need domain-specific subagents.
- All repo custom agents are read-only and advisory-only by design. They inspect, cite evidence, and recommend next actions; they do not write or edit files.
- Keep one owner per file or change area during parallel work.
- Use read-only investigation/review passes before proposing risky cross-cutting changes.
