# Plan: Enforce Large File Handling Rules

## Context

Agents and Claude Code itself sometimes use the Write tool to overwrite entire large files when only a few lines changed — wasting tokens and risking data loss. Similarly, reading a 2000-line file in one shot bloats context unnecessarily. We need both advisory rules (so agents understand *why*) and a deterministic hook (so violations are blocked).

## Changes

### 1. Create hook script: `.claude/hooks/enforce-edit-for-large-files.sh`

PreToolUse hook that intercepts `Write` calls on existing files >200 lines (~50KB). If the file already exists and exceeds the threshold, the hook denies the Write and tells the agent to use Edit instead. New files (that don't exist yet) are allowed through.

```bash
#!/bin/bash
# Reads tool input from stdin, checks if target file is large
# Denies Write on existing large files, suggests Edit
INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

if [ -z "$FILE_PATH" ] || [ ! -f "$FILE_PATH" ]; then
  exit 0  # New file — allow Write
fi

FILE_SIZE=$(stat -f%z "$FILE_PATH" 2>/dev/null || stat -c%s "$FILE_PATH" 2>/dev/null)
THRESHOLD=51200  # 50KB

if [ "$FILE_SIZE" -gt "$THRESHOLD" ]; then
  KB=$(( FILE_SIZE / 1024 ))
  echo '{"decision":"block","reason":"File is '"${KB}"'KB. Use Edit tool for surgical changes instead of Write."}' | \
    jq '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: .reason}}'
  exit 0
fi

exit 0
```

### 2. Register hook in `.claude/settings.json`

Add `hooks` key:

```json
"hooks": {
  "PreToolUse": [
    {
      "matcher": "Write",
      "hooks": [
        {
          "type": "command",
          "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/enforce-edit-for-large-files.sh"
        }
      ]
    }
  ]
}
```

### 3. Create rule file: `.claude/rules/file-operations.md`

Advisory rules that all agents see in context. Covers both reading and writing:

- **Writing**: Use Edit for existing files. Only use Write for new files or complete rewrites of small files (<50KB). The hook enforces this, but agents should understand the reasoning.
- **Reading**: For files >500 lines, use Read with `offset`/`limit` to read in chunks of ~200 lines. Read the section you need, not the whole file. For code files, prefer LSP tools (documentSymbol, goToDefinition) to navigate rather than reading everything.

### 4. Update root `CLAUDE.md`

Add a one-liner under the existing rules referencing the new file-operations rule, so it's visible at the top level.

## Files Modified

| File | Action |
|------|--------|
| `.claude/hooks/enforce-edit-for-large-files.sh` | Create (new) |
| `.claude/settings.json` | Edit — add `hooks` key |
| `.claude/rules/file-operations.md` | Create (new) |
| `CLAUDE.md` | Edit — add reference to file-operations rule |

## Verification

1. `chmod +x .claude/hooks/enforce-edit-for-large-files.sh`
2. Test hook manually: `echo '{"tool_input":{"file_path":"backend/src/routes/mod.rs"}}' | .claude/hooks/enforce-edit-for-large-files.sh` — should output deny JSON for large files
3. Test with a small file — should exit 0 silently
4. Verify settings.json is valid JSON after edit
