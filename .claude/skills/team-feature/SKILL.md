---
name: team-feature
description: Coordinated parallel feature development with automated team spawning, task decomposition, and integration verification. Dynamically adapts to any project stack via conductor context.
argument-hint: "[feature-description] [--preset name] [--plan-first]"
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

---

## Step 1: Load Project Context

Read project configuration to build service detection and verification maps:

1. **Read `conductor/tech-stack.md`** to identify:
   - Service categories (e.g., Backend, Frontend, Infrastructure) and their technologies
   - Technology keywords per service (used for scope detection in Step 2)
   - Build/test/lint commands per service (used for verification in Step 7)

2. **Read `.claude/agents/*.md`** frontmatter to build agent registry:
   - Map each agent's description keywords to the service categories from tech-stack.md
   - Result: a `service-to-agent` map (e.g., Backend -> agent whose description matches backend technologies)

3. **Build keyword detection table** from tech-stack.md. For each service category, extract:
   - Technology names (e.g., "Axum", "React", "nginx")
   - Component names (e.g., "Web framework", "Build tool", "Reverse proxy")
   - Related terms from the Notes column
   - Common directory patterns (scan project structure for service directories)

4. **Build verification command map** from tech-stack.md. For each service, determine:
   - The lint command (if a linter is listed)
   - The build command (if a build tool is listed)
   - The test command (if a test framework is listed)
   - The project subdirectory (inferred from tech-stack.md scope or directory structure)

## Step 2: Analyze Scope

Analyze the feature description against the keyword detection table built in Step 1.

For each service category from tech-stack.md:
- Check if feature description contains any of that service's keywords
- Scan for file paths mentioned in the description (match against project directory structure)
- Determine which services are affected

Map affected services to agents using the service-to-agent map from Step 1.

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

## Step 5: Spawn Team

Use `/team-spawn` with the selected preset. Agent names and colors are resolved dynamically by team-spawn from the agent registry — do not hardcode them here.

## Step 6: Assign Work Streams

Send each agent their assignment via `SendMessage`. Include in each assignment:

1. **Owned files** — explicit list, no overlaps
2. **Requirements** — specific deliverables for this work stream
3. **Interface contracts** — shared types/APIs this agent must implement or consume
4. **Wave number** — when this work stream should begin
5. **Dependencies** — which other work streams must complete first
6. **Acceptance criteria** — how to verify this work stream is complete

Wave execution:
- Start Wave 1 agents immediately
- Start Wave 2 agents after Wave 1 APIs/contracts are ready
- Start Wave 3 agents after feature code is substantially complete
- Start Wave 4 agents after implementation is stable

## Step 7: Integration Verification

Run verification commands dynamically based on the verification command map built in Step 1.

For each affected service, run its lint, build, and test commands:

```
For each service in affected_services:
  cd {project-root}/{service-subdirectory} && {lint-command} && {test-command}
```

If no commands were found in tech-stack.md for a service, skip verification for that service and note it in the report.

### Cross-Service Contract Validation

For each interface contract defined in Step 4, verify:
1. Both sides of the contract are implemented
2. Types/schemas match across the boundary
3. Integration points are wired correctly

Use Grep to verify contract compliance:
- Search for endpoint paths, function names, or type names from the contracts
- Confirm they exist on both sides of each service boundary

### Final Report

```
Feature: {description}
Team: {team-name}
Status: {COMPLETE / NEEDS_ATTENTION}

Work Streams:
  {agent-name}: {status} — {summary}

Verification:
  {service-name}: {PASS/FAIL} ({commands run})
  ...

Cross-Service Contracts:
  {contract-name}: {VERIFIED / MISMATCH}
  ...
```
