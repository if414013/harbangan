# Plan: Create /team-document Skill

## Context

The project has documentation in `gh-pages/docs/` (Jekyll-based GitHub Pages), `README.md`, `CLAUDE.md`, and `.claude/README.md`. Currently documentation updates only happen as Wave 4 of `/team-implement`. The user wants a dedicated `/team-document` skill that:
- Spawns all 7 agents (consistent with always-spawn pattern)
- document-writer is the primary worker
- document-writer consults all other domain agents for technical accuracy
- Can be invoked independently to update all documentation

## Changes

### 1. Create `.claude/skills/team-document/SKILL.md`

```yaml
---
name: team-document
description: Update project documentation by consulting all domain agents for accuracy. document-writer leads, other agents provide technical review. Use when user says 'update docs', 'write documentation', 'refresh docs', 'document this', or 'sync documentation'.
argument-hint: "[scope-or-topic] [--target gh-pages|readme|claude|all]"
allowed-tools:
  - Bash
  - Read
  - Write
  - Grep
  - Glob
  - SendMessage
  - AskUserQuestion
  - TeamCreate
  - Agent
  - TaskCreate
  - TaskUpdate
  - TaskList
---
```

**Phases:**
1. Load context — read CLAUDE.md, agent definitions, existing docs
2. Spawn all 7 agents
3. Identify stale docs — compare source code against documentation for drift
4. Consult domain agents — each agent reviews docs in their domain for accuracy
5. document-writer writes/updates — based on consultation findings
6. Cross-check — domain agents verify the updated docs are accurate
7. Report — list of docs updated, verified by which agents

**Documentation targets:**
- `gh-pages/docs/**` — API reference, architecture, deployment, config, etc.
- `README.md` — project overview
- `CLAUDE.md` — project instructions
- `.claude/README.md` — workflow documentation

### 2. Update `.claude/CLAUDE.md`

- Update skill count from 9 to 10
- Add `team-document/` to structure tree
- Add row to Quick Reference: `Update docs | /team-document [scope]`

### 3. Update `.claude/README.md`

- Update skill count from 9 to 10
- Add team-document to Team Skills table (6 → 7 team skills... wait, actually team-status is separate)
- Add description

## Files

| File | Action |
|------|--------|
| `.claude/skills/team-document/SKILL.md` | Create |
| `.claude/CLAUDE.md` | Edit — add skill reference |
| `.claude/README.md` | Edit — add skill reference |

## Verification

- `ls .claude/skills/team-document/SKILL.md` — file exists
- Grep for "team-document" in CLAUDE.md and README.md — present
- Skill count is 10 everywhere
