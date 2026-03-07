---
name: team-delegate
description: Assign tasks, send messages, and manage workload across team members.
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

---

## Step 1: Load Team

Resolve from argument or list available teams:
```bash
ls -1 ~/.claude/teams/ 2>/dev/null
```

Load `~/.claude/teams/{team-name}/config.json`.

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

Send via `SendMessage` with task description, priority, and context.

### Send Message
Direct message via `SendMessage`.

### Broadcast
Send to ALL agents with `type: "broadcast"`.

### Rebalance
Review assignments, identify idle/overloaded/blocked agents, suggest reassignments.

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
