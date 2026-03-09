# Plan Mode Rules

Applies when Claude Code is in Plan mode.

## Mandatory Agent Consultation

Before writing any non-trivial plan, you MUST consult domain agents for feasibility and scope input. This is not optional — plans written without agent consultation will miss implementation details.

### Step 1: Identify affected services

Use the Service Map in CLAUDE.md to determine which domains the task touches.

### Step 2: Spawn domain consultants in parallel

For each affected service, spawn an Explore agent that reads the corresponding agent definition and investigates the codebase from that agent's perspective. Run all consultations in parallel:

| Affected Service | Agent to Consult | What to Ask |
|-----------------|-----------------|-------------|
| Backend (Rust/Axum) | `rust-backend-engineer` | Existing patterns, affected modules, type changes, error handling approach |
| Frontend (React/TS) | `react-frontend-engineer` | Component structure, API integration points, styling approach, state management |
| Infrastructure | `devops-engineer` | Docker/nginx impact, env vars, deployment changes |
| Backend tests | `backend-qa` | Test coverage gaps, which test patterns to follow, integration test needs |
| Frontend tests | `frontend-qa` | E2E test scenarios, Playwright page objects to update |

Prompt template for each consultant:
```
Read .claude/agents/{agent}.md for your role context, then investigate:
1. What existing code/patterns are relevant to: {task description}
2. What files would need to change?
3. What risks or gotchas should the plan account for?
4. Estimated complexity (small/medium/large)?
```

### Step 3: Clarify ambiguities with the user

After gathering agent findings, identify any open questions before writing the plan. Use AskUserQuestion to resolve ambiguities — do NOT make assumptions. Common things to clarify:

- Scope boundaries (e.g., "Should this also handle X, or just Y?")
- Design trade-offs surfaced by agents (e.g., "Agent found two approaches — A is simpler, B is more extensible. Which do you prefer?")
- Missing requirements (e.g., "Should this support streaming, or non-streaming only?")
- Priority conflicts (e.g., "This touches auth and config — which should we tackle first?")

If the task is unambiguous and agent findings don't raise questions, skip this step. But when in doubt, ask — a 30-second clarification beats rewriting a plan.

### Step 4: Incorporate findings into the plan

The plan MUST reference specific findings from each consultant. Include a "Consultation Summary" section listing what each agent reported and how it influenced the plan.

## Plan Output Format

Every non-trivial plan must include:
1. **Consultation Summary** — what each domain agent reported
2. **Agent Assignment** — maps tasks to agents:

### Task Decomposition

Structure tasks in waves for parallel execution:

- **Wave 1** (foundations): Backend types, DB migrations, core logic
  - Assigned to: `rust-backend-engineer`
- **Wave 2** (consumers): Frontend pages, API integration
  - Assigned to: `react-frontend-engineer`
- **Wave 3** (verification): Unit tests, E2E tests
  - Assigned to: `backend-qa`, `frontend-qa`

### Team Preset Recommendation

Based on affected services, recommend a team preset:
- Backend only → `backend-feature`
- Frontend only → `frontend-feature`
- Both → `fullstack`
- Infrastructure → `infra`

### File Ownership

Assign each file to exactly one agent. No overlaps.

## Rules Reference

Read `.claude/rules/*.md` to ensure plans follow project conventions:
- `backend.md` — Rust/Axum patterns, error handling, testing
- `web-ui.md` — React 19, TypeScript, CRT aesthetic, API patterns
