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

## Step 3.5: Agent Activity Probe

For each agent that has a running process but no recent messages:

1. **Check git activity** — look for recent commits or file changes:
   ```bash
   cd {project-root} && git log --oneline --since="30 minutes ago" --all
   ```

2. **Check file modification times** — look for recently modified files in the agent's owned directories:
   ```bash
   find {project-root}/backend/src -name "*.rs" -mmin -10 2>/dev/null | head -5
   find {project-root}/frontend/src \( -name "*.ts" -o -name "*.tsx" \) -mmin -10 2>/dev/null | head -5
   ```

3. **Classify activity**:
   - **Active**: Recent commits or file modifications in the last 10 minutes
   - **Quiet**: Process running, no file changes in 10-30 minutes (may be reading/planning)
   - **Stale**: Process running, no file changes in 30+ minutes, no messages — likely stuck or context-exhausted

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
  Agent                        Role              Status    Activity
  rust-backend-engineer        Axum backend      working   active (edited 2 min ago)
  react-frontend-engineer      React UI          idle      stale (no activity 45 min)
  backend-qa                   Rust tests        waiting   quiet (no changes 15 min)

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

## Step 5.5: Context Exhaustion Heuristic

For each agent that has status "idle" but owns an in_progress task, check:

1. Count consecutive `idle_notification` messages from this agent (from conversation history)
2. Check if the agent produced any file edits, tool calls, or teammate messages between idle notifications

Classification:
- **Normal idle**: 0-1 idle notifications, or idle with no assigned tasks
- **Possibly stuck**: 2 consecutive idle notifications with an in_progress task
- **Likely context-exhausted**: 3+ consecutive idle notifications with an in_progress task and no tool calls between them

For "likely context-exhausted" agents, recommend:
```
Recommendation: {agent-name} appears context-exhausted. Run respawn protocol:
  1. Note completed work: check git log for recent commits by this agent
  2. Kill the agent process
  3. Use /team-spawn --respawn-for {agent-name}
```

## Step 6: Alerts

```
Alerts:
  [!] react-frontend-engineer process not found
  [!] rust-backend-engineer task running for >2 hours
  [!] rust-backend-engineer suspected context exhaustion — idle 3+ times with in_progress task
  [!] react-frontend-engineer stale — process running but no file activity for 30+ minutes
      Recommendation: Send a ping message. If no response after 2 minutes, likely context-exhausted.
```
