---
name: conductor-manage
description: Manage track lifecycle — complete, archive, pause, resume, or delete tracks. Use when user says 'complete track', 'archive track', 'pause work', 'resume track', 'delete track', or 'rename track'. Do NOT use for reverting git changes (use conductor-revert).
argument-hint: "<track-id> [--action complete|archive|restore|pause|resume|rename|delete|cleanup]"
allowed-tools:
  - Bash
  - Read
  - Write
  - Edit
  - Glob
  - AskUserQuestion
---

# Conductor Manage

Manage the lifecycle of development tracks. Supports completing, archiving, restoring, pausing, resuming, renaming, deleting, and cleaning up tracks.

## Critical Constraints

- **Validate status transitions** — enforce the allowed-actions table (e.g., cannot complete a paused track without resuming first; cannot archive an in-progress track)
- **Complete action requires all tasks checked** — verify all tasks are `[x]` or `[-]` in `plan.md` before allowing completion; warn if incomplete tasks remain
- **Never use destructive git operations** — never delete git commits, force-push, or reset; track deletion removes conductor artifacts only, not git history
- **Delete and rename require explicit confirmation** — always ask the user before executing these irreversible actions

---

## Step 1 — Parse Input

- **track-id** (required): e.g., `mcp-tool-caching_20260306`
- **--action** (optional): One of `complete`, `archive`, `restore`, `pause`, `resume`, `rename`, `delete`, `cleanup`. If omitted, ask the user.

---

## Step 2 — Load Track Context

Read:
- `conductor/tracks/{track-id}/metadata.json`
- `conductor/tracks/{track-id}/plan.md`
- `conductor/tracks.md`

---

## Step 3 — Validate Action

| Current Status | Allowed Actions |
|----------------|-----------------|
| `planned` | pause, delete |
| `in_progress` | complete, pause, delete |
| `paused` | resume, delete |
| `completed` | archive, delete |
| `archived` | restore, delete |
| (any) | rename, cleanup |

### Special Validation for `complete`
Verify all tasks are `[x]` or `[-]` in `plan.md`. Warn if incomplete.

---

## Step 4 — Execute Action

### Complete
Update metadata.json (`status: completed`, `completed_at`), plan.md, tracks.md.

### Archive
Move `conductor/tracks/{track-id}/` to `conductor/tracks/_archived/{track-id}/`. Update tracks.md.

### Pause
Set `status: paused`, ask for reason, update tracks.md.

### Resume
Set `status: in_progress` (or `planned`), update tracks.md.

### Delete
**Requires explicit confirmation.** Remove track directory and tracks.md entry. Does NOT revert git commits.

### Restore
Move from `_archived/` back to `tracks/`. Update metadata and tracks.md.

### Rename
Validate new ID format, check uniqueness, rename directory and update all references.

### Cleanup
Scan for orphaned directories, registry orphans, incomplete tracks, stale in-progress tracks. Offer fixes for each.

---

## Step 5 — Post-action Cleanup

Verify tracks.md consistency. Warn about active teams if track deleted/archived.

---

## Error Handling

- If track-id missing, list all tracks.
- Invalid action for status: explain valid transitions.
- Delete and rename always require explicit confirmation.
- Never delete git commits.
