#!/bin/bash
# PreToolUse hook: blocks git add/commit of sensitive files.
# Watches Bash tool calls for git commands that stage or commit dangerous paths.

INPUT=$(cat)
TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // empty')

# Only check Bash tool
if [ "$TOOL_NAME" != "Bash" ]; then
  exit 0
fi

COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // empty')

if [ -z "$COMMAND" ]; then
  exit 0
fi

# Only check git add and git commit commands
if ! echo "$COMMAND" | grep -qE '^\s*git\s+(add|commit)'; then
  exit 0
fi

# Sensitive file patterns to block
BLOCKED_PATTERNS=(
  '\.env$'
  '\.env\.'
  '\.env\.local'
  '\.env\.proxy'
  'certs/'
  '\.pem$'
  '\.key$'
  '\.p12$'
  '\.pfx$'
  'e2e-tests/\.auth'
)

# Allowlisted patterns
ALLOW_PATTERNS=(
  '\.env\.example'
)

for pattern in "${BLOCKED_PATTERNS[@]}"; do
  if echo "$COMMAND" | grep -qE "$pattern"; then
    # Check if it matches an allowlisted pattern
    ALLOWED=false
    for allow in "${ALLOW_PATTERNS[@]}"; do
      if echo "$COMMAND" | grep -qE "$allow"; then
        ALLOWED=true
        break
      fi
    done

    if [ "$ALLOWED" = false ]; then
      cat <<ENDJSON
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"Blocked: staging sensitive file matching '${pattern}'. These files may contain secrets and should not be committed. Only .env.example is safe to commit."}}
ENDJSON
      exit 0
    fi
  fi
done

# Block 'git add -A' and 'git add .' as they can accidentally include sensitive files
if echo "$COMMAND" | grep -qE 'git\s+add\s+(-A|--all|\.($|\s|&&|;|\|))'; then
  cat <<ENDJSON
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"Blocked: 'git add -A' / 'git add .' can accidentally stage sensitive files (.env, certs, auth state). Stage specific files by name instead."}}
ENDJSON
  exit 0
fi

exit 0
