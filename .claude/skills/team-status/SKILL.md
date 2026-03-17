---
name: team-status
description: Show team health and agent activity for a running team session. Checks agent commits, file changes, task progress, and context exhaustion. Use when user says 'team status', 'how is the team doing', 'check agents', or 'who is stuck'.
argument-hint: "[team-name]"
allowed-tools:
  - Bash
  - Read
  - Grep
  - Glob
  - TaskList
  - TaskGet
---

# Team Status

Show health and activity for a running agent team. Auto-detects the team if only one is active.

## Steps

1. **Resolve team**: If `team-name` provided, use it. Otherwise, check `~/.claude/teams/` for active teams — if exactly one, use it; if multiple, ask via output which to inspect.

2. **Load team config**: Read `~/.claude/teams/{team-name}/config.json` to get member list (name, agentType, agentId).

3. **Check agent activity**:
   - `git log --author={agent} --since="30 minutes ago" --oneline` for recent commits
   - File modification times in owned directories
   - TaskList status for assigned tasks

4. **Classify each agent**:
   - **Active**: commits or file changes in last 5 minutes
   - **Quiet**: no activity for 5-15 minutes
   - **Stale**: no activity for 15+ minutes

5. **Context exhaustion detection**: 3+ consecutive idle notifications with an in_progress task and no file edits = likely exhausted.

6. **Cross-reference TaskList vs GitHub Issues** for drift (tasks completed locally but not updated on board, or vice versa).

7. **Output summary**:
   ```
   Team: {team-name}
   Members: {count} agents

   Agent          | Status  | Current Task        | Last Activity
   ---------------|---------|---------------------|---------------
   backend-eng    | Active  | Implement converter | 2m ago (commit)
   frontend-eng   | Quiet   | Build settings page | 8m ago (file edit)
   backend-qa     | Stale   | Write auth tests    | 22m ago

   Alerts:
   - backend-qa may be context-exhausted (3 idle cycles, no edits)
   ```

8. **Actionable suggestions**: For stale/exhausted agents, suggest respawn or task reassignment.
