#!/bin/bash
# PreToolUse hook: scans Write/Edit content for secret patterns before allowing the operation.
# Exits 0 with no output to allow, outputs deny JSON to block.

INPUT=$(cat)
TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // empty')

# Only scan Write and Edit tools
if [ "$TOOL_NAME" != "Write" ] && [ "$TOOL_NAME" != "Edit" ]; then
  exit 0
fi

# Extract content to scan (Write uses .content, Edit uses .new_string)
if [ "$TOOL_NAME" = "Write" ]; then
  CONTENT=$(echo "$INPUT" | jq -r '.tool_input.content // empty')
else
  CONTENT=$(echo "$INPUT" | jq -r '.tool_input.new_string // empty')
fi

FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

if [ -z "$CONTENT" ]; then
  exit 0
fi

# Allowlisted files — these are expected to contain example/placeholder values
case "$FILE_PATH" in
  *.env.example|*/.env.example) exit 0 ;;
esac

# --- Secret patterns ---
SANITIZED="$CONTENT"
FOUND=""

# AWS access key IDs
if echo "$SANITIZED" | grep -qE 'AKIA[0-9A-Z]{16}'; then
  FOUND="AWS access key ID (AKIA...)"
fi

# Private keys (PEM)
if echo "$SANITIZED" | grep -qE -- '-----BEGIN[[:space:]]+(RSA|EC|DSA|OPENSSH|PGP)?[[:space:]]*PRIVATE KEY-----'; then
  FOUND="Private key (PEM format)"
fi

# Database connection strings with embedded passwords
if echo "$SANITIZED" | grep -qE 'postgres(ql)?://[^:]+:[^@]+@'; then
  # Allow placeholder patterns
  if ! echo "$SANITIZED" | grep -qE 'postgres(ql)?://[^:]+:(changeme|password|your[-_]password|<[^>]+>|\$\{)'; then
    FOUND="Database connection string with embedded password"
  fi
fi

# High-entropy values assigned to secret-like variable names
# Match: VAR_SECRET = "long-value" or _TOKEN="long-value" etc.
if echo "$SANITIZED" | grep -qE '(_SECRET|_TOKEN|_PASSWORD|_KEY)[[:space:]]*[=:][[:space:]]*"[^"]{20,}"'; then
  # Exclude placeholder/test patterns
  if ! echo "$SANITIZED" | grep -qE '(_SECRET|_TOKEN|_PASSWORD|_KEY)[[:space:]]*[=:][[:space:]]*"(changeme|your[-_]|test[-_]|fake[-_]|placeholder|<[^>]+>)'; then
    FOUND="High-entropy value assigned to secret variable"
  fi
fi

# GitHub personal access tokens
if echo "$SANITIZED" | grep -qE 'ghp_[A-Za-z0-9]{36}'; then
  FOUND="GitHub personal access token"
fi

# Generic JWT (3 base64 segments)
if echo "$SANITIZED" | grep -qE 'eyJ[A-Za-z0-9_-]{20,}\.eyJ[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{20,}'; then
  FOUND="JWT token"
fi

if [ -n "$FOUND" ]; then
  REASON="Potential secret detected: ${FOUND}. Use environment variables or placeholder values instead."
  # Escape for JSON
  REASON_ESCAPED=$(echo "$REASON" | sed 's/"/\\"/g')
  cat <<ENDJSON
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"${REASON_ESCAPED}"}}
ENDJSON
  exit 0
fi

exit 0
