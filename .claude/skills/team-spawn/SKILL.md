---
name: team-spawn
description: Initialize agent teams from presets or custom composition. Dynamically loads agent definitions and service mappings from project configuration. Use when user says 'spin up a team', 'create agents', 'need a fullstack team', 'start backend team', or 'spawn agents'.
argument-hint: "[preset] [--delegate]"
allowed-tools:
  - Bash
  - Read
  - Write
  - Glob
  - AskUserQuestion
---

# Team Spawn

Initialize agent teams from presets or custom composition. Agent definitions and service mappings are loaded dynamically from project configuration — no hardcoded agent names or colors.

## Critical Constraints

- **Agent teams required** — `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` must be set
- **Dynamic agent loading** — load agent definitions from `.claude/agents/*.md` at runtime; never hardcode agent names, roles, or colors
- **Background spawning** — spawn all agents with `run_in_background: true`
- **Persist team config** — save team config to `~/.claude/teams/{team-name}/config.json` after spawning

---

## Step 1: Load Project Context

Read the following files to build the agent registry and service map:

1. **Agent definitions** — Glob `.claude/agents/*.md` and parse each file's YAML frontmatter to extract:
   - `name` — agent identifier
   - `description` — role summary (first sentence is the short role)
   - `model` — model override (if any)

   > **If an agent .md file is not found or its YAML frontmatter cannot be parsed:** Skip that agent, warn the user (e.g., "Skipping agent '{filename}': unable to parse definition"), and continue loading the remaining agent files. The team can still be spawned with the successfully loaded agents.

2. **Tech stack** — Read `conductor/tech-stack.md` to identify:
   - Service categories (e.g., Backend, Frontend, Infrastructure)
   - Technologies and keywords associated with each service
   - Build/test commands per service

3. **Agent colors** — If `.claude/agent-colors.json` exists, read it. Otherwise, auto-assign colors from this default palette based on agent load order:
   ```
   #FF6B6B, #4ECDC4, #45B7D1, #FFA07A, #DDA0DD, #F0E68C, #98D8C8, #B8A9C9, #F4A460, #87CEEB
   ```

Build an in-memory registry:
```
agents = [{ name, description, role (first sentence of description), color, model }]
```

## Step 2: Resolve Team Composition

### Presets

Presets reference agents by **role keywords**, not hardcoded names. Match each role keyword against agent `description` fields to find the best agent.

| Preset | Role Keywords | Composition |
|--------|--------------|-------------|
| fullstack | coordinator + all service-layer agents + test agents | 1 coordinator + 1 agent per service + QA agents |
| backend-feature | coordinator + backend agent + backend test agent | Agents whose descriptions match backend technologies from tech-stack.md |
| frontend-feature | coordinator + frontend agent + frontend test agent | Agents whose descriptions match frontend technologies from tech-stack.md |
| review | all service-layer agents + test agents | Multi-dimensional code review |
| debug | all service-layer agents + infrastructure agent | Competing hypothesis investigation |
| infra | coordinator + infrastructure agent + backend agent | Infrastructure changes |
| docs | coordinator + documentation agent | Documentation tasks |
| research | 3 general-purpose agents | Codebase/web investigation (agents spawned as general-purpose, not from agent definitions) |
| security | 4 reviewer agents | OWASP/vulns, auth/access, dependencies, secrets/config audit |
| migration | coordinator + 2 service agents + 1 reviewer | Coordinated refactoring with correctness verification |

**Role keyword matching rules:**
- "coordinator" — agent whose description contains "coordinator", "workflow", or "project" keywords
- "backend" — agent whose description matches backend technologies listed in tech-stack.md
- "frontend" — agent whose description matches frontend technologies listed in tech-stack.md
- "infrastructure" — agent whose description contains "docker", "deploy", "nginx", or "infrastructure"
- "test/backend" — agent whose description contains "test" AND backend technology keywords
- "test/frontend" — agent whose description contains "test" AND ("E2E", "browser", "playwright", or frontend keywords)
- "documentation" — agent whose description contains "documentation", "docs", or "writing"

### Custom Composition

> **If the preset name is not recognized:** List all available presets (from the table above) and the available agents from the registry, then ask the user to choose a valid preset or specify agents by name.

If no preset matches, or user specifies agent names directly, use those. If no preset or agent list is provided, prompt:

```
Which team would you like to spawn?

Presets:
  fullstack          — All service layers + QA (most common)
  backend-feature    — Backend service + backend QA
  frontend-feature   — Frontend service + frontend QA
  review             — All service + QA agents as reviewers
  debug              — Service + infrastructure agents as investigators
  infra              — Infrastructure + backend for infra changes
  docs               — Documentation agent
  research           — 3 general-purpose researchers (codebase + web)
  security           — 4 reviewers covering OWASP, auth, deps, config
  migration          — Coordinator + 2 migrators + 1 verifier

Or specify agents by name: e.g. "{agent1}, {agent2}"
(Available: {comma-separated list of agent names from registry})
```

### Research Preset Details

The research preset spawns 3 general-purpose agents (not from agent definitions):
- researcher-1: Codebase architecture exploration
- researcher-2: Library/documentation research
- researcher-3: Web resources and examples

Each has access to: Grep, Glob, Read, WebSearch, WebFetch, and Task (with subagent_type: Explore).

### Security Preset Details

The security preset spawns 4 reviewer agents:
- vuln-reviewer: OWASP Top 10, injection, XSS, CSRF, deserialization, SSRF
- auth-reviewer: Authentication, authorization, session management
- deps-reviewer: CVEs, supply chain, outdated packages, license risks
- config-reviewer: Hardcoded secrets, env vars, debug endpoints, CORS

### Migration Preset Details

The migration preset spawns 4 agents:
- migration-lead (team-lead): Migration plan, coordination, conflict handling
- migrator-1 (team-implementer): Migration stream 1 (assigned files/modules)
- migrator-2 (team-implementer): Migration stream 2 (assigned files/modules)
- migration-verify (team-reviewer): Verify migrated code correctness and patterns

Dependency pattern:
```
migration-lead (plan) → migrator-1 ──┐
                      → migrator-2 ──┼→ migration-verify
                                     ┘
```

## Step 3: Generate Team Name

Format: `{preset-or-custom}-{short-id}` (4 random alphanumeric chars).

```bash
cat /dev/urandom | LC_ALL=C tr -dc 'a-z0-9' | head -c4
```

> **If the generated team name collides with an existing team** (i.e., `~/.claude/teams/{team-name}/` already exists): Append a random 4-character suffix and retry. If the collision persists after 3 attempts, prompt the user for a custom team name.

## Step 4: Spawn Agents

For each agent in the resolved composition, spawn using the registry data:

```bash
cd {project-root} && claude \
  --agent-id {agent-name}@{team-name} \
  --agent-name {agent-name} \
  --team-name {team-name} \
  --agent-color "{color-from-registry}" \
  --parent-session-id $CLAUDE_SESSION_ID \
  --agent-type {agent-name} \
  --dangerously-skip-permissions \
  --model {model-from-registry-or-inherit}
```

For generic presets (research, security, migration) where agents are not from definition files, use the appropriate `subagent_type` from the agent-teams plugin:
- research: `general-purpose`
- security: `agent-teams:team-reviewer`
- migration: `agent-teams:team-lead`, `agent-teams:team-implementer`, `agent-teams:team-reviewer`

Run each spawn command with `run_in_background: true`.

> **If an agent spawn command fails (non-zero exit):** Retry the spawn once. If it still fails, report the error (including the agent name and exit code), mark that agent as `"status": "failed"` in the team config, and continue spawning the remaining agents. Do not abort the entire team spawn due to a single agent failure.

## Step 5: Register Team

Note track association if `conductor/tracks.md` exists.

## Step 6: Save Team Config

Write to `~/.claude/teams/{team-name}/config.json`:

```json
{
  "name": "{team-name}",
  "preset": "{preset-name-or-custom}",
  "agents": [
    {
      "name": "{agent-name}",
      "color": "{color}",
      "role": "{role-description}",
      "status": "spawning"
    }
  ],
  "created_at": "{ISO-8601 timestamp}",
  "parent_session_id": "{CLAUDE_SESSION_ID}"
}
```

## Step 7: Report

```
Team '{team-name}' spawned successfully.

Agents:
  {color-dot} {agent-name} — {role}
  ...

Preset: {preset-name}
Config: ~/.claude/teams/{team-name}/config.json
```
