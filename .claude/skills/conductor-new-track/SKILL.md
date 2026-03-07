---
name: conductor-new-track
description: Create a new development track with spec, phased plan, and metadata. Auto-detects affected rkgw services and suggests team preset.
argument-hint: "<title> [--type feature|bug|refactor|chore]"
allowed-tools:
  - Bash
  - Read
  - Write
  - Edit
  - Glob
  - Grep
  - AskUserQuestion
---

# Conductor New Track

Create a new development track with a specification, phased implementation plan, and metadata. Automatically detects which rkgw services are affected and suggests the appropriate team preset.

---

## Step 1 — Parse Input

Extract from the argument string:
- **title**: The track title (required). If missing, ask the user.
- **type**: One of `feature`, `bug`, `refactor`, `chore`. Default: `feature`.

Example invocations:
```
/conductor-new-track "Add MCP tool execution caching" --type feature
/conductor-new-track "Fix streaming parser truncation on large responses" --type bug
/conductor-new-track "Refactor converter shared logic"
```

---

## Step 2 — Load Conductor Context

Read these files to understand the project:
- `conductor/setup_state.json` — verify conductor is initialized (check that `status` field is `"complete"`)
- `conductor/tech-stack.md` — service registry (services, languages, frameworks, agents, verify commands)
- `conductor/workflow.md` — TDD policy, commit format, verification commands, Definition of Done
- `conductor/tracks.md` — existing tracks (for duplicate check and ID generation)

If `conductor/setup_state.json` does not exist or its `status` field is not `"complete"`, tell the user to run `conductor-setup` first and stop.

---

## Step 3 — Check for Duplicates

Scan `conductor/tracks.md` for tracks with similar titles. If a potential duplicate is found (fuzzy match on title), warn the user and ask whether to continue or abort.

---

## Step 4 — Auto-detect Affected Services

Analyze the track title and any additional context from the user using the keyword-to-service detection table:

| Keywords | Service Key | Primary Agent |
|----------|-------------|---------------|
| proxy, streaming, converter, auth, middleware, API key, Kiro, token, refresh, format, SSE, Event Stream, thinking, truncation, model, resolver, cache | backend | rust-backend-engineer |
| page, component, dashboard, UI, frontend, React, CSS, CRT, metrics display, config page, admin page, login, SSE hook, apiFetch | frontend | react-frontend-engineer |
| Docker, nginx, deployment, certificate, Let's Encrypt, compose, Dockerfile, proxy-only, health check, TLS | infra | devops-engineer |
| guardrails, CEL, Bedrock, content safety, rule, validation | backend-guardrails | rust-backend-engineer |
| MCP, tool server, tool discovery, client manager, JSON-RPC | backend-mcp | rust-backend-engineer |
| test, spec, unit test, integration test, cargo test | backend-qa | backend-qa |
| E2E, Playwright, browser test, UI test | frontend-qa | frontend-qa |
| documentation, Notion, Slack, runbook, release notes | docs | document-writer |

Present the detected services to the user and ask for confirmation. Allow additions/removals.

---

## Step 5 — Suggest Team Preset

Based on the detected services, suggest a team preset:

| Preset | Agents |
|--------|--------|
| fullstack | scrum-master + rust-backend-engineer + react-frontend-engineer + frontend-qa |
| backend-feature | scrum-master + rust-backend-engineer + backend-qa |
| frontend-feature | scrum-master + react-frontend-engineer + frontend-qa |
| review | rust-backend-engineer + react-frontend-engineer + backend-qa |
| debug | rust-backend-engineer + react-frontend-engineer + devops-engineer |
| infra | scrum-master + devops-engineer + rust-backend-engineer |
| docs | scrum-master + document-writer |

**Suggestion logic:**

| Detected Services | Suggested Preset |
|-------------------|-----------------|
| backend only | backend-feature |
| frontend only | frontend-feature |
| backend + frontend | fullstack |
| infra (any) | infra |
| docs only | docs |

Present the suggestion and allow the user to override. Show the agents from the chosen preset.

---

## Step 6 — Generate Track ID

Format: `{shortname}_{YYYYMMDD}` where:
- `{shortname}` is a lowercase, hyphenated slug derived from the track title (e.g., "Add MCP tool execution caching" becomes `mcp-tool-caching`)
- `{YYYYMMDD}` is today's date

**Slug generation rules:**
1. Extract 2-4 key words from the title (drop articles, prepositions, and common verbs like "add", "fix", "update").
2. Lowercase and join with hyphens.
3. Keep the slug under 30 characters.

**Collision handling:** If a track with the same ID already exists in `conductor/tracks.md`, append a numeric suffix (e.g., `mcp-tool-caching_20260306-2`).

**Examples:**
- "Add MCP tool execution caching" -> `mcp-tool-caching_20260306`
- "Fix streaming parser truncation on large responses" -> `streaming-parser-truncation_20260306`
- "Refactor converter shared logic" -> `converter-shared-logic_20260306`

---

## Step 7 — Gather Specification Details (Interactive Q&A)

**CRITICAL: Ask ONE question per turn.** Use AskUserQuestion for each question, wait for the response, then proceed to the next question. Do NOT batch multiple questions together.

The questions depend on the track type determined in Step 1. Maximum 6 questions per track.

### For `feature` tracks:

1. **Feature Summary**: "Describe the feature in 1-2 sentences."
2. **User Story**: "Who benefits and how? Format: As a [user type], I want to [action] so that [benefit]."
3. **Acceptance Criteria**: "What must be true for this to be complete? List 3-5 criteria."
4. **Dependencies**: "Does this depend on any existing code, APIs, or other tracks?"
5. **Scope Boundaries**: "What is explicitly OUT of scope?"
6. **Technical Considerations** (optional): "Any specific approach or constraints? Press enter to skip."

### For `bug` tracks:

1. **Bug Summary**: "Describe the bug in 1-2 sentences."
2. **Steps to Reproduce**: "What are the exact steps to trigger this bug?"
3. **Expected vs Actual Behavior**: "What should happen vs. what actually happens?"
4. **Affected Areas**: "Which services, pages, or APIs are affected?"
5. **Root Cause Hypothesis** (optional): "Do you have any idea what might be causing this? Press enter to skip."

### For `chore` or `refactor` tracks:

1. **Task Summary**: "Describe the task in 1-2 sentences."
2. **Motivation**: "Why is this needed now? What pain does it address?"
3. **Success Criteria**: "How will we know this is done correctly?"
4. **Risk Assessment**: "What could go wrong? Any areas that need extra caution?"

---

## Step 8 — Generate Phased Plan

Create a phased implementation plan. Each phase contains numbered tasks.

### Phase Ordering Rules

Follow the standard order: **backend → frontend → infrastructure → QA**

Only include phases for detected services. Each task should be:
- Atomic (one clear deliverable)
- Assignable to a specific agent
- Verifiable (has a done condition)

### Phase Templates

#### Backend Phase (if applicable)
```
## Phase 1: Backend
Agent: rust-backend-engineer

- [ ] 1.1 — <Type/model changes>
- [ ] 1.2 — <Business logic implementation>
- [ ] 1.3 — <Route handler>
- [ ] 1.4 — <Unit tests>
```

#### Frontend Phase (if applicable)
```
## Phase 2: Frontend
Agent: react-frontend-engineer

- [ ] 2.1 — <API integration>
- [ ] 2.2 — <Page/component implementation>
- [ ] 2.3 — <Styling and polish>
```

#### Infrastructure Phase (if applicable)
```
## Phase 3: Infrastructure
Agent: devops-engineer

- [ ] 3.1 — <Docker/nginx changes>
- [ ] 3.2 — <Deployment config>
```

#### QA Phase (if applicable)
```
## Phase 4: QA
Agents: backend-qa, frontend-qa

- [ ] 4.1 — <Backend test coverage>
- [ ] 4.2 — <Frontend E2E tests>
```

### Status Markers

| Marker | Meaning | When to Use |
|--------|---------|-------------|
| `[ ]` | Pending | Task not started |
| `[~]` | In Progress | Task currently being worked |
| `[x]` | Complete | Task finished (append commit SHA) |
| `[-]` | Skipped | Intentionally not done (append reason) |
| `[!]` | Blocked | Waiting on dependency (append blocker) |

### TDD Task Insertion

For each backend task, check the TDD policy from `conductor/workflow.md`:
- If the task touches a "required" TDD area (streaming parser, auth token refresh, converter bidirectional, middleware auth chain, guardrails engine), prepend a test-writing subtask.
- If the task touches a "recommended" area (route handlers, HTTP client, model cache, resolver), add a note suggesting tests.
- If the task touches a "skip" area (Docker config, static UI, CSS-only, env variable additions), no test task needed.

---

## Step 9 — Create Track Artifacts

### 9.1 — `conductor/tracks/{track-id}/spec.md`

```markdown
# {track-id}: {title}

**Type**: {type}
**Created**: {ISO date}
**Preset**: {preset name}
**Services**: {comma-separated service keys}

## Problem Statement
{from step 7}

## Acceptance Criteria
{from step 7}

## Scope Boundaries
{from step 7}

## Dependencies
{from step 7}
```

#### Spec Review Gate

After generating the spec, present it to the user with AskUserQuestion:

```
1. Yes, proceed to plan generation
2. No, let me edit
3. Start over with different inputs
```

### 9.2 — `conductor/tracks/{track-id}/plan.md`

```markdown
# {track-id}: Implementation Plan

**Status**: planned
**Branch**: {suggested branch name, e.g., feat/mcp-tool-caching_20260306}

{phased plan from step 8}
```

#### Plan Review Gate

After generating the plan, present it to the user with AskUserQuestion.

### 9.3 — `conductor/tracks/{track-id}/metadata.json`

```json
{
  "id": "{track-id}",
  "title": "{title}",
  "type": "{type}",
  "status": "planned",
  "preset": "{preset name}",
  "services": ["{service-key}"],
  "agents": ["{agent-name}"],
  "branch": "{branch name}",
  "created_at": "{ISO timestamp}",
  "updated_at": "{ISO timestamp}",
  "phases": {
    "1": { "name": "Backend", "status": "pending", "tasks_total": 0, "tasks_done": 0 }
  },
  "commits": []
}
```

### 9.4 — `conductor/tracks/{track-id}/index.md`

```markdown
# Track: {title}

**ID:** {track-id}
**Type:** {type}
**Status:** planned

## Documents
- [Specification](./spec.md)
- [Implementation Plan](./plan.md)

## Progress
- Phases: 0/{N} complete
- Tasks: 0/{M} complete

## Quick Links
- [Back to Tracks](../../tracks.md)
- [Product Context](../../product.md)
```

---

## Step 10 — Register in Track Index

Append a row to `conductor/tracks.md`:

```
| {track-id} | {title} | {type} | planned | {services} | {date} |
```

---

## Step 11 — Report

Print a summary:
```
Track created: {track-id}
  Title: {title}
  Type: {type}
  Preset: {preset name}
  Services: {services}
  Phases: {count} phases, {total tasks} tasks
  Branch: {branch name}

  Artifacts:
    conductor/tracks/{track-id}/index.md
    conductor/tracks/{track-id}/spec.md
    conductor/tracks/{track-id}/plan.md
    conductor/tracks/{track-id}/metadata.json

  Next: Use conductor-implement {track-id} to start implementation.
```

---

## Error Handling

- If conductor is not initialized, direct user to run `conductor-setup`.
- If title is empty, ask for it interactively.
- If service detection is ambiguous, present options and let user choose.
- If the suggested branch name conflicts with an existing branch, append a suffix.
