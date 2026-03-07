---
name: team-status
description: Monitor agent team members, their roles, and current task status.
argument-hint: "[team-name] [--tasks] [--members] [--json]"
allowed-tools:
  - Bash
  - Read
---

# Team Status

Monitor agent team members, their roles, and current task status for rkgw Gateway teams.

---

## Step 1: Resolve Team

1. If `team-name` provided, use directly
2. Otherwise, list teams: `ls -1 ~/.claude/teams/ 2>/dev/null`
3. Single team: use automatically. Multiple: report all.

## Step 2: Load Team Config

```bash
cat ~/.claude/teams/{team-name}/config.json
```

Also check conductor tracks:
```bash
cat /Users/hikennoace/ai-gateway/rkgw/conductor/tracks.md 2>/dev/null
```

## Step 3: Check Agent Processes

```bash
ps aux | grep "claude.*--team-name {team-name}" | grep -v grep
```

## Step 4: Compile Task Status

Gather from TaskList and team config.

## Step 5: Output Report

### Default (human-readable)
```
Team: {team-name}
Preset: {preset}
Created: {timestamp}

Members ({N} total):
  Agent                        Role                      Status
  rust-backend-engineer        Axum backend              working
  react-frontend-engineer      React UI                  idle
  backend-qa                   Rust tests                waiting

Tasks:
  Agent                        Task                      Status
  rust-backend-engineer        Add converter logic       in_progress
  react-frontend-engineer      Build config page         pending

Summary:
  Active: {N}  |  Idle: {N}  |  Exited: {N}
  Tasks — Completed: {N}  |  In Progress: {N}  |  Pending: {N}
```

### Members-only (`--members`)
### JSON (`--json`)

## Step 6: Alerts

```
Alerts:
  [!] react-frontend-engineer process not found
  [!] rust-backend-engineer task running for >2 hours
```
