#!/bin/sh
set -e

DB_FILE="${KIRO_CLI_DB_FILE:-/home/kiro/.local/share/kiro-cli/data.sqlite3}"

# Auto-login if not already authenticated
if ! kiro-cli whoami > /dev/null 2>&1; then
  echo ""
  echo "╔═══════════════════════════════════════════════════════════╗"
  echo "║  No Kiro credentials found. Starting login flow...       ║"
  echo "║  A URL will appear below — open it in your browser.      ║"
  echo "╚═══════════════════════════════════════════════════════════╝"
  echo ""

  LOGIN_ARGS="--use-device-flow"
  if [ -n "$KIRO_SSO_URL" ]; then
    LOGIN_ARGS="$LOGIN_ARGS --license pro --identity-provider $KIRO_SSO_URL"
    if [ -n "$KIRO_SSO_REGION" ]; then
      LOGIN_ARGS="$LOGIN_ARGS --region $KIRO_SSO_REGION"
    fi
  else
    LOGIN_ARGS="$LOGIN_ARGS --license free"
  fi

  kiro-cli login $LOGIN_ARGS
  echo ""
  echo "✅ Login successful! Starting gateway..."
  echo ""
fi

ARGS="--host ${SERVER_HOST:-0.0.0.0} --port ${SERVER_PORT:-8000}"

if [ "${TLS_ENABLED:-true}" = "true" ]; then
  ARGS="$ARGS --tls"
else
  ARGS="$ARGS --allow-insecure"
fi

export KIRO_CLI_DB_FILE="$DB_FILE"
exec kiro-gateway $ARGS "$@"
