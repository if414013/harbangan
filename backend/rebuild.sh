#!/usr/bin/env bash
set -euo pipefail
# Nuclear rebuild — clears ALL Docker build caches and forces a real recompile.
# Use when: `strings /app/harbangan | grep <your-change>` returns nothing after a normal rebuild.

echo "==> Stopping backend..."
docker compose stop backend 2>/dev/null || true

echo "==> Removing backend container..."
docker compose rm -f backend 2>/dev/null || true

echo "==> Removing backend image..."
docker rmi harbangan-backend:latest 2>/dev/null || true

echo "==> Pruning BuildKit cache..."
docker builder prune -af

echo "==> Rebuilding backend (no cache, no BuildKit inline cache)..."
DOCKER_BUILDKIT=1 docker compose build --no-cache --progress=plain backend

echo "==> Starting backend..."
docker compose up -d backend

echo "==> Waiting for healthy..."
sleep 5
docker compose ps backend
echo ""
echo "Verify: docker compose exec backend strings /app/harbangan | grep <your-change>"
