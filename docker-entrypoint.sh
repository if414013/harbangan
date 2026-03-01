#!/bin/sh
set -e

ARGS="--host ${SERVER_HOST:-0.0.0.0} --port ${SERVER_PORT:-8000}"

if [ "${TLS_ENABLED:-true}" = "true" ]; then
  ARGS="$ARGS --tls"
else
  ARGS="$ARGS --allow-insecure"
fi

exec kiro-gateway $ARGS "$@"
