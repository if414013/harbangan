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
