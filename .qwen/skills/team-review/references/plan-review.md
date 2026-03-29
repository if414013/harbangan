# Plan Review Checklist for Harbangan

Use this when `team-review --plan <path>` is selected.

## What to look for

### Scope and intent

- Does the plan solve the stated problem directly, or does it drift into adjacent work?
- Are the success criteria concrete enough to verify?
- Are important in-scope behaviors missing?
- Is anything listed that should clearly be out of scope?

### Ownership and boundaries

- Do task assignments respect the ownership rules from `.claude/agents/*.md` and `.claude/rules/team-coordination.md`?
- Are shared files called out where ownership is split, such as `config_db.rs`, `routes/mod.rs`, `Cargo.toml`, or `frontend/package.json`?
- Does the plan accidentally assign code-writing work to QA-only or docs-only domains?

### Dependency order

- Are foundational changes ordered before consumers?
- Are migrations, API contracts, and types defined before dependent frontend or test work?
- Does the plan describe parallelism only where interfaces are stable enough to make it safe?

### Verification

- Are the right checks listed for each affected area?
- Are critical edge cases and regression risks covered by the proposed tests?
- If the plan changes auth, config, migrations, streaming, or UI behavior, does it include targeted verification for those areas?

### Risk and omissions

- Are failure modes, compatibility risks, or rollout hazards ignored?
- Does the plan rely on assumptions that are not stated?
- Are there hidden prerequisites such as environment variables, data migration expectations, or branch/PR workflow assumptions?

### Clarity

- Is the plan decision-complete, or does it leave important implementation choices unresolved?
- Are contradictions present between sections or tasks?
- Would another engineer know what to do next without guessing?

## Severity guide

- `high`: the plan is likely to cause incorrect implementation, broken ownership, or major missing behavior
- `medium`: the plan leaves a meaningful gap, ambiguity, or validation hole
- `low`: the plan is mostly sound but could be tightened to avoid confusion
- `info`: useful observation or follow-up suggestion

## Reporting rules

- Cite the plan file with line numbers when possible.
- If line numbers are awkward, cite the exact section heading.
- Prefer concrete gaps over style commentary.
- If a plan references code or repo behavior incorrectly, call that out explicitly.
