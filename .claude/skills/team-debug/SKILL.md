---
name: team-debug
description: ACH-based debugging — spawns domain investigators to evaluate competing hypotheses across the Harbangan stack using formal evidence standards and arbitration. Use when user says 'debug this error', 'why is this failing', 'investigate bug', 'root cause analysis', or 'something is broken'.
argument-hint: "[error-description] [--scope path] [--hypotheses count]"
allowed-tools:
  - Bash
  - Read
  - Grep
  - Glob
  - Write
  - SendMessage
  - AskUserQuestion
  - TeamCreate
  - TeamDelete
  - Agent
  - TaskCreate
  - TaskUpdate
  - TaskList
---

# Team Debug — Analysis of Competing Hypotheses

Structured debugging methodology that spawns multiple AI investigators to evaluate competing hypotheses in parallel. Based on the ACH (Analysis of Competing Hypotheses) framework, adapted for the Harbangan stack.

See `references/hypothesis-testing.md` for templates, decision trees, and Harbangan-specific examples.

## Critical Constraints

- **Agent teams required** — `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` must be set
- **Read-only investigators** — investigators must not modify code; their sole job is to collect evidence and report verdicts
- **Formal evidence standards** — all evidence must be classified by type (Direct, Correlational, Testimonial, Absence) with file:line citations
- **Report only** — never auto-fix the root cause; present findings, causal chain, and suggested fix in the debug report

---

## Prerequisites

- `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` must be enabled
- Provide one of: error description, file path, log snippet, or API response
- Optional: `--hypotheses N` (default 3), `--scope path` (limit investigation directory)

---

## Phase 1: Initial Triage

Create a GitHub Issue for the debug investigation and add to the Harbangan Board:
```bash
gh issue create --title "[bug]: Investigate {error summary}" \
  --label "bug,priority:{p0|p1|p2}" \
  --project "Harbangan Board" \
  --body "Debug investigation using ACH methodology.\n\nSymptom: {description}"
```
Set board Status → In progress, Priority based on severity.

Analyze the error or symptom to establish the investigation baseline.

### 1.1 Gather Context

If no error description is provided, ask:
1. What is the symptom? (error message, unexpected behavior, crash)
2. When does it occur? (always, intermittently, after specific action)
3. Where was it observed? (proxy endpoint, web UI, Docker logs, test suite)
4. Is it reproducible? (steps to reproduce, frequency)

Collect all available artifacts:
- Stack traces and panic messages
- Error messages and HTTP status codes
- Log snippets (`tracing` output, Docker logs)
- API request/response pairs
- Recent git changes (`git log --oneline -10`, `git diff HEAD~3`)

### 1.2 Classify Error Domain

Route to the appropriate investigator agent(s) based on error indicators.

| Error Indicators | Domain | Investigator |
|-----------------|--------|--------------|
| Rust panic, `unwrap()` failure, Axum error, `anyhow::Error`, `ApiError` | Backend | rust-backend-engineer |
| Streaming parse error, Event Stream, truncation, SSE disconnect | Backend (Streaming) | rust-backend-engineer |
| Auth failure, token expired, 401/403, session invalid, CSRF mismatch | Backend (Auth) | rust-backend-engineer |
| Converter error, format mismatch, missing field, serde error | Backend (Converters) | rust-backend-engineer |
| Guardrails block, CEL evaluation, Bedrock timeout | Backend (Guardrails) | rust-backend-engineer |
| MCP connection failure, tool execution error, JSON-RPC error | Backend (MCP) | rust-backend-engineer |
| React error, TypeScript error, component crash, blank page | Frontend | react-frontend-engineer |
| SSE not connecting, metrics not updating, apiFetch error | Frontend | react-frontend-engineer |
| Docker build failure, container crash, port conflict | Infrastructure | devops-engineer |
| PostgreSQL connection error, migration failure, query timeout | Backend (Database) | rust-backend-engineer |

### Debug Presets

| Preset | Agents |
|--------|--------|
| backend | rust-backend-engineer |
| frontend | react-frontend-engineer |
| infra | devops-engineer |
| fullstack | rust-backend-engineer + react-frontend-engineer + devops-engineer |

---

## Phase 2: Hypothesis Generation

Generate N competing hypotheses (default 3, override with `--hypotheses`). Each hypothesis MUST:

1. Be a **clear, falsifiable statement** about the root cause
2. Be assigned to exactly one **failure mode category**
3. Define **confirming evidence** (what would prove it)
4. Define **falsifying evidence** (what would disprove it)
5. Specify **files and scope** for investigation

### 6 Failure Mode Categories

Every hypothesis must be classified into one of these categories:

| Category | Description | Typical Indicators |
|----------|-------------|--------------------|
| **Logic Error** | Incorrect conditional, wrong algorithm, edge case not handled | Wrong output, panic on specific input, test failure |
| **State Corruption** | Stale cache, inconsistent shared state, mutation without synchronization | Intermittent wrong results, data changes unexpectedly |
| **Resource Exhaustion** | Connection pool drained, memory leak, file descriptor limit | Timeout, 503, OOM kill, increasing latency over time |
| **Integration Failure** | API contract mismatch, version incompatibility, protocol error | 4xx/5xx from external service, deserialization error |
| **Configuration Error** | Missing env var, wrong value, config not persisted to DB | Works in one environment but not another, 500 on startup |
| **Race Condition** | Concurrent access without proper synchronization, ordering assumption | Intermittent failure, works under low load, fails under high load |

### Harbangan-Specific Hypothesis Patterns

Use these as starting points when generating hypotheses for common Harbangan issues:

| Pattern | Category | Key Files |
|---------|----------|-----------|
| Malformed AWS Event Stream chunk, missing end marker | Logic Error | `backend/src/streaming/mod.rs` |
| Kiro token expired but cached (4-min TTL race) | Race Condition | `backend/src/auth/`, AppState `kiro_token_cache` |
| OpenAI/Anthropic field not mapped in converter | Logic Error | `backend/src/converters/` |
| API key hash not in cache, DB lookup fails | State Corruption | `backend/src/middleware/`, `backend/src/web_ui/api_keys.rs` |
| Model alias not in resolver, no fallback | Configuration Error | `backend/src/resolver.rs`, `backend/src/cache.rs` |
| Frontend useSSE hook loses connection, no reconnect | Logic Error | `frontend/src/lib/useSSE.ts` |
| CORS middleware blocks legitimate request | Configuration Error | `backend/src/middleware/` |
| Backend not reachable from frontend, wrong port | Configuration Error | `docker-compose.yml`, `frontend/vite.config.ts` |
| Runtime config change lost on restart | State Corruption | `backend/src/web_ui/config_db.rs` |
| Guardrails CEL rule rejects valid content | Logic Error | `backend/src/guardrails/` |
| Session cache grows unbounded, no eviction | Resource Exhaustion | AppState `session_cache` |

### Hypothesis Format

For each hypothesis, produce:

```
Hypothesis {N}: {Title}
Category: {Failure Mode Category}
Statement: {Clear, falsifiable claim about root cause}
Confirming evidence: {What we expect to find if true}
Falsifying evidence: {What we expect to find if false}
Scope: {Files/directories to investigate}
Assigned to: {Investigator agent}
```

---

## Phase 3: Investigation

### 3.1 Spawn Investigators

Generate team name: `debug-{short-id}`.

1. Create the team using `TeamCreate`:
   ```
   TeamCreate({ team_name: "debug-{short-id}", description: "ACH debug investigation" })
   ```

2. Spawn investigator agents using the `Agent` tool with `team_name` parameter. Use the domain-specific agents identified in Phase 1:
   ```
   Agent({
     name: "{agent-name}",
     team_name: "debug-{short-id}",
     subagent_type: "{agent-name}",
     description: "Investigate hypothesis {N}",
     prompt: "You are an investigator on debug team. Read-only — do not modify code. {hypothesis details}",
     run_in_background: true
   })
   ```

   Spawn all investigators in parallel (single message with multiple Agent calls).

### 3.2 Assign Investigations

Send each investigator a task using the **Hypothesis Task Template** (see `references/hypothesis-testing.md`). Each investigator receives:

1. The specific hypothesis to evaluate (not all hypotheses)
2. The failure mode category and what it implies
3. Files and scope to examine
4. Confirming and falsifying evidence criteria
5. The **Evidence Report Template** to use for their findings

Investigators must NOT pursue fixes. Their job is to **collect evidence** and **report a verdict**.

---

## Phase 4: Evidence Collection

### 4 Evidence Standards

All evidence collected by investigators must be classified by type and strength:

| Evidence Type | Strength | Description | Example |
|---------------|----------|-------------|---------|
| **Direct** | Strong | Code that directly proves or disproves the hypothesis | `streaming/mod.rs:142` — buffer not flushed before return |
| **Correlational** | Medium | Timing, patterns, or co-occurrence that suggests causation | Error only occurs when token cache TTL expires at same time as request |
| **Testimonial** | Weak | Log messages, error strings, or user reports | `error!(error = ?err, "Failed to refresh Kiro token")` in logs |
| **Absence** | Variable | Expected evidence that is missing (can confirm or falsify) | No error handling for `None` return from `model_cache.get()` |

### Evidence Requirements

- Every piece of evidence must include a **file:line citation**
- Direct evidence requires the actual code snippet
- Correlational evidence must describe the observed pattern
- Absence evidence must explain what was expected and why it matters
- Each investigator must report at least one piece of confirming OR falsifying evidence

### Monitor Progress

Poll investigators for status. If an investigator is stuck:
- Suggest additional files to examine
- Provide context from other investigators (without revealing their conclusions)
- Set a time bound — if no evidence after thorough search, report INCONCLUSIVE

---

## Phase 5: Arbitration

Cross-reference all investigation reports using the formal arbitration process.

### 5.1 Classify Each Hypothesis

Based on the evidence collected, assign each hypothesis a verdict:

| Verdict | Confidence | Criteria |
|---------|------------|----------|
| **CONFIRMED** | >80% | Multiple pieces of direct evidence, clear causal chain, no contradicting evidence |
| **PROBABLE** | 50-80% | Correlational evidence supports, some direct evidence, minor gaps in causal chain |
| **INCONCLUSIVE** | <50% | Mixed evidence, unable to confirm or falsify, needs more investigation |
| **RULED_OUT** | N/A | Direct falsifying evidence found, or contradicting evidence from another confirmed hypothesis |

### 5.2 Arbitration Decision Tree

```
All investigators reported?
|
+-- NO --> Wait for remaining reports
|
+-- YES --> Count CONFIRMED hypotheses
    |
    +-- 0 confirmed
    |   |
    |   +-- Any PROBABLE? --> Promote highest-confidence PROBABLE
    |   |   to focused investigation (new round, narrower scope)
    |   |
    |   +-- All INCONCLUSIVE/RULED_OUT? --> Generate new hypotheses
    |       (different failure mode categories), return to Phase 2
    |
    +-- 1 confirmed
    |   |
    |   +-- Confidence >80%?
    |       |
    |       +-- YES --> Declare root cause, proceed to fix
    |       |
    |       +-- NO --> Flag as likely cause, recommend
    |           targeted verification before fix
    |
    +-- 2+ confirmed
        |
        +-- Are they causally related?
            |
            +-- YES --> Compound issue: identify primary cause
            |   and contributing factors, fix in dependency order
            |
            +-- NO --> Rank by confidence level, declare
                highest as primary root cause
```

### 5.3 Evidence Conflict Resolution

When investigators present contradicting evidence:
1. Weigh by evidence strength: Direct > Correlational > Testimonial
2. Check for scope differences (same code path? different conditions?)
3. Look for compound causation (both could be true under different conditions)
4. If unresolvable, spawn a tiebreaker investigation with combined scope

---

## Phase 6: Cleanup and Reporting

### 6.1 Generate Debug Report

```
Debug Report
============
Error: {description}
Team: {team-name}
Methodology: Analysis of Competing Hypotheses (ACH)

Triage Summary:
  Domain: {classified domain}
  Symptoms: {observed symptoms}
  Scope: {files/modules investigated}

Root Cause:
  Hypothesis: {confirmed hypothesis title}
  Category: {failure mode category}
  Verdict: {CONFIRMED|PROBABLE}
  Confidence: {percentage}
  Investigator: {agent}

  Causal Chain:
    1. {First cause} -->
    2. {Intermediate effect} -->
    3. {Observable symptom}

Evidence:
  Direct:
    1. {file:line -- description}
  Correlational:
    1. {file:line -- description}
  Absence:
    1. {expected evidence -- why missing matters}

Other Hypotheses:
  - {hypothesis 2}: {RULED_OUT|INCONCLUSIVE} -- {brief evidence summary}
  - {hypothesis 3}: {RULED_OUT|INCONCLUSIVE} -- {brief evidence summary}

Suggested Fix:
  {specific code changes with file paths and line numbers}

Prevention:
  {recommendations to prevent recurrence}
  {additional test cases to add}
```

### 6.2 Cleanup

- Update the debug GitHub Issue board Status → Done
- Terminate all investigator agents via `SendMessage` with `type: "shutdown_request"`
- Save report to `~/.claude/teams/{team-name}/debug-report.md`
- Use `TeamDelete` to remove team and task directories
