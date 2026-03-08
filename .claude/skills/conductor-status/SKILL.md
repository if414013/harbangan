---
name: conductor-status
description: Display project status — active tracks, progress, and active teams. Use when user says 'show progress', 'what is the status', 'how far along', 'list active tracks', or 'project overview'. Do NOT use for checking individual agent status (use team-status).
argument-hint: "[track-id] [--tracks] [--teams] [--full]"
allowed-tools:
  - Bash
  - Read
  - Glob
  - Grep
---

# Conductor Status

Display the current status of the rkgw Gateway conductor orchestration layer. Shows active tracks, implementation progress, and active agent teams.

## Critical Constraints

- **Read-only** — this skill never modifies any files; it only reads and displays information
- **Fail gracefully if conductor/ directory doesn't exist** — if conductor is not initialized, suggest running `conductor-setup` instead of erroring out

---

## Step 1 — Parse Input

- **track-id** (optional): Show detailed status for a specific track.
- **--tracks** (optional): Show all tracks summary.
- **--teams** (optional): Show active agent teams.
- **--full** (optional): Show everything.

Default: `--tracks`.

---

## Step 2 — Load Conductor Context

Read:
- `conductor/tracks.md` — track registry
- `conductor/setup_state.json` — initialization state

If conductor is not initialized, suggest running `conductor-setup`.

---

## Step 3 — Overview Mode (default or `--tracks`)

Parse `conductor/tracks.md`, enrich with `metadata.json` for each track, display:

```
rkgw Gateway — Conductor Status
================================

Active Tracks:
  mcp-tool-caching_20260306       Add MCP tool caching       feature   in_progress  [████░░░░] 5/12 tasks   2h ago
  streaming-truncation_20260305   Fix streaming truncation    bug       planned      [░░░░░░░░] 0/6 tasks    1d ago

Summary: 2 active, 0 completed, 5/18 total tasks done
```

---

## Step 4 — Track Detail Mode (`<track-id>`)

Read full track artifacts and display phase-by-phase progress with task status markers, agent assignments, and commit SHAs.

---

## Step 5 — Teams Mode (`--teams`)

Scan `~/.claude/teams/` for active team configurations. Display members, roles, and current task status.

---

## Step 6 — Full Mode (`--full`)

Combine all outputs plus recent git activity:
```bash
cd /Users/hikennoace/ai-gateway/rkgw && git log --oneline --since="24 hours ago" --all --grep="TRACK-"
```

---

## Output Formatting

- Progress bars: `█` for complete, `░` for incomplete (8-char bar)
- Relative timestamps for recent, full date for older
- Status markers: `[x]` Complete, `[~]` In Progress, `[ ]` Pending, `[!]` Blocked, `[-]` Skipped

---

## Error Handling

- If conductor not initialized, suggest `conductor-setup`.
- If track-id not found, list available tracks.
- This skill is read-only. It never modifies any files.
