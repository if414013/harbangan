---
name: team-delegate
description: Assign tasks, send messages, and manage workload across team members. Use when user says 'assign task to agent', 'send message to team', 'rebalance workload', or 'broadcast to team'. Do NOT use for full feature orchestration (use team-feature).
argument-hint: "[team-name] [--assign agent 'task'] [--message agent 'content']"
allowed-tools:
  - Bash
  - Read
  - Write
  - SendMessage
  - AskUserQuestion
---

# Team Delegate

Assign tasks, send messages, and manage workload across rkgw Gateway team members.

## Critical Constraints

- **Team must exist** — verify `~/.claude/teams/{team-name}/config.json` exists before delegating; fail if team config is missing
- **Use SendMessage** — all inter-agent communication must go through `SendMessage`, not file-based coordination
- **Respect file ownership** — never assign files that are owned by another agent; check existing assignments before delegating

---

## Step 1: Load Team

Resolve from argument or list available teams:
```bash
ls -1 ~/.claude/teams/ 2>/dev/null
```

Load `~/.claude/teams/{team-name}/config.json`.

> **If the team config is not found at the expected path:** List all available teams from `~/.claude/teams/` and present them to the user. If no team directories exist at all, report "No active teams found. Use /team-spawn to create a team first." and stop.

## Step 2: Determine Mode

### Interactive (no flags)
```
Team: {team-name}
Members:
  1. scrum-master — Coordinator (idle/busy)
  2. rust-backend-engineer — Axum backend ({current-task})
  3. react-frontend-engineer — React UI ({current-task})
  ...

Actions:
  [a] Assign task to agent
  [m] Send message to agent
  [b] Broadcast to all agents
  [r] Rebalance workload
  [s] Show status
```

### Assign (`--assign agent 'task'`)
### Message (`--message agent 'content'`)

## Step 3: Execute Action

### Assign Task
Validate agent is one of: scrum-master, rust-backend-engineer, react-frontend-engineer, devops-engineer, backend-qa, frontend-qa, document-writer.

> **If the specified agent is not found in the team:** List the available team members from the loaded config (names and roles) and ask the user to pick a valid agent from the list.

Send via `SendMessage` with task description, priority, and context.

> **If SendMessage delivery fails:** Retry the message once. If it still fails, report the error to the user (including the target agent name and error details) and suggest checking whether the agent process is still running.

### Send Message
Direct message via `SendMessage`.

### Broadcast
Send to ALL agents with `type: "broadcast"`.

### Rebalance
Review assignments, identify idle/overloaded/blocked agents, suggest reassignments.

> **If no idle agents are available for rebalancing:** Report the current workload distribution for all agents (agent name, current task, how long they have been working) and suggest the user either wait for an agent to finish or manually reassign a lower-priority task.

## Step 4: Update Config

Update `~/.claude/teams/{team-name}/config.json` with assignments, statuses, timestamps.

## Step 5: Report

```
Action: {assign/message/broadcast/rebalance}
Target: {agent-name or "all"}
Status: Delivered

Team '{team-name}' updated.
  Active: {N} agents
  Working: {N} agents
  Idle: {N} agents
```
