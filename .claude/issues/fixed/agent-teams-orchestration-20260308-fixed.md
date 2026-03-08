# Agent Teams & Multi-Agent Orchestration Issues

Filed: 2026-03-08
Context: Team `fullstack-s2qk` spawned for track `qwen-coder-provider_20260308`

---

## Issue 1: Agents go idle without picking up work

**Severity**: High
**Frequency**: Consistent (3 respawns needed for rust-backend-engineer)

After completing Phase 1, the rust-backend-engineer went idle and never picked up Phase 2 despite:
- Being told in the spawn prompt to continue to the next task
- Receiving 3+ direct messages with explicit instructions
- Having unblocked tasks visible in TaskList

The agent repeatedly emitted `idle_notification` messages but never started working. Required killing and respawning multiple times.

**Root cause**: Agent hits context window limit after completing a large task (Phase 1 = 7 subtasks across many files). Once the context is full, the agent can no longer process new instructions or tool calls, so it emits idle notifications indefinitely. The lead has no signal that the agent is context-exhausted vs simply idle.

**Impact**: Required 3 manual kills and respawns to get Phase 2 started. Each respawn wastes time and loses accumulated context.

---

## Issue 2: Respawned agents inherit stale iTerm2 session IDs

**Severity**: Medium
**Frequency**: Consistent when respawning after manual kill

When an agent is manually killed and respawned via the Agent tool with `team_name`, the system tries to reuse the dead iTerm2 pane session ID, causing:
```
Failed to create iTerm2 split pane: Error: Session 'XXXX' not found
```

**Workaround**: Spawn without `team_name` parameter (standalone agent) to bypass the stale session. This loses team messaging capabilities but at least the agent runs.

---

## Issue 3: Blocked agents consume resources while waiting

**Severity**: Low
**Frequency**: By design

Agents assigned to blocked tasks (react-frontend-engineer, backend-qa, frontend-qa) all spawned immediately, did their research, then sat idle for 15+ minutes waiting for dependencies. Each idle agent holds an iTerm2 pane and process.

**Suggestion**: Consider a "lazy spawn" pattern — only spawn agents when their tasks become unblocked, rather than spawning the full team upfront.

---

## Issue 4: TaskUpdate owner field doesn't propagate to respawned agents

**Severity**: Medium
**Frequency**: On respawn

Task #6 was owned by `rust-backend-engineer` (original). After killing and respawning as `rust-backend-engineer-2` and then `rust-backend-engineer-3`, the task ownership still showed the original agent name. Respawned agents don't automatically claim tasks from their predecessors.

---

## Issue 5: Team config accumulates dead members

**Severity**: Low
**Frequency**: On respawn

Each respawn adds a new member entry to `~/.claude/teams/{team}/config.json` without removing the dead predecessor. The config grows with ghost entries (rust-backend-engineer, rust-backend-engineer-2, rust-backend-engineer-3) that no longer correspond to running processes.

---

## Issue 6: No mechanism to verify agent is actually working

**Severity**: Medium
**Frequency**: Ongoing

Between spawn and the next message from an agent, there's no way to check if it's actively coding, stuck in a loop, or silently idle. The only signals are:
- `idle_notification` (means it stopped)
- A teammate message (means it finished something)

No heartbeat, progress indicator, or "currently editing file X" signal exists.

---

## Recommendations

1. Add context window usage signal — agents should report when they're approaching context limits so the lead can proactively respawn before they go silent
2. Auto-respawn on context exhaustion — detect when an agent stops responding due to context limits and automatically spawn a fresh agent with a summary of completed work
3. Implement automatic task reclaim on respawn — new agent with same role should inherit pending tasks
4. Clean up dead member entries from team config on spawn failure or manual kill
5. Support lazy agent spawning based on task dependency resolution
6. Handle iTerm2 session cleanup when agents are killed — don't cache stale pane IDs for respawns
7. Add agent heartbeat/progress signals so the lead can distinguish "working" from "stuck" from "context-exhausted"
8. Consider splitting large phases into smaller tasks so agents complete within context limits — e.g., Phase 1 (7 tasks across many files) exhausted the agent's context before it could move to Phase 2

---

## Resolution (2026-03-08)

All 6 issues addressed via prompt-level fixes across 6 skill files:

| Issue | Fix | Skill Files Modified |
|-------|-----|---------------------|
| 1. Context exhaustion | Detection heuristic (3+ idle + in_progress task), respawn protocol (`--respawn-for`), task sizing limit (max 4-5 per wave), health monitoring loop | team-status, team-spawn, team-feature, team-coordination |
| 2. Stale iTerm2 sessions | Retry + fallback spawn without `--team-name`, manual config registration | team-spawn |
| 3. Blocked agents waste resources | Lazy per-wave spawning (Wave 1 immediate, Wave 2+ deferred until dependencies resolve) | team-feature |
| 4. Task ownership lost on respawn | Reuse same agent name, automatic task reclaim via TaskUpdate, new "Reclaim Tasks" action | team-spawn, team-delegate |
| 5. Ghost members in config | Mark predecessors as `"replaced"` (not removed), dead member cleanup on shutdown | team-spawn, team-shutdown |
| 6. No working verification | Activity probe (git log + file mtime), stale detection (30+ min no activity), new Activity column in status output | team-status |

Key new concepts introduced:
- `--respawn-for {agent-name}` flag on `/team-spawn`
- `deferred_agents` array in team config for lazy spawning
- `replaced_by` / `replaced_at` fields on agent config entries
- Context exhaustion heuristic: 3+ consecutive idle_notifications + in_progress task + no file edits
- Agent activity classification: Active / Quiet / Stale
