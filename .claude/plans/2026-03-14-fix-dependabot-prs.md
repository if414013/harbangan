# Fix Dependabot Security PRs

11 open Dependabot PRs grouped into 4 batches by dependency coupling.

## Batch 1: Safe Independent Merges
Close PRs and apply on a single branch `chore/deps-independent`:
- **#77** tempfile 3.24→3.27 — `backend/Cargo.toml` already uses `"3"`, just `cargo update -p tempfile`
- **#81** @datadog/browser-rum-react 6.28→6.30 — within `^6.28.1`, just `npm update` in frontend
- **#74** actions/setup-node 4.3→6.3 — update `.github/workflows/ci.yml` pin

## Batch 2: Frontend Bundle (coupled)
Close PRs #76, #80, #47, #78. Create branch `chore/deps-frontend-major`:
- **#76** vite 7.3.1→8.0.0 — update `frontend/package.json`
- **#80** @vitejs/plugin-react 5.1.4→6.0.1 — requires vite 8 (peer dep)
- **#47** eslint 9→10 + **#78** @eslint/js 9→10 — must bump together
- **Also**: typescript-eslint 8.48→8.57+ (required for eslint 10 compat, not a Dependabot PR)
- Verify: `cd frontend && npm install && npm run lint && npm run build`

## Batch 3: Backend OpenTelemetry Bundle (coupled)
Close PRs #46, #49. Create branch `chore/deps-otel-upgrade`:
- All otel crates must be version-aligned. Update `backend/Cargo.toml`:
  - opentelemetry 0.27→0.31, opentelemetry_sdk 0.27→0.31
  - opentelemetry-otlp 0.27→0.31, opentelemetry-datadog 0.15→0.19
  - tracing-opentelemetry 0.28→compatible version
- Fix any API changes in `backend/src/datadog.rs`
- Verify: `cd backend && cargo clippy --all-targets && cargo test --lib`

## Batch 4: Backend Individual
Create branch `chore/deps-backend-misc`:
- **#79** sysinfo 0.32→0.38 — fix API breaks in `backend/src/web_ui/routes.rs` and `backend/src/bench/metrics.rs` (`ProcessesToUpdate` enum may have changed)
- **#75** config 0.14→0.15 — **REMOVE** this dep entirely (dead/unused, shadowed by local `mod config`)
- Verify: `cd backend && cargo clippy --all-targets && cargo test --lib`

## Execution Order
1. Batch 1 (safe, fast) → PR → merge
2. Batch 4 (backend misc, isolated) → PR → merge
3. Batch 2 (frontend major bumps) → PR → merge
4. Batch 3 (otel, highest risk) → PR → merge

After each batch merges, close the corresponding Dependabot PRs.
