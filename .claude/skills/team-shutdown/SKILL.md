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
- Ask for confirmation

## Step 3: Terminate Members

Send shutdown requests via `SendMessage`:
1. First: Worker agents (engineers, QA, document-writer)
2. Last: scrum-master

Handle non-responsive agents (retry, force, skip).

## Step 4: Cleanup Configuration

### Team Config
Unless `--keep-config`:
```bash
rm -rf ~/.claude/teams/{team-name}/
```

### Conductor Tracks
Update `conductor/tracks.md` if applicable:
```bash
cat /Users/hikennoace/ai-gateway/rkgw/conductor/tracks.md 2>/dev/null
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

Cleanup:
  Team config: {removed / kept}
  Conductor track: {updated / not applicable}

Duration: {time from creation to shutdown}
```
