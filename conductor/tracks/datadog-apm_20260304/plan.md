# Implementation Plan: Datadog APM Integration

**Track ID:** datadog-apm_20260304
**Spec:** [spec.md](./spec.md)
**Created:** 2026-03-04
**Status:** [x] Complete

## Overview

Add Datadog APM observability across the full stack: backend distributed tracing via `datadog-opentelemetry`, frontend RUM via `@datadog/browser-rum-react`, Datadog Agent sidecar in Docker Compose, and log forwarding. The integration is opt-in via environment variables.

## Phase 1: Backend Tracing Foundation

Add the Datadog tracing layer to the Rust backend, instrumenting HTTP request spans.

### Tasks

- [x] Task 1.1: Add `datadog-opentelemetry`, `opentelemetry`, and `opentelemetry-sdk` crates to `Cargo.toml`
- [x] Task 1.2: Create `backend/src/datadog.rs` module — initialize Datadog tracer pipeline when `DD_AGENT_HOST` is set, return an optional tracing layer
- [x] Task 1.3: Integrate the Datadog layer into `main.rs` tracing subscriber registry (conditional on env var)
- [x] Task 1.4: Add `tower-http` tracing middleware to the Axum router for automatic HTTP span generation (method, path, status, latency)
- [x] Task 1.5: Add span instrumentation to key request flow functions (route handler, auth, converter, streaming) using `#[tracing::instrument]`
- [x] Task 1.6: Add environment variable documentation to `.env.example` (DD_AGENT_HOST, DD_SERVICE, DD_ENV, DD_VERSION)

### Verification

- [ ] `cargo clippy` passes with no warnings
- [ ] `cargo test --lib` passes (all existing tests still pass)
- [ ] With DD_AGENT_HOST set to a local Datadog Agent, traces appear in Datadog APM

## Phase 2: Backend Metrics Export

Ship request metrics (count, latency, error rate) to Datadog via the tracing/OTel metrics pipeline.

### Tasks

- [x] Task 2.1: Add `opentelemetry-datadog-metrics` or configure OTel metrics exporter in `datadog.rs`
- [x] Task 2.2: Instrument the existing `MetricsCollector` to emit OTel metrics (request count, latency histogram, error count by status code)
- [x] Task 2.3: Add per-model and per-user metric dimensions where appropriate
- [x] Task 2.4: Write unit tests for metrics instrumentation

### Verification

- [ ] `cargo clippy` and `cargo test --lib` pass
- [ ] Metrics visible in Datadog Metrics Explorer when DD Agent is running

## Phase 3: Log Forwarding

Configure structured JSON logging for Datadog log ingestion and trace-log correlation.

### Tasks

- [x] Task 3.1: Add JSON log formatting option to the tracing subscriber (enabled when DD_AGENT_HOST is set)
- [x] Task 3.2: Inject Datadog trace/span IDs into log output for trace-log correlation
- [x] Task 3.3: Configure DD Agent to collect container logs (Docker label-based log collection)

### Verification

- [ ] Logs appear in Datadog Log Explorer with trace correlation links
- [ ] Existing web UI log streaming (SSE) still works correctly

## Phase 4: Docker Compose Integration

Add Datadog Agent as an optional sidecar service.

### Tasks

- [x] Task 4.1: Add `datadog-agent` service to `docker-compose.yml` with proper networking, volumes, and env vars
- [x] Task 4.2: Add `datadog-agent` service to `docker-compose.gateway.yml` for Proxy-Only mode
- [x] Task 4.3: Configure DD Agent for APM trace collection (OTLP endpoint), metrics, and log forwarding
- [x] Task 4.4: Add DD_AGENT_HOST environment variable to backend service configs pointing to the agent container
- [x] Task 4.5: Update `.env.example` with Datadog-specific variables (DD_API_KEY, DD_SITE, DD_ENV)
- [x] Task 4.6: Ensure the agent is optional — compose starts cleanly without DD_API_KEY set (agent either skipped via profile or exits gracefully)

### Verification

- [ ] `docker compose build` succeeds
- [ ] `docker compose up -d` starts all services including DD Agent
- [ ] Without DD_API_KEY, system runs normally without Datadog
- [ ] With DD_API_KEY, traces and logs flow to Datadog

## Phase 5: Frontend RUM Integration

Add Datadog Real User Monitoring to the React frontend.

### Tasks

- [x] Task 5.1: Install `@datadog/browser-rum` and `@datadog/browser-rum-react` packages
- [x] Task 5.2: Create `frontend/src/lib/datadog.ts` — initialize RUM with configurable client token, application ID, and environment (via Vite env vars)
- [x] Task 5.3: Wrap the React app with Datadog RUM provider for automatic route tracking (react-router-dom v7 integration)
- [x] Task 5.4: Configure RUM to connect frontend traces to backend APM traces (propagate trace context via HTTP headers)
- [x] Task 5.5: Add Vite environment variables for RUM configuration (VITE_DD_CLIENT_TOKEN, VITE_DD_APPLICATION_ID, VITE_DD_ENV)
- [x] Task 5.6: Ensure RUM is disabled when env vars are not set (no-op initialization)

### Verification

- [ ] `npm run lint` passes
- [ ] `npm run build` succeeds
- [ ] RUM sessions visible in Datadog RUM Explorer
- [ ] Frontend-to-backend trace connection working (end-to-end traces)
- [ ] Without env vars, frontend works normally with no Datadog overhead

## Final Verification

- [ ] All acceptance criteria met
- [ ] All backend tests passing (`cargo test --lib`)
- [ ] All frontend lint/build passing
- [ ] Docker Compose starts cleanly in both modes
- [ ] End-to-end: frontend RUM → backend APM traces connected in Datadog
- [ ] Graceful degradation: system works identically without Datadog env vars
- [ ] Documentation updated (.env.example, inline code comments)

---

_Generated by Conductor. Tasks will be marked [~] in progress and [x] complete._
