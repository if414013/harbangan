#!/bin/bash
# Stop hook: blocks downstream Stop hooks when running inside a worktree.
# Worktree CWDs contain /.claude/worktrees/ which breaks plugins that
# derive transcript paths from CWD.

INPUT=$(cat)
CWD=$(echo "$INPUT" | jq -r '.cwd // empty')

if echo "$CWD" | grep -q '/\.claude/worktrees/'; then
  exit 2
fi

exit 0
