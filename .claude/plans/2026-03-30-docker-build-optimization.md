# Docker Build Optimization for Backend

## Context

Two problems:

1. **Slow rebuilds:** The current `backend/Dockerfile` is a simple two-stage build that copies all source and compiles from scratch every time. Any code change triggers a full Rust recompilation of ~50 deps.

2. **Stale binary bug (recurring):** Docker builds complete in seconds instead of ~12 minutes, producing a binary that doesn't contain new code. `strings /app/harbangan | grep <new-code>` returns nothing. `--no-cache`, `docker rmi`, `DOCKER_BUILDKIT=0` all fail to fix it. Root cause: BuildKit's content-addressed cache on macOS can serve stale layers. The `COPY . .` + `RUN cargo build` pattern collapses into a single cache decision — if BuildKit thinks the context hasn't changed, the entire build is skipped.

This plan adds cargo-chef for dependency layer caching, mold linker for faster linking, BuildKit cache mounts for registry caching, a separate dev setup with hot-reload, and a nuclear rebuild script to fix the stale binary issue when it recurs.

## Files to Create/Modify

| File | Action | Purpose |
|------|--------|---------|
| `backend/Dockerfile` | **Replace** | cargo-chef + mold + BuildKit cache mounts |
| `backend/Dockerfile.dev` | **Create** | Dev image with cargo-watch |
| `docker-compose.dev.yml` | **Create** | Dev compose with volume mounts + named caches |
| `backend/.dockerignore` | **Update** | Exclude more unnecessary files |

**Not modified:** Cargo.toml, Cargo.lock, source code, existing docker-compose.yml/prod.yml/gateway.yml.

## 1. Production Dockerfile (`backend/Dockerfile`)

Replace current 16-line Dockerfile with 4-stage cargo-chef build:

```dockerfile
# syntax=docker/dockerfile:1

# ── Base: tools shared across build stages ──
FROM rust:1-slim-bookworm AS base
RUN apt-get update && apt-get install -y pkg-config mold clang && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef
WORKDIR /build
RUN mkdir -p .cargo && printf '[target.x86_64-unknown-linux-gnu]\nlinker = "clang"\nrustflags = ["-C", "link-arg=-fuse-ld=mold"]\n' > .cargo/config.toml

# ── Stage 1: Analyze deps → recipe.json ──
FROM base AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 2: Build ONLY dependencies (cached when Cargo.toml/lock unchanged) ──
FROM base AS cook
COPY --from=planner /build/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo chef cook --release --recipe-path recipe.json

# ── Stage 3: Build application (only this reruns on src changes) ──
FROM cook AS builder
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release --bin harbangan \
    && cp target/release/harbangan /usr/local/bin/harbangan

# ── Stage 4: Minimal runtime ──
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates curl jq && rm -rf /var/lib/apt/lists/* \
    && adduser --disabled-password --gecos '' appuser
WORKDIR /app
COPY --from=builder /usr/local/bin/harbangan /app/harbangan
RUN mkdir -p /data && chown appuser:appuser /data
USER appuser
EXPOSE 8000
CMD ["/app/harbangan"]
```

**Key changes from current:**
- cargo-chef splits dep compilation into its own cached layer — source-only changes skip dep rebuild entirely
- BuildKit cache mounts persist cargo registry/git downloads across builds
- mold linker reduces link time significantly (Rust's link phase is often the slowest part)
- `# syntax=docker/dockerfile:1` enables BuildKit features
- Runtime stage unchanged (debian:bookworm-slim, non-root user, same packages)

## 2. Dev Dockerfile (`backend/Dockerfile.dev`) — NEW

```dockerfile
FROM rust:1-slim-bookworm

RUN apt-get update && apt-get install -y pkg-config mold clang && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-watch

WORKDIR /app

# Configure mold linker for faster incremental linking
RUN mkdir -p .cargo && printf '[target.x86_64-unknown-linux-gnu]\nlinker = "clang"\nrustflags = ["-C", "link-arg=-fuse-ld=mold"]\n' > .cargo/config.toml

# Pre-build dependencies with dummy source (cached in image layer)
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && mkdir -p src/bin && echo 'fn main() {}' > src/bin/probe_limits.rs \
    && cargo build \
    && rm -rf src

CMD ["cargo", "watch", "-x", "run", "-w", "src/"]
```

**Notes:**
- Debug build (no `--release`) for faster incremental compilation
- Dummy main.rs trick pre-compiles all deps into the image
- Named volume for `target/` persists incremental build artifacts across container restarts
- Real `src/` is bind-mounted at runtime by docker-compose.dev.yml

## 3. Dev Compose (`docker-compose.dev.yml`) — NEW

```yaml
# Dev stack with hot-reload backend
# Usage: docker compose -f docker-compose.dev.yml up
services:
  db:
    image: postgres:16-alpine
    restart: unless-stopped
    environment:
      POSTGRES_DB: kiro_gateway
      POSTGRES_USER: kiro
      POSTGRES_PASSWORD: ${POSTGRES_PASSWORD:-kiro_secret}
    volumes:
      - pgdata:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U kiro -d kiro_gateway"]
      interval: 10s
      timeout: 5s
      retries: 5
      start_period: 10s

  backend:
    build:
      context: ./backend
      dockerfile: Dockerfile.dev
    restart: on-failure:5
    ports:
      - "9999:9999"
    env_file:
      - .env
    environment:
      SERVER_HOST: "0.0.0.0"
      SERVER_PORT: "9999"
      DATABASE_URL: postgres://kiro:${POSTGRES_PASSWORD:-kiro_secret}@db:5432/kiro_gateway
      INITIAL_ADMIN_EMAIL: ${INITIAL_ADMIN_EMAIL:-}
      INITIAL_ADMIN_PASSWORD: ${INITIAL_ADMIN_PASSWORD:-}
      INITIAL_ADMIN_TOTP_SECRET: ${INITIAL_ADMIN_TOTP_SECRET:-}
    volumes:
      - ./backend/src:/app/src
      - cargo-cache:/usr/local/cargo/registry
      - target-cache:/app/target
    depends_on:
      db:
        condition: service_healthy

  frontend:
    build:
      context: ./frontend
      dockerfile: Dockerfile
    image: harbangan-frontend:latest
    restart: unless-stopped
    ports:
      - "5173:80"
    depends_on:
      backend:
        condition: service_started
    healthcheck:
      test: ["CMD", "curl", "-fs", "http://localhost:80/nginx-health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 10s

volumes:
  pgdata:
  cargo-cache:
  target-cache:
```

**Key points:**
- `./backend/src:/app/src` — live code mount, cargo-watch detects changes
- `cargo-cache` — named volume persists downloaded crates across restarts
- `target-cache` — named volume persists compiled artifacts (incremental builds survive container restarts)
- No healthcheck on backend (cargo-watch restarts on every change, /health would be flaky)
- Frontend uses `service_started` instead of `service_healthy` since backend won't have stable health during dev

## 4. Updated `.dockerignore` (`backend/.dockerignore`)

```
target/
.git/
.env
*.db
*.sqlite3
entrypoint.sh
Dockerfile*
.dockerignore
.cargo/
```

**Added:** `Dockerfile*`, `.dockerignore`, `.cargo/` (prevent local cargo config from overriding Docker's mold config).

## Build Speed Impact

| Scenario | Current | After |
|----------|---------|-------|
| First build (cold) | ~5 min | ~5 min (same, no cache yet) |
| Source-only change (warm) | ~5 min (rebuilds all deps) | ~30-60s (deps cached, only recompile + relink) |
| Dep change (warm) | ~5 min | ~3-4 min (registry cached, recompile deps) |
| Dev hot-reload (code change) | N/A (manual rebuild) | ~5-15s (incremental + mold linking) |

## 5. Stale Binary Fix: `backend/rebuild.sh` — NEW

Script to nuke all Docker caches when the stale binary bug recurs:

```bash
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
```

**Why this works when `--no-cache` alone doesn't:**
The critical missing step is `docker builder prune -af` — this clears BuildKit's internal build cache (separate from Docker image cache). Without it, `--no-cache` can still serve cached layers from BuildKit's content-addressed store.

## Verification

1. **Production build:**
   ```bash
   docker compose build backend
   docker compose up -d
   curl -s http://localhost:9999/health  # should return 200
   ```

2. **Dev hot-reload:**
   ```bash
   docker compose -f docker-compose.dev.yml up
   # Edit backend/src/main.rs → cargo-watch should detect and recompile
   # Check logs for "Compiling harbangan..." output
   ```

3. **Cache validation (production):**
   ```bash
   # Change a line in src/main.rs, rebuild:
   docker compose build backend
   # Observe: planner + cook stages should say "CACHED", only builder reruns
   ```

4. **Stale binary fix:**
   ```bash
   # Add a unique string to any handler, then:
   bash backend/rebuild.sh
   docker compose exec backend strings /app/harbangan | grep "your-unique-string"
   # Should return the string — confirms fresh binary
   ```

5. **Existing compose files unaffected:**
   ```bash
   docker compose -f docker-compose.yml config --quiet       # no errors
   docker compose -f docker-compose.gateway.yml config --quiet  # no errors
   docker compose -f docker-compose.prod.yml config --quiet     # no errors
   ```
