# Plan: Create `/rename-plan` skill

## Files to create

- `.claude/skills/rename-plan/SKILL.md` — Skill definition with frontmatter + instructions

## Skill behavior

1. User invokes: `/rename-plan my-oauth-refactor`
2. Skill finds the most recent `.md` file in `.claude/plans/`
3. Renames it to `YYYY-MM-DD-<description>.md` (e.g., `2026-03-14-my-oauth-refactor.md`)
4. If no `$ARGUMENTS`, prompts user for a description via `AskUserQuestion`
5. Handles duplicate names by appending `-2`, `-3`, etc.

## Skill frontmatter

```yaml
name: rename-plan
description: Rename plan files to datetime-prefixed meaningful names
argument-hint: "<description>"
allowed-tools:
  - Bash
  - Glob
  - AskUserQuestion
```

## Key details

- Uses `Bash` with `date +%Y-%m-%d` for date, `mv` for rename
- Uses `Glob` to find `*.md` in `.claude/plans/`
- Sanitizes description: lowercase, spaces→hyphens, strip special chars
- `disable-model-invocation` NOT set — allow Claude to suggest it contextually
- Project-level skill (`.claude/skills/`) not global (`~/.claude/skills/`)

## Limitation

Plan file names are system-generated at plan mode entry — no hook exists to auto-rename.
The skill is manual, invoked after a plan is created or finalized.

## Verification

```bash
ls .claude/skills/rename-plan/SKILL.md  # file exists
```
