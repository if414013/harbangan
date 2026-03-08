---
name: conductor-implement
description: Execute tasks from a track's plan with flexible TDD and optional team delegation. Use when user says 'start working on track', 'implement next task', 'begin phase', 'pick up where I left off', or 'delegate task'. Do NOT use for building features from scratch without a track (use team-feature).
argument-hint: "<track-id> [--phase N] [--task N.M] [--delegate]"
allowed-tools:
  - Bash
  - Read
  - Write
  - Edit
  - Glob
  - Grep
  - SendMessage
  - AskUserQuestion
---

# Conductor Implement

Execute implementation tasks from a track's phased plan. Supports direct execution (self) or delegation to specialized agents. Handles TDD policy, verification, commits, and status tracking.

## Critical Constraints

- **Never auto-commit** — always suggest a commit message and wait for explicit user approval before committing
- **Phase gates** — STOP and wait for explicit user approval between phases; this approval gate is mandatory
- **Branch required** — refuse to execute if on the `main` branch; create or switch to the track's feature branch first
- **One question per turn** — never batch multiple questions together
- **Mark tasks `[~]` when starting, `[x]` when done** — if implementation fails, revert the marker from `[~]` back to `[ ]`
- **Never force-push or reset** — only additive git operations; never use `git reset --hard` or `git push --force`

---

## Status Markers

| Marker | Meaning | When to use |
|--------|---------|-------------|
| `[ ]` | Pending | Not started yet |
| `[~]` | In progress | Currently being worked on |
| `[x]` | Complete | Finished (include commit SHA in task line) |
| `[-]` | Skipped | Intentionally not done (add reason) |
| `[!]` | Blocked | Waiting on a dependency (add reason) |

---

## Step 1 — Parse Input

Extract from the argument string:
- **track-id** (required): e.g., `mcp-tool-caching_20260306`
- **--phase N** (optional): Execute all tasks in phase N
- **--task N.M** (optional): Execute a specific task (e.g., 2.3)
- **--delegate** (optional): Delegate to the assigned agent instead of executing directly

If no `--phase` or `--task` is specified, auto-select the next pending task in the earliest incomplete phase.

---

## Step 2 — Load Track Context

Read these files:
1. `conductor/tracks/{track-id}/metadata.json` — track status, services, agents
2. `conductor/tracks/{track-id}/plan.md` — phased plan with task list
3. `conductor/tracks/{track-id}/spec.md` — specification and acceptance criteria
4. `conductor/workflow.md` — TDD policy, verification commands, commit format
5. `conductor/tech-stack.md` — service paths and agent mappings

If the track does not exist, report an error and stop.

---

## Step 3 — Determine Task Scope

### 3.1 — If `--task N.M` is specified
Select exactly that task from the plan.

### 3.2 — If `--phase N` is specified
Select all pending tasks in that phase, in order.

### 3.3 — If neither is specified (auto-select)
1. Find the earliest phase with `status: pending` or `status: in_progress`.
2. Within that phase, find the first actionable task (`- [ ]` or `- [!]` that is now unblocked).
3. Present the selected task to the user and ask to confirm.

---

## Step 4 — Pre-flight Checks

### 4.1 — Git Status
```bash
cd /Users/hikennoace/ai-gateway/rkgw && git status --short
```
- If there are uncommitted changes, warn the user.

### 4.2 — Branch Check
Read the expected branch from `metadata.json`.
```bash
cd /Users/hikennoace/ai-gateway/rkgw && git branch --show-current
```
- If the track branch does not exist, create it from `main`:
  ```bash
  cd /Users/hikennoace/ai-gateway/rkgw && git checkout -b {branch-name} main
  ```

### 4.3 — Directory Verification
Verify the service path exists.

---

## Step 5 — Execute Task

### 5A — Direct Execution (no `--delegate`)

#### 5A.0 — Mark Task In Progress
Change `- [ ]` to `- [~]` for the current task in `plan.md`.

#### 5A.1 — Understand the Task
Re-read the task description from `plan.md`. Cross-reference with `spec.md` acceptance criteria.

#### 5A.2 — TDD Check

**Required TDD** (write tests FIRST):
- Streaming parser logic
- Auth token refresh flow
- Converter bidirectional translation
- Middleware auth chain
- Guardrails engine evaluation

**Recommended TDD** (suggest but do not enforce):
- Route handlers
- HTTP client logic
- Model cache behavior
- Resolver alias mapping

**Skip TDD**:
- Docker config changes
- Static UI components
- CSS-only changes
- Environment variable additions
- Documentation

#### 5A.3 — Implement
Perform the implementation. All file operations happen within `/Users/hikennoace/ai-gateway/rkgw/`.

#### 5A.4 — Verify
Run the verification command for the affected service:

| Service | Command |
|---------|---------|
| backend | `cd /Users/hikennoace/ai-gateway/rkgw/backend && cargo clippy --all-targets && cargo test --lib` |
| frontend | `cd /Users/hikennoace/ai-gateway/rkgw/frontend && npm run build && npm run lint` |

#### 5A.5 — Mark Task Complete
Change `- [~]` to `- [x]` for the completed task.

#### 5A.6 — Phase Checkpoint (between phases)
After completing all tasks in a phase:

1. **Run phase-level verification** for every service touched.
2. **Present a Phase Completion Summary** to the user.
3. **WAIT for explicit user approval** before proceeding to the next phase. This approval gate is mandatory.

### 5B — Delegated Execution (`--delegate`)

Send the task to the assigned agent using SendMessage with track context, TDD requirement, verification command, and commit format.

---

## Step 6 — Commit Suggestion

Suggest a commit message following:
```
type(scope): description
```

Where:
- **type**: feat, fix, refactor, chore, test, docs, style, perf
- **scope**: proxy, streaming, auth, converter, model, middleware, guardrails, mcp, metrics, web-ui, config, docker

Do NOT commit automatically. Always ask the user first.

---

## Step 7 — Update Track Status

### 7.1 — Update `plan.md`
Mark task complete with commit SHA.

### 7.2 — Update `metadata.json`
- Increment `tasks_done` for the current phase.
- If all phases complete, set track `status: completed`.
- Update `updated_at` and append to `commits` array.

### 7.3 — Update `tracks.md`
Update status column if changed.

---

## Step 8 — Report and Next Steps

Print task completion summary with branch, commit, verification status, and next task suggestion.

---

## Error Handling

- If the track does not exist, list available tracks.
- If pre-flight checks fail and user aborts, exit cleanly.
- If implementation fails, revert task marker from `[~]` back to `[ ]`.
- Never force-push or reset. Only additive git operations.
