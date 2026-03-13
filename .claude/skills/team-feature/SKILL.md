---
name: team-feature
description: Coordinated parallel feature development with automated team spawning, task decomposition, and integration verification. Dynamically adapts to any project stack via project context. Use when user says 'build this feature end to end', 'coordinate frontend and backend', or 'full feature development'.
argument-hint: "[feature-description] [--preset name] [--plan-first] [--worktree] [--no-worktree]"
allowed-tools:
  - Bash
  - Read
  - Write
  - Grep
  - Glob
  - SendMessage
  - AskUserQuestion
---

# Team Feature

Coordinated parallel feature development. All service detection, agent mapping, and verification commands are loaded dynamically from project configuration.

## Critical Constraints

- **Agent teams required** — `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` must be set
- **Dynamic service detection** — load service categories, agent mappings, and verification commands from project context (`CLAUDE.md` Service Map and `.claude/agents/*.md`); never hardcode service names or agent roles
- **One owner per file** — no file may be assigned to multiple agents
- **Cross-service contract verification** — verify that both sides of every interface contract are implemented before reporting success

---

## Step 1: Load Project Context

Read project configuration to build service detection and verification maps:

1. **Read the Service Map section from `CLAUDE.md`** to identify:
   - Service categories (e.g., Backend, Frontend, Infrastructure) and their technologies
   - Technology keywords per service (used for scope detection in Step 2)
   - Verification commands per service (used for verification in Step 7)

2. **Read `.claude/agents/*.md`** frontmatter to build agent registry:
   - Map each agent's description keywords to the service categories from the Service Map
   - Result: a `service-to-agent` map (e.g., Backend -> agent whose description matches backend technologies)

   > **If no matching agent is found for a detected service:** Warn the user (e.g., "No agent definition matches the '{service}' service. You can manually assign an agent or spawn a general-purpose agent for this service.") and suggest manual assignment. Continue building the map for the remaining services.

3. **Build keyword detection table** from the Service Map. For each service category, extract:
   - Technology names (e.g., "Axum", "React", "nginx")
   - Agent Role Keywords column entries
   - Common directory patterns (from the Path column)

4. **Build verification command map** from the Service Map. For each service, use:
   - The Verification column command
   - The project subdirectory (from the Path column)

## Step 2: Analyze Scope

Analyze the feature description against the keyword detection table built in Step 1.

For each service category from the Service Map:
- Check if feature description contains any of that service's keywords
- Scan for file paths mentioned in the description (match against project directory structure)
- Determine which services are affected

Map affected services to agents using the service-to-agent map from Step 1.

> **If no services are detected from the feature description:** Ask the user to specify which services are involved (e.g., "I couldn't detect which services this feature affects. Please specify: Backend, Frontend, Infrastructure, or a combination."). Do not proceed with team spawning until at least one service is confirmed.

Also detect if testing agents are needed:
- Look for test-related keywords in the feature description
- If the feature touches a service, include that service's QA agent if one exists

## Step 3: Select Preset

Based on detected scope, select a team preset:

| Scope Pattern | Recommended Preset |
|---------------|-------------------|
| Multiple service layers | fullstack |
| Single service only | {service}-feature (e.g., backend-feature) |
| Infrastructure only | infra |
| All services + comprehensive testing | fullstack |

If `--preset` is provided, use that directly. If `--plan-first` is set, present the analysis to the user for approval before proceeding.

## Step 4: Plan Decomposition

Break into parallel work streams, one per agent. Rules:

1. **One owner per file** — no file assigned to multiple agents
2. **Wave-based ordering** — organize work streams into waves based on dependency analysis:
   - Wave 1: Core/backend agents (foundations that other services depend on)
   - Wave 2: Consumer agents (frontend, integration layers that depend on Wave 1 APIs)
   - Wave 3: Verification agents (QA, testing — after feature code is substantially complete)
   - Wave 4: Documentation agents (after implementation is stable)
3. **Cross-service interface contracts** — for each boundary between services, define:
   - API endpoints / function signatures that both sides must agree on
   - Data types / schemas shared across the boundary
   - Event formats (if services communicate via events/streams)
4. **Task sizing for context limits** — no single agent should be assigned more than 4-5 subtasks in a single wave. If a wave has more subtasks for one agent, split into sub-waves (e.g., Wave 1a, Wave 1b) so the agent can be respawned between sub-waves if needed. Large phases (7+ subtasks across many files) are the primary cause of context exhaustion.

## Step 4.5: Create GitHub Issues

For each task from Step 4, create a GitHub Issue to establish the persistent tracking layer:

1. **Create issues** for each task:
   ```bash
   gh issue create --title "[service]: task description" \
     --label "service:backend,priority:p1-high,feature" \
     --body "## Requirements\n...\n\n## Acceptance Criteria\n...\n\n## Dependencies\nDepends on #N\nDepends on #M"
   ```
   For issues with open dependency issues, add the `status:blocked` label:
   ```bash
   gh issue create --title "[service]: task description" \
     --label "service:backend,priority:p1-high,feature,status:blocked" \
     --body "..."
   ```

2. **Add to project board** and set fields (Status, Priority, Service) using `gh project item-add` and `gh project item-edit`.

3. **Reference in TaskList** — include `[#N]` in each TaskList item description so agents can cross-reference:
   ```
   TaskList item: "[#42] [backend]: Add guardrails endpoint — Wave 1"
   ```

4. **Update board status** to "To Do" for Wave 1 tasks, "Backlog" for later waves.

## Step 5: Spawn Team (Lazy, Per-Wave)

Spawn agents incrementally by wave, not all at once:

### 5.1 — Spawn Wave 1 agents immediately
Use `/team-spawn` with only the Wave 1 agents (core/backend agents whose tasks have no dependencies). Pass `--feature-name` with a sanitized version of the feature description (lowercase, spaces → hyphens, max 50 chars) for worktree branch naming. Forward `--worktree` or `--no-worktree` if provided:
```
/team-spawn {wave1-agent1}, {wave1-agent2} --feature-name "{sanitized-feature-desc}" [--worktree]
```

### 5.2 — Defer Wave 2+ agents
Do NOT spawn Wave 2, 3, or 4 agents yet. Record their planned composition in the team config under a `"deferred_agents"` array:
```json
{
  "deferred_agents": [
    { "name": "{agent}", "wave": 2, "trigger": "Wave 1 APIs ready" },
    { "name": "{agent}", "wave": 3, "trigger": "Feature code complete" }
  ]
}
```

### 5.3 — Spawn deferred agents when unblocked
When a wave completes (all its tasks marked done), spawn the next wave's agents:
1. Read `deferred_agents` from team config
2. Filter for agents whose wave number matches the next wave
3. Spawn those agents via the same mechanism as Step 4 in team-spawn
4. Move them from `deferred_agents` to `agents` in the config
5. Send them their assignments immediately after spawn

This avoids 15+ minutes of idle resource consumption for blocked agents.

## Step 6: Assign Work Streams

Send each agent their assignment via `SendMessage`. Include in each assignment:

1. **Owned files** — explicit list, no overlaps
2. **Requirements** — specific deliverables for this work stream
3. **Interface contracts** — shared types/APIs this agent must implement or consume
4. **Wave number** — when this work stream should begin
5. **Dependencies** — which other work streams must complete first
6. **Acceptance criteria** — how to verify this work stream is complete
7. **Blocked status** — if the agent's tasks have `status:blocked` label (open dependencies), note that the agent should wait or be deferred until dependencies close

Wave execution:
- Wave 1: Already spawned and assigned in Step 5.1
- Wave 2: Spawn agents (from deferred list) when Wave 1 tasks are complete, then assign
- Wave 3: Spawn agents when feature code is substantially complete, then assign
- Wave 4: Spawn agents when implementation is stable, then assign
- Between waves: run `/team-status` to verify previous wave completion before spawning next

> **If an agent fails mid-task during work stream execution:** Report the failure to the user, including the agent name, error output, and which work stream was affected. Collect any partial results the agent produced (files created/modified, tests written). Then ask the user how to proceed: retry the failed agent, reassign the work stream to another agent, or continue with the remaining work streams and address the gap manually.

### Agent Health Monitoring Loop

After assigning Wave 1, enter a monitoring cycle:

1. Every time an agent sends an `idle_notification`, increment that agent's idle counter
2. If an agent sends a task completion message or a teammate DM, reset its idle counter to 0
3. If an agent's idle counter reaches 3 while it has an in_progress task:
   a. Log: `"{agent-name} appears context-exhausted after {N} tasks"`
   b. Check `git log` for the agent's recent commits to assess progress
   c. If the agent made meaningful progress, initiate the Respawn Protocol (`/team-spawn --respawn-for {agent-name}`)
   d. If no progress, send one final explicit message with the exact task description
   e. If still idle after that message, initiate Respawn Protocol
4. Continue monitoring until all waves complete

## Step 7: Integration Verification

Run verification commands dynamically based on the verification command map built in Step 1.

Determine `{working-dir}` by reading the team config's `worktree.path` field. If set, use `{project-root}/{worktree.path}` as the base directory; otherwise use `{project-root}`.

For each affected service, run its lint, build, and test commands:

```
For each service in affected_services:
  cd {working-dir}/{service-subdirectory} && {lint-command} && {test-command}
```

If no commands were found in the Service Map for a service, skip verification for that service and note it in the report.

> **If verification commands fail (non-zero exit from lint, build, or test):** Report which specific checks failed, include the command output (stderr/stdout), and ask the user whether to fix the issues before completing or proceed despite the failures. Do not mark the feature as COMPLETE if any verification check has failed — use status NEEDS_ATTENTION in the final report.

### Cross-Service Contract Validation

For each interface contract defined in Step 4, verify:
1. Both sides of the contract are implemented
2. Types/schemas match across the boundary
3. Integration points are wired correctly

Use Grep to verify contract compliance:
- Search for endpoint paths, function names, or type names from the contracts
- Confirm they exist on both sides of each service boundary

### Issue Closure

After verification passes, close all GitHub Issues resolved by this feature:

1. For each completed task with a GitHub Issue `[#N]`:
   ```bash
   gh issue close N --comment "Resolved in PR #M — all verification checks passed."
   ```

2. Update project board status to "Done" for closed issues.

3. If a PR was created, ensure the PR body includes `Closes #N` for each resolved issue so merging auto-closes them.

## Step 7.5: PR Creation (Worktree Teams)

If the team has an active worktree (check `worktree` field in team config):

1. **Push the worktree branch:**
   ```bash
   cd {working-dir} && git push -u origin {worktree.branch}
   ```

2. **Create a pull request from the worktree directory:**
   ```bash
   cd {working-dir} && gh pr create \
     --title "feat: {feature-description}" \
     --body "## Summary\n{feature summary}\n\n## Changes\n{list of changes}\n\nCloses #{issue-numbers}"
   ```

3. Include the PR URL in the final report.

Skip this step if no worktree is active — the team is working directly on a branch managed outside this workflow.

### Final Report

```
Feature: {description}
Team: {team-name}
Status: {COMPLETE / NEEDS_ATTENTION}

Work Streams:
  {agent-name}: {status} — {summary}

GitHub Issues:
  #{N}: {title} — {CLOSED / OPEN}
  ...

Verification:
  {service-name}: {PASS/FAIL} ({commands run})
  ...

Cross-Service Contracts:
  {contract-name}: {VERIFIED / MISMATCH}
  ...
```
