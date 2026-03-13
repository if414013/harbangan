---
name: team-shutdown
description: Gracefully terminate an agent team and clean up its configuration. Use when user says 'shut down team', 'stop all agents', 'clean up team', 'terminate agents', or 'kill the team'.
argument-hint: "[team-name] [--force] [--keep-config]"
allowed-tools:
  - Bash
  - Read
  - SendMessage
  - AskUserQuestion
---

# Team Shutdown

Gracefully terminate an agent team and clean up its configuration.

## Critical Constraints

- **Ordered shutdown** — terminate worker agents (engineers, QA, document-writer) first, scrum-master last
- **Confirm before proceeding** — show team status and ask for user confirmation unless `--force` is provided
- **Clean up team config** — remove `~/.claude/teams/{team-name}/` and `~/.claude/tasks/{team-name}/` after shutdown (unless `--keep-config`)

---

## Step 1: Resolve Team

1. If `team-name` provided, use directly
2. Otherwise, list: `ls -1 ~/.claude/teams/ 2>/dev/null`
3. Single team: confirm. Multiple: ask user to select.

Load `~/.claude/teams/{team-name}/config.json`.

## Step 2: Confirm Shutdown

Unless `--force`:
- Show members and their status
- Warn about in-progress tasks
- If team has a worktree (`worktree` field in config), show:
  - Uncommitted changes: `cd {worktree.path} && git status --short`
  - Unpushed commits: `cd {worktree.path} && git log --oneline origin/main..HEAD`
  - PR state: `gh pr list --head {worktree.branch} --json number,state,title`
- Ask for confirmation

## Step 3: Terminate Members

Send shutdown requests via `SendMessage`:
1. First: Worker agents (engineers, QA, document-writer)
2. Last: scrum-master

Handle non-responsive agents (retry, force, skip).

## Step 4: Cleanup Configuration

### Dead Member Cleanup (before full shutdown)

Before removing the team config directory, clean up ghost entries:

1. Read `~/.claude/teams/{team-name}/config.json`
2. For each agent in the `agents` array:
   - Check if the agent process is running: `ps aux | grep "claude.*{agent-name}.*{team-name}" | grep -v grep`
   - If not running and status is not `"replaced"`, set status to `"exited"`
3. This ensures the final config snapshot (if `--keep-config`) accurately reflects reality

### Persist to GitHub

Before cleaning up ephemeral state, sync incomplete work back to GitHub Issues:

1. For each in-progress or pending TaskList item that references a GitHub Issue `[#N]`:
   ```bash
   gh issue comment N --body "Session ended with task in-progress. Last known state: {status}. Remaining work: {description of what's left}"
   ```

2. Update project board status:
   - In-progress tasks → keep "In Progress" on the board
   - Pending tasks → move back to "To Do" on the board
   - Completed tasks → move to "Done" if not already

This ensures no work context is lost when the ephemeral TaskList is cleaned up.

### Step 4.5: Worktree Cleanup

If the team config has a `worktree` field (non-null):

1. **Check for uncommitted changes:**
   ```bash
   cd {project-root}/{worktree.path} && git status --short
   ```
   If dirty, offer to commit: `git add -A && git commit -m "chore: save uncommitted work from team {team-name}"`

2. **Check for unpushed commits:**
   ```bash
   cd {project-root}/{worktree.path} && git log --oneline origin/main..HEAD
   ```
   If unpushed commits exist, offer to push: `git push -u origin {worktree.branch}`

3. **Check PR status:**
   ```bash
   gh pr list --head {worktree.branch} --json number,state,mergedAt
   ```

4. **Remove the worktree:**
   ```bash
   git worktree remove .trees/{team-name} --force
   ```

5. **Branch cleanup:**
   - If PR is merged: delete local branch `git branch -d {worktree.branch}`
   - If PR is open or no PR exists: preserve the branch (work may still be needed)

6. **Prune stale worktree references:**
   ```bash
   git worktree prune
   ```

### Team Config
Unless `--keep-config`:
```bash
rm -rf ~/.claude/teams/{team-name}/
```

### Task List
```bash
rm -rf ~/.claude/tasks/{team-name}/
```

## Step 5: Report

```
Team '{team-name}' shut down successfully.

Terminated:
  scrum-master — acknowledged
  rust-backend-engineer — acknowledged
  react-frontend-engineer — acknowledged

Worktree:
  Path: {worktree.path} — {removed / n/a}
  Branch: {worktree.branch} — {deleted (PR merged) / preserved (PR open) / n/a}
  PR: #{number} ({state}) | none

Cleanup:
  Team config: {removed / kept}

Duration: {time from creation to shutdown}
```
