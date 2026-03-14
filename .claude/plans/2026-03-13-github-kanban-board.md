# Plan: Centralize Task Management on GitHub Project Board (Kanban)

## Context

The `.claude/` workflow infrastructure currently references an old GitHub Project board (project ID `PVT_kwHOATKEhs4BRm0k`) with stale field IDs. The user wants to switch to a new board — **"Harbangan Board"** (project #3, `PVT_kwHOATKEhs4BRp0j`) — as the centralized task management system for all agent workflows. Additionally, the `scrum-master` agent is being renamed to `kanban-master` to better reflect the kanban-style board workflow.

The goal: every code change, refactor, bug fix, and enhancement flows through the GH Project kanban board. All skills (`team-plan`, `team-implement`, `team-review`, `team-debug`) route task tracking through the kanban-master, which syncs to the board.

## New Board Constants

```
PROJECT_ID     = PVT_kwHOATKEhs4BRp0j
PROJECT_NUMBER = 3
OWNER          = if414013

STATUS_FIELD   = PVTSSF_lAHOATKEhs4BRp0jzg_azo8
  Backlog=f75ad846, Ready=61e4505c, In progress=47fc9ee4, In review=df73e18b, Done=98236657

PRIORITY_FIELD = PVTSSF_lAHOATKEhs4BRp0jzg_azuA
  P0=79628723, P1=0a877460, P2=da944a9c

SIZE_FIELD     = PVTSSF_lAHOATKEhs4BRp0jzg_azuE
  XS=6c6483d2, S=f784b110, M=7515a9f1, L=817d0097, XL=db339eb2
```

Key differences from old board:
- No custom "Service" field — use labels (`service:backend`, `service:frontend`, etc.) instead
- Status column "To Do" is now "Ready"
- Priority simplified to P0/P1/P2 (was P0-Critical through P3-Low)
- New "Size" field (XS/S/M/L/XL) replaces complexity estimates
- New date fields: "Start date", "Target date", "Estimate"

---

## Changes

### 1. Rename `scrum-master` → `kanban-master`

**Files:**
- `RENAME` `.claude/agents/scrum-master.md` → `.claude/agents/kanban-master.md`
- `MODIFY` `.claude/agents/kanban-master.md` — update name, description, all internal references
- `MODIFY` `.claude/agent-colors.json` — rename key from `scrum-master` to `kanban-master`

### 2. Rewrite kanban-master agent with new board constants

**File:** `.claude/agents/kanban-master.md`

Replace the entire "GitHub CLI Reference" section with new board constants. Key changes:
- All field IDs updated to new project
- `gh project item-add 3 --owner if414013` (project number 3)
- Status options: Backlog / Ready / In progress / In review / Done
- Priority options: P0 / P1 / P2
- Size options: XS / S / M / L / XL
- Drop the custom "Service" field — use issue labels instead
- Add `--project "Harbangan Board"` to `gh issue create` so issues auto-add to the board
- Update issue creation template to include size estimation

### 3. Update team-implement to sync with kanban board

**File:** `.claude/skills/team-implement/SKILL.md`

Phase 5 (GitHub Issues) changes:
- Use new `gh issue create` format with `--project "Harbangan Board"`
- After creating issue, update project item fields (Status→Ready, Priority, Size)
- When agent starts work: update Status→In progress
- When agent completes: update Status→In review
- After verification passes: update Status→Done

Phase 8 (Monitor) changes:
- Add board sync step: when TaskList status changes, mirror to GH Project Status column
- Map: `pending`→Ready, `in_progress`→In progress, `completed`→In review (until verified)

Phase 11 (Shutdown) changes:
- Incomplete tasks: update GH Project Status→Backlog with progress comment
- Completed tasks: update GH Project Status→Done

### 4. Update team-plan to create board items

**File:** `.claude/skills/team-plan/SKILL.md`

Phase 5 (Plan Output) addition:
- After writing plan file, create GH Issues for each wave task
- Add issues to project board with Status=Backlog, appropriate Priority and Size
- Include issue numbers in the plan file for traceability
- Note: plan creation should go through kanban-master agent when invoked via plan mode

### 5. Update team-review to track on board

**File:** `.claude/skills/team-review/SKILL.md`

- Create a single GH Issue for the review task (e.g., "[review]: Security + Performance review of PR #N")
- Add to board with Status=In progress when review starts
- Update to Done when report is delivered

### 6. Update team-debug to track on board

**File:** `.claude/skills/team-debug/SKILL.md`

- Create a GH Issue for the debug investigation (e.g., "[bug]: Investigate logout failure")
- Add to board with Status=In progress, Priority based on severity
- Update to Done when root cause is identified and report delivered

### 7. Update plan-mode.md rule to route through kanban-master

**File:** `.claude/rules/plan-mode.md`

Add instruction: when in plan mode, the kanban-master agent should be consulted to:
- Check existing board items for related/duplicate work
- Create board items for planned tasks
- Ensure plans reference GH Issue numbers

### 8. Update CLAUDE.md references

**File:** `CLAUDE.md`

- Update Service Map: `scrum-master` → `kanban-master` in any references
- No other structural changes needed

### 9. Update .claude/README.md

**File:** `.claude/README.md`

- Rename `scrum-master.md` reference to `kanban-master.md`
- Update description to reflect kanban board centralization

### 10. Convert `team-coordination` skill → rule

The skill is pure reference material with no invocable workflow. Moving it to a rule makes it auto-available to all agents.

**Delete:** `.claude/skills/team-coordination/` (entire directory — SKILL.md + references/)

**Create:** `.claude/rules/team-coordination.md`
- Consolidate SKILL.md content: team sizing, file ownership, communication protocols, task coordination, presets, integration patterns, agent health/respawn
- Inline the most useful bits from the 3 reference files (messaging-patterns.md, dependency-graphs.md, merge-strategies.md) — keep it concise, drop verbose examples
- Replace all `scrum-master` references with `kanban-master`

**Update:** `.claude/agents/kanban-master.md`
- Remove `skills: [team-coordination]` from frontmatter (no longer a skill)

**Update:** `.claude/CLAUDE.md`
- Remove `team-coordination/` from the skills tree listing
- Add `team-coordination.md` to the rules listing

**Update:** `.claude/README.md`
- Remove `team-coordination` from the skills table
- Note that team coordination info is now in `.claude/rules/team-coordination.md`

---

## Verification

1. `grep -r "scrum-master" .claude/` — should return zero matches (excluding `.claude/plans/`)
2. `grep -r "PVT_kwHOATKEhs4BRm0k" .claude/` — old project ID gone (excluding `.claude/plans/`)
3. `grep -r "PVT_kwHOATKEhs4BRp0j" .claude/` — new project ID present in kanban-master
4. `grep -r "team-coordination" .claude/skills/` — should return zero matches (skill deleted)
5. `ls .claude/rules/team-coordination.md` — rule file exists
6. `gh project field-list 3 --owner if414013` — verify field IDs match kanban-master.md
7. Dry-run: create a test issue and verify it appears on the board with correct fields
