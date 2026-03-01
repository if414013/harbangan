#!/bin/sh
set -e

DB_FILE="${KIRO_CLI_DB_FILE:-/home/kiro/.local/share/kiro-cli/data.sqlite3}"

# Show configuration for debugging
echo ""
echo "┌─────────────────────────────────────────────────────────┐"
echo "│  Kiro Gateway - Configuration                           │"
echo "├─────────────────────────────────────────────────────────┤"
echo "│  SERVER_HOST:  ${SERVER_HOST:-0.0.0.0}"
echo "│  SERVER_PORT:  ${SERVER_PORT:-8000}"
echo "│  TLS_ENABLED:  ${TLS_ENABLED:-false}"
echo "│  KIRO_REGION:  ${KIRO_REGION:-us-east-1}"
echo "│  LOG_LEVEL:    ${LOG_LEVEL:-info}"
echo "│  DB_FILE:      $DB_FILE"
if [ -n "$KIRO_SSO_URL" ]; then
echo "│  KIRO_SSO_URL: $KIRO_SSO_URL"
echo "│  SSO_REGION:   ${KIRO_SSO_REGION:-(not set)}"
echo "│  Login mode:   Identity Center (pro)"
else
echo "│  Login mode:   Builder ID (free)"
fi
echo "└─────────────────────────────────────────────────────────┘"
echo ""

# Auto-login if not already authenticated
if ! kiro-cli whoami > /dev/null 2>&1; then
  echo "╔═══════════════════════════════════════════════════════════╗"
  echo "║  No Kiro credentials found. Starting login flow...       ║"
  echo "║  A URL will appear below — open it in your browser.      ║"
  echo "╚═══════════════════════════════════════════════════════════╝"
  echo ""

  LOGIN_ARGS="--use-device-flow"
  if [ -n "$KIRO_SSO_URL" ]; then
    # Validate SSO URL format
    case "$KIRO_SSO_URL" in
      https://*.awsapps.com/start|https://*.awsapps.com/start/)
        ;;
      https://*)
        echo "⚠️  Warning: KIRO_SSO_URL doesn't look like a standard AWS SSO URL."
        echo "   Expected format: https://your-org.awsapps.com/start"
        echo "   Got: $KIRO_SSO_URL"
        echo ""
        ;;
      *)
        echo "❌ Error: KIRO_SSO_URL must start with https://"
        echo "   Expected format: https://your-org.awsapps.com/start"
        echo "   Got: $KIRO_SSO_URL"
        echo ""
        echo "   To use Builder ID (free) instead, remove KIRO_SSO_URL from docker-compose.yml"
        exit 1
        ;;
    esac

    LOGIN_ARGS="$LOGIN_ARGS --license pro --identity-provider $KIRO_SSO_URL"
    if [ -n "$KIRO_SSO_REGION" ]; then
      LOGIN_ARGS="$LOGIN_ARGS --region $KIRO_SSO_REGION"
    fi
    echo "→ Using Identity Center (pro) login: $KIRO_SSO_URL"
  else
    LOGIN_ARGS="$LOGIN_ARGS --license free"
    echo "→ Using Builder ID (free) login"
  fi
  echo "→ Running: kiro-cli login $LOGIN_ARGS"
  echo ""

  if [ -n "$KIRO_SSO_URL" ]; then
    # Identity Center (pro) login requires a PTY workaround.
    # kiro-cli's input() returns empty string when stdout is not a terminal,
    # ignoring --identity-provider and --region defaults. Using 'script' to
    # allocate a pseudo-TTY and piping two newlines to accept the defaults.
    echo "→ Note: 'script' PTY workaround active for non-TTY Identity Center login"
    if ! printf '\n\n' | script -qec "kiro-cli login $LOGIN_ARGS" /dev/null; then
      echo ""
      echo "❌ Login failed!"
      echo ""
      echo "   Possible causes:"
      echo "   1. KIRO_SSO_URL is incorrect: $KIRO_SSO_URL"
      echo "   2. KIRO_SSO_REGION is wrong or missing: ${KIRO_SSO_REGION:-(not set)}"
      echo "   3. Network connectivity issue (can the container reach AWS?)"
      echo ""
      echo "   To use Builder ID (free) instead, remove KIRO_SSO_URL:"
      echo "   PROXY_API_KEY=xxx docker compose up"
      exit 1
    fi
  else
    if ! kiro-cli login $LOGIN_ARGS; then
      echo ""
      echo "❌ Login failed!"
      echo ""
      echo "   Possible causes:"
      echo "   1. Network connectivity issue (can the container reach AWS?)"
      echo "   2. The device authorization timed out"
      exit 1
    fi
  fi

  echo ""
  echo "✅ Login successful! Starting gateway..."
  echo ""
else
  echo "✅ Already authenticated. Starting gateway..."
  echo ""
fi

ARGS="--host ${SERVER_HOST:-0.0.0.0} --port ${SERVER_PORT:-8000}"

if [ "${TLS_ENABLED:-false}" = "true" ]; then
  ARGS="$ARGS --tls"
else
  ARGS="$ARGS --allow-insecure"
fi

export KIRO_CLI_DB_FILE="$DB_FILE"
echo "→ Starting: kiro-gateway $ARGS"
echo ""
exec kiro-gateway $ARGS "$@"
