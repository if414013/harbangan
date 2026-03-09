#!/bin/bash
# PreToolUse hook: blocks Write on existing files >50KB, suggests Edit instead.
# New files (path doesn't exist yet) are allowed through.
INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

if [ -z "$FILE_PATH" ] || [ ! -f "$FILE_PATH" ]; then
  exit 0
fi

FILE_SIZE=$(stat -f%z "$FILE_PATH" 2>/dev/null || stat -c%s "$FILE_PATH" 2>/dev/null)
THRESHOLD=51200

if [ "$FILE_SIZE" -gt "$THRESHOLD" ]; then
  KB=$(( FILE_SIZE / 1024 ))
  cat <<ENDJSON
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"File is ${KB}KB (>${THRESHOLD}B). Use the Edit tool for surgical changes instead of overwriting with Write."}}
ENDJSON
  exit 0
fi

exit 0
