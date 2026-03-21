# Fix iTerm Split Pane Spawning — Force In-Process Teammate Mode

## Context

Despite setting `"teammateMode": "in-process"` in `.claude/settings.json` (project-level, commit 0a06fb67), Claude Code still spawns iTerm2 split panes when creating agent teams. The terminal is iTerm2 (`TERM_PROGRAM=iTerm.app`, `ITERM_SESSION_ID` set), and the default `"auto"` mode detects iTerm2 and uses split panes.

**Root cause**: Settings precedence in Claude Code is `Managed → User → Project → Local` (highest to lowest). When the user-level `~/.claude/settings.json` doesn't explicitly set `teammateMode`, Claude Code may apply the default `"auto"` at the user level *before* checking the project level, effectively overriding the project-level `"in-process"` setting.

## Audit Results

| Location | Current State | Issue |
|----------|--------------|-------|
| `.claude/settings.json:23` (project) | `"teammateMode": "in-process"` | Correct, but lower precedence than user level |
| `~/.claude/settings.json` (user) | **No `teammateMode` key** | Missing — defaults to `"auto"`, which detects iTerm2 → split panes |
| `~/.claude/settings.local.json` (local) | No `teammateMode` key | OK (lowest precedence) |
| Managed settings | Not present | OK |
| Shell aliases/env vars | No `--teammate-mode` flags | OK |
| `.claude/agents/*.md` (all 8) | No teammate/terminal config | OK — agents don't control spawn mode |
| `.claude/skills/team-*` (all 4) | Use `Agent` + `TeamCreate` tools only | OK — no terminal-specific spawning |
| `.claude/README.md:148` | Says `iterm2` | **Outdated** — should say `in-process` |

## Changes

### 1. Add `teammateMode` to user-level settings (primary fix)

**File**: `~/.claude/settings.json`

Add `"teammateMode": "in-process"` at the user level so it takes effect at the highest non-managed precedence. This ensures the setting can't be bypassed regardless of how Claude Code resolves defaults.

```json
{
  "model": "opus[1m]",
  "teammateMode": "in-process",   // ← ADD THIS
  ...
}
```

### 2. Update outdated README documentation

**File**: `.claude/README.md:148`

```diff
- - **Teammate mode**: `iterm2` (agents spawn as iTerm2 tabs with distinct colors)
+ - **Teammate mode**: `in-process` (agents run within the main terminal, cycle with Shift+Down)
```

### 3. Keep project-level setting (no change needed)

`.claude/settings.json:23` already has `"teammateMode": "in-process"` — keep this as a fallback for other users who clone the repo.

## Verification

1. After applying changes, **restart Claude Code** (settings are loaded at startup)
2. Run `/team-review` or spawn an agent with `Agent` tool + `team_name` parameter
3. Confirm: agents appear in-process (accessible via Shift+Down), NOT as iTerm split panes
4. Verify `~/.claude/settings.json` contains `"teammateMode": "in-process"`
