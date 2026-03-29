---
name: team-review
description: Review Harbangan plans and code with read-only advisory agents. Use `--plan <path>` for domain-driven plan review, and `--code [target]` for fixed reviewer-dimension code review with domain-advisor consultation.
---

# Team Review

Report findings only. Do not auto-fix issues unless the user explicitly pivots from review to implementation.

## Workflow

1. Parse mode explicitly. Supported forms:
   - `team-review --plan <path>`
   - `team-review --code`
   - `team-review --code <file-or-dir>`
   - `team-review --code <diff-range>`
   - `team-review --code <pr-number>`
2. If `--plan` is selected:
   - require an explicit file or directory path
   - if the path is a directory, review the newest `*.md` file in that directory
   - read `references/plan-review.md`
   - spawn the relevant read-only domain agents from `.qwen/agents/`
   - ask each agent to review feasibility, ownership, dependency order, verification, omissions, and contradictions with repo conventions
3. If `--code` is selected:
   - with no explicit target, review the current branch diff against the default branch
   - otherwise resolve the target as a file, directory, diff range, or PR number
   - read `references/review-dimensions.md`
   - always spawn these 5 fixed reviewer dimensions:
     - `security`
     - `performance`
     - `architecture`
     - `testing`
     - `accessibility`
   - instruct each fixed reviewer to consult only the relevant read-only domain advisors from `.qwen/agents/`
   - require every fixed reviewer to return either concrete findings or an explicit `no findings in my dimension` result
4. Fixed-reviewer consultation rules for `--code`:
   - `security` consults backend, devops, and frontend when auth, sessions, or browser security are involved
   - `performance` consults backend, database, frontend, and devops when runtime or infrastructure behavior matters
   - `architecture` consults backend, frontend, database, and devops depending on touched layers
   - `testing` consults backend-qa and frontend-qa
   - `accessibility` consults frontend and frontend-qa
   - `document-writer` is consult-only when a code change creates user-facing config, docs, or communication implications
5. Require each fixed reviewer to cite concrete evidence with file locations, note which domain advisor(s) it consulted, and avoid style-only commentary unless it hides a real bug.
6. Consolidate duplicate findings and present the report ordered by severity.
7. If you ran automated checks, include their status. If you did not, say so.

## Output

- Findings first, with severity and location.
- For `--code`, include `Dimension:` and `Consulted domain agent(s):` for each finding.
- Then open questions or testing gaps.
- Then a brief summary only if it adds value.

## Constraints

- If there are no findings, state that explicitly and mention residual risks or missing verification.
- `--plan` without a path is invalid; require the user to supply a plan target.
- Use the custom agents as read-only advisors only. They must not write files or propose that they made changes.
- `--plan` remains domain-driven; the fixed 5-dimension reviewer model applies only to `--code`.
- For `--code`, all 5 fixed reviewers always run even when some dimensions are ultimately no-ops.
- Do not fabricate active reviewer state outside the current conversation.
