# Plan: Simplify team-* Skill Parameters

## Context

`team-implement` has an intimidating argument-hint that bundles 3 unrelated management sub-commands into one skill:
```
[feature-or-plan] [--preset name] [--worktree] [--no-worktree] [--shutdown team-name] [--status team-name] [--delegate team-name]
```

The management commands (`--status`, `--delegate`, `--shutdown`) are conceptually separate operations that got absorbed into team-implement. This makes the skill confusing — users see 7 params when they just want to build a feature.

## Approach: Simplify in-place + extract team-status

### 1. Slim down `team-implement` to 1 param

**Before:**
```
/team-implement [feature-or-plan] [--preset name] [--worktree] [--no-worktree] [--shutdown team-name] [--status team-name] [--delegate team-name]
```

**After:**
```
/team-implement [feature-or-plan]
```

Changes:
- Remove `--preset` — auto-detect from affected services (Phase 2 already has this logic). If ambiguous, ask via AskUserQuestion
- Remove `--worktree` / `--no-worktree` — worktree is now always the default. No flag needed
- Remove `--status` — extract to standalone `/team-status` skill
- Keep `--delegate` and `--shutdown` inline as sub-commands (they stay in team-implement but aren't shown in the argument-hint since they're secondary operations)

### 2. Extract `team-status` as standalone skill

| New Skill | Syntax | Purpose |
|-----------|--------|---------|
| `team-status` | `/team-status [team-name]` | Show team health (auto-detect team if only one active) |

### 3. Other team skills — no changes

team-plan, team-review, team-debug are already clean.

## File Manifest

| File | Action | Description |
|------|--------|-------------|
| `.claude/skills/team-implement/SKILL.md` | modify | Remove --preset/--worktree from argument-hint, make worktree default, move status sub-command docs to bottom as hidden sub-commands |
| `.claude/skills/team-status/SKILL.md` | create | Extract status logic from team-implement |

## Summary of changes

**Before:** team-implement has 7 params in argument-hint
**After:** team-implement has 1 param (`[feature-or-plan]`), worktree is default, preset is auto-detected, status is a separate skill. Delegate/shutdown remain as inline sub-commands but hidden from the argument-hint

## Verification

1. Read each new SKILL.md and confirm the argument-hint is minimal
2. Confirm team-implement no longer references sub-commands
3. Confirm the extracted skills contain the full logic from the original sub-command sections
4. Run `/team-implement --help` style check (read the description field) to verify it's clean
