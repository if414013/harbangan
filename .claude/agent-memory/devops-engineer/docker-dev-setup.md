# Docker Dev Infrastructure

## backend/Dockerfile.dev -- Dev Dockerfile with hot-reload

- Base image: rust:1-slim-bookworm
- Installs: cargo-watch, mold linker, clang
- Pre-builds dependencies using dummy main.rs trick (layer caching)
- CMD: `cargo watch -x run -w src/`
- Watches `src/` directory for changes and auto-rebuilds

## docker-compose.dev.yml -- Dev compose with hot-reload

Usage: `docker compose -f docker-compose.dev.yml up`

Services:
- `db`: postgres:16-alpine (same as production compose)
- `backend`: built from backend/Dockerfile.dev
- `frontend`: existing frontend/Dockerfile

Backend volume mounts:
- `./backend/src:/app/src` -- live code mount for cargo-watch hot-reload
- `cargo-cache:/usr/local/cargo/registry` -- persists downloaded crates across restarts
- `target-cache:/app/target` -- persists compiled artifacts across restarts

Frontend depends_on:
- Uses `service_started` (not `service_healthy`) for backend dependency, since backend restarts frequently during dev and would fail health checks repeatedly

## backend/rebuild.sh -- Nuclear rebuild for stale binary bug

Steps:
1. Stops backend service
2. Removes container and image
3. `docker builder prune -af` -- clears BuildKit's internal cache (this is the critical step; `--no-cache` alone is insufficient)
4. Rebuilds with `--no-cache --progress=plain`

Usage: `bash backend/rebuild.sh`

## backend/Dockerfile -- Production Dockerfile (upgraded)

- Uses cargo-chef for dependency layer caching (prepare, cook, build stages)
- BuildKit cache mounts for `/usr/local/cargo/registry` and `/usr/local/cargo/git`
- mold linker configured via in-Docker `.cargo/config.toml`
- Requires `# syntax=docker/dockerfile:1` at top for BuildKit features
- Runtime stage: debian:bookworm-slim, non-root appuser (unchanged)

## backend/.dockerignore

Added exclusions: `Dockerfile*`, `.dockerignore`, `.cargo/`

## Stale Binary Bug (recurring)

Problem: Docker builds complete in seconds but binary does not contain new code.
Root cause: BuildKit's content-addressed cache on macOS serves stale layers even with `--no-cache`.
Fix: `docker builder prune -af` before rebuilding.
Script: `backend/rebuild.sh` automates the full nuclear rebuild.
