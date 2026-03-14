---
name: team-review
description: Launch a multi-reviewer parallel code review organized by quality dimensions (Security, Performance, Architecture, Testing, Accessibility). Use when user says 'review this code', 'security review', 'review my PR', 'code quality check', or 'architecture review'.
argument-hint: "<target> [--reviewers security,performance,architecture,testing,accessibility] [--base-branch main]"
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

# Team Review

Orchestrate a multi-reviewer parallel code review where each reviewer focuses on a specific quality dimension. Produces a consolidated, deduplicated report organized by severity.

Refer to `references/review-dimensions.md` for detailed per-dimension checklists.

## Critical Constraints

- **Agent teams required** — `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` must be set
- **Report only** — never auto-fix findings; reviewers report issues but do not modify code
- **Deduplicate findings** — merge identical findings across reviewers (same file:line, same issue) into a single entry crediting all dimensions
- **Clean up after completion** — shut down all reviewer agents after the consolidated report is produced

---

## Pre-flight Checks

1. Verify `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` is set
2. Parse `$ARGUMENTS`:
   - `<target>`: file path, directory, git diff range (e.g., `main...HEAD`), or PR number (e.g., `#123`)
   - `--reviewers`: comma-separated dimensions (default: `security,performance,architecture`)
   - `--base-branch`: base branch for diff comparison (default: `main`)
3. Create a GitHub Issue for the review task and add to the Harbangan Board:
   ```bash
   gh issue create --title "[review]: {dimensions} review of {target}" \
     --label "chore,priority:p1" \
     --project "Harbangan Board" \
     --body "Multi-dimensional code review: {dimensions}"
   ```
   Set board Status → In progress when review starts.

## Phase 1: Target Resolution

1. Determine target type:
   - **File/Directory**: Use as-is for review scope
   - **Git diff range**: `git -C /Users/hikennoace/ai-gateway/harbangan diff {range} --name-only`
   - **PR number**: `gh pr diff {number} --name-only`
   - **Default** (no target): `git -C /Users/hikennoace/ai-gateway/harbangan diff main...HEAD --name-only`
2. Collect full diff content for distribution to reviewers
3. Display review scope: "{N} files to review across {M} dimensions"

## Phase 2: Dimension Assignment

### Available Dimensions

| Dimension         | Focus Area                                          | Assign When                           |
|-------------------|-----------------------------------------------------|---------------------------------------|
| **Security**      | Auth, secrets, injection, CSRF, CORS, token handling | Always for auth/middleware/API changes |
| **Performance**   | Async patterns, caching, pooling, streaming, renders | Data access, hot paths, streaming     |
| **Architecture**  | Module boundaries, error handling, patterns, coupling | Structural changes, new modules       |
| **Testing**       | Coverage, edge cases, mocking, test naming           | New functionality or refactors        |
| **Accessibility** | WCAG 2.1 AA, keyboard nav, screen reader support     | Frontend/UI changes                   |

### Recommended Presets

| Changed Files Touch               | Default Dimensions                           |
|------------------------------------|----------------------------------------------|
| `backend/src/auth/`, `middleware/` | Security, Performance, Architecture          |
| `backend/src/converters/`, `streaming/` | Performance, Architecture, Testing      |
| `backend/src/routes/`, `web_ui/`   | Security, Performance, Architecture          |
| `backend/src/guardrails/`  | Security, Architecture, Testing              |
| `frontend/src/`                    | Architecture, Testing, Accessibility         |
| `docker-compose*`, `Dockerfile`    | Security, Architecture                       |
| Mixed backend + frontend           | Security, Performance, Architecture, Testing |

## Phase 3: Spawn Reviewers

1. Generate team name: `review-{short-id}`
2. Create the team using `TeamCreate`:
   ```
   TeamCreate({ team_name: "review-{short-id}", description: "Code review: {dimensions}" })
   ```
3. For each requested dimension, spawn a reviewer using the `Agent` tool:
   ```
   Agent({
     name: "{dimension}-reviewer",
     team_name: "review-{short-id}",
     subagent_type: "general-purpose",
     description: "Review {dimension}",
     prompt: "You are a {dimension} reviewer. {dimension assignment, checklist, target files, diff content}",
     run_in_background: true
   })
   ```
   Spawn all reviewers in parallel (single message with multiple Agent calls).

### Reviewer-to-Agent Mapping

Each dimension reviewer draws on domain expertise from the appropriate agent:

| Dimension     | Primary Agent               | Also Consults         |
|---------------|-----------------------------|-----------------------|
| Security      | rust-backend-engineer       | devops-engineer       |
| Performance   | rust-backend-engineer       | react-frontend-engineer |
| Architecture  | rust-backend-engineer       | react-frontend-engineer |
| Testing       | backend-qa                  | frontend-qa           |
| Accessibility | react-frontend-engineer     | --                    |

### Harbangan Codebase Context for Reviewers

Provide each reviewer with relevant context so they know where to look:

- **Backend modules**: `backend/src/` -- `auth/`, `converters/`, `streaming/`, `routes/`, `middleware/`, `guardrails/`, `web_ui/`, `models/`, `metrics/`
- **Frontend**: `frontend/src/` -- `pages/`, `components/`, `lib/`, `styles/`
- **Infrastructure**: `docker-compose.yml`, `docker-compose.gateway.yml`, `Dockerfile`
- **Error handling**: `thiserror` enums + `anyhow::Result` with `.context()`
- **Logging**: `tracing` macros with structured fields
- **Tests**: `#[cfg(test)] mod tests` at bottom of each file, `test_<fn>_<scenario>` naming

## Phase 4: Monitor and Collect

1. Wait for all review tasks to complete (check periodically)
2. Track progress: "{completed}/{total} dimension reviews complete"
3. Collect structured findings from each reviewer

## Phase 5: Consolidation

### Severity Calibration

| Severity     | Impact                                       | Likelihood             | Examples                                        |
|--------------|----------------------------------------------|------------------------|-------------------------------------------------|
| **CRITICAL** | Data loss, security breach, complete failure | Certain or very likely | SQL injection, auth bypass, token leak          |
| **HIGH**     | Significant functionality impact             | Likely                 | Memory leak, missing validation, broken stream  |
| **MEDIUM**   | Partial impact, workaround exists            | Possible               | N+1 query, missing edge case, poor error msg    |
| **LOW**      | Minimal impact, cosmetic                     | Unlikely               | Style issue, minor optimization, naming         |
| **INFO**     | Observation, suggestion, no immediate impact | N/A                    | Pattern recommendation, documentation gap       |

### Calibration Rules

- Security vulnerabilities exploitable by external users: always CRITICAL or HIGH
- Token/session handling bugs: at least HIGH
- Performance issues in streaming hot paths: at least MEDIUM
- Missing tests for critical paths (auth, converters): at least MEDIUM
- Accessibility violations for core web UI functionality: at least MEDIUM
- Import ordering, naming style issues: LOW or INFO

### Deduplication Rules

1. **Same file:line, same issue** -- Merge into one finding, credit all dimensions
2. **Same file:line, different issues** -- Keep as separate findings
3. **Same issue, different locations** -- Keep separate but cross-reference
4. **Conflicting severity** -- Use the higher severity rating
5. **Conflicting recommendations** -- Include both with dimension attribution

## Phase 6: Verification

Run automated checks to validate the codebase state:

```bash
cd /Users/hikennoace/ai-gateway/harbangan/backend && cargo clippy --all-targets 2>&1 | tail -20
cd /Users/hikennoace/ai-gateway/harbangan/backend && cargo test --lib 2>&1 | tail -5
cd /Users/hikennoace/ai-gateway/harbangan/frontend && npm run build 2>&1 | tail -5
cd /Users/hikennoace/ai-gateway/harbangan/frontend && npm run lint 2>&1 | tail -5
```

## Phase 7: Report

Present the consolidated report:

```markdown
## Code Review Report

**Target**: {files/PR/diff range}
**Dimensions**: {security, performance, architecture, ...}
**Date**: {date}
**Files Reviewed**: {count}

### CRITICAL ({count})

#### [CR-001] {Title}
**Location**: `{file}:{line}`
**Dimension**: {Security/Performance/etc.}
**Description**: {what was found}
**Impact**: {what could happen}
**Fix**: {recommended remediation}

### HIGH ({count})
...

### MEDIUM ({count})
...

### LOW ({count})
...

### INFO ({count})
...

### Summary

| Dimension     | CRITICAL | HIGH | MEDIUM | LOW | INFO | Total |
|---------------|----------|------|--------|-----|------|-------|
| Security      | ...      | ...  | ...    | ... | ...  | ...   |
| Performance   | ...      | ...  | ...    | ... | ...  | ...   |
| Architecture  | ...      | ...  | ...    | ... | ...  | ...   |
| Testing       | ...      | ...  | ...    | ... | ...  | ...   |
| Accessibility | ...      | ...  | ...    | ... | ...  | ...   |
| **Total**     | ...      | ...  | ...    | ... | ...  | ...   |

### Verification

| Check             | Status      |
|-------------------|-------------|
| cargo clippy      | PASS / FAIL |
| cargo test --lib  | PASS / FAIL |
| npm run build     | PASS / FAIL |
| npm run lint      | PASS / FAIL |

### Recommendation

{Overall assessment and prioritized action items}
```

## Phase 8: Cleanup

1. Update the review GitHub Issue board Status → Done
2. Send `shutdown_request` to all reviewer teammates via `SendMessage`
3. Use `TeamDelete` to remove team and task directories
