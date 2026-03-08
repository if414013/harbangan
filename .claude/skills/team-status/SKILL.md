---
name: team-status
description: Monitor agent team members, their roles, and current task status. Use when user says 'how are agents doing', 'who is idle', 'team progress', 'check agent status', or 'show team members'. Do NOT use for project track progress (use conductor-status).
argument-hint: "[team-name] [--tasks] [--members] [--json]"
allowed-tools:
  - Bash
  - Read
---

# Team Status

Monitor agent team members, their roles, and current task status for rkgw Gateway teams.

## Critical Constraints

- **Read-only** — never modify team config, task state, or any project files; this skill is strictly observational
- **Graceful degradation** — if team config is missing or malformed, report the absence clearly instead of failing

---

## Step 1: Resolve Team

1. If `team-name` provided, use directly
2. Otherwise, list teams: `ls -1 ~/.claude/teams/ 2>/dev/null`
3. Single team: use automatically. Multiple: report all.

## Step 2: Load Team Config

```bash
cat ~/.claude/teams/{team-name}/config.json
```

> **If the config file is missing or cannot be read:** Check whether any team configs exist at all by listing `~/.claude/teams/`. If other teams are found, list them and ask the user to specify the correct team name. If no team directories exist, report "No active teams found. Use /team-spawn to create a team first." and stop.

Also check conductor tracks:
```bash
cat /Users/hikennoace/ai-gateway/rkgw/conductor/tracks.md 2>/dev/null
```

## Step 3: Check Agent Processes

```bash
ps aux | grep "claude.*--team-name {team-name}" | grep -v grep
```

> **If the `ps` command fails to find agent processes** (returns no matches or errors): Mark those agents as "status unknown" in the report rather than "stopped". An absent process entry may mean the agent exited, was never started, or the process name pattern does not match -- do not assume the agent has stopped.

## Step 4: Compile Task Status

Gather from TaskList and team config.

> **If TaskList returns empty or no tasks are found:** Report "No active tasks" in the Tasks section of the output rather than failing or omitting the section. This is a normal state for newly spawned teams or teams between assignments.

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
