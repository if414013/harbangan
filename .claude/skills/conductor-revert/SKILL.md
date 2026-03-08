---
name: conductor-revert
description: Git-aware undo for track changes. Reverts tasks, phases, or entire tracks safely. Use when user says 'undo task changes', 'revert phase', 'roll back track', or 'preview revert'. Do NOT use for pausing, archiving, or completing tracks (use conductor-manage).
argument-hint: "<track-id> [--task N.M] [--phase N] [--preview]"
allowed-tools:
  - Bash
  - Read
  - Write
  - Edit
  - Glob
  - AskUserQuestion
---

# Conductor Revert

Safely revert code changes made during track implementation. Uses `git revert` (never `git reset --hard`) to preserve history.

## Critical Constraints

- **Always use `git revert --no-edit`** — never use `git reset --hard`, `git push --force`, or any other destructive git operation
- **Preserve git history** — all reverts must create new commits that undo changes, never rewrite history
- **Preview mode (`--preview`) must not make any changes** — only display what would be reverted without executing
- **Ask for confirmation before executing reverts** — always confirm with the user before running any `git revert` commands
- **Never force-resolve conflicts** — if a revert causes a merge conflict, report it and ask the user to resolve, abort, or skip

---

## Step 1 — Parse Input

- **track-id** (required): e.g., `mcp-tool-caching_20260306`
- **--task N.M** (optional): Revert a specific task's commit(s)
- **--phase N** (optional): Revert all commits from a phase
- **--preview** (optional): Show what would be reverted without executing

---

## Step 2 — Load Track Context

Read `metadata.json` (commits array) and `plan.md` (task-to-commit mapping).

---

## Step 3 — Identify Commits to Revert

Filter commits by scope (task, phase, or entire track). Verify each commit exists in git history. Order: reverse chronological (newest first).

---

## Step 4 — Preview Mode (`--preview`)

Display commits, affected files, and tasks that will be marked incomplete. Do not execute.

---

## Step 5 — Execute Revert

1. Confirm with user
2. Pre-flight: check for uncommitted changes
3. Execute `git revert --no-edit {hash}` for each commit (newest first)
4. If conflict: report, ask user to resolve/abort/skip
5. Verify after revert:
```bash
cd /Users/hikennoace/ai-gateway/rkgw/backend && cargo clippy --all-targets && cargo test --lib
cd /Users/hikennoace/ai-gateway/rkgw/frontend && npm run build && npm run lint
```

---

## Step 6 — Update Track Status

- Update `plan.md`: change `[x]` back to `[ ]` for reverted tasks
- Update `metadata.json`: remove reverted commits, decrement counters, add `reverts` entry
- Update `tracks.md` if status changed

---

## Step 7 — Report

Show reverted commits, reset tasks, updated track/phase status, and verification result.

---

## Error Handling

- No recorded commits: report nothing to revert
- Missing commit hash: skip with warning
- Conflict: never force-resolve, always ask user
- Never use `git reset --hard`, `git push --force`, or destructive operations
