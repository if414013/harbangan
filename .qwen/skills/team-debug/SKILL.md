---
name: team-debug
description: Investigate a bug with competing hypotheses and parallel subagents. Use when the user asks to debug an error, find a root cause, or investigate why something is failing.
---

# Team Debug

Run a structured, read-first debugging pass before making fixes.

## Workflow

1. Capture the symptom, scope, reproduction hints, and any logs or errors the user provided.
2. Read the relevant repo rules, the updated `.claude/agents/*.md` ownership guidance, and `references/hypothesis-testing.md`.
3. Generate 2 to 4 competing hypotheses across different failure categories when possible.
4. Spawn read-only investigators from `.qwen/agents/`, choosing the smallest relevant subset of:
   - `rust-backend-engineer`
   - `react-frontend-engineer`
   - `database-engineer`
   - `devops-engineer`
   - `backend-qa`
   - `frontend-qa`
   - `document-writer` for clarity, docs, or expectation mismatches when relevant
5. Give each investigator one hypothesis, the files to inspect, and the confirming or falsifying evidence to look for.
6. Require evidence with `file:line` references and explicit verdicts such as confirmed, probable, inconclusive, or ruled out.
7. Consolidate the results into:
   - most likely root cause
   - supporting evidence
   - contradicting evidence
   - recommended fix path
   - remaining unknowns

## Constraints

- The custom investigators are advisory-only and read-only. They inspect and explain; they do not edit files.
- Do not edit code unless the user explicitly asks to move from investigation to fixing.
- If the evidence is weak, say so plainly and explain what additional data would settle it.
