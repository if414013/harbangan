# frontend-backend-revamp_20260306: Implementation Plan

**Status**: in_progress
**Branch**: refactor/frontend-backend-revamp_20260306

---

## Parallel Group A (Wave 1 — run simultaneously)

### Phase 1: Backend — Remove metrics & logs
Agent: rust-backend-engineer

- [ ] 1.1 — Delete `backend/src/metrics/` directory and `backend/src/log_capture.rs`; remove `pub mod metrics;` and `pub mod log_capture;` from `lib.rs` and `main.rs`
- [ ] 1.2 — Remove `metrics` and `log_buffer` fields from AppState in `routes/mod.rs`; remove `RequestGuard` struct; clean up imports
- [ ] 1.3 — Delete `backend/src/web_ui/sse.rs`; remove `pub mod sse;` from `web_ui/mod.rs`; remove `/metrics`, `/logs`, `/stream/metrics`, `/stream/logs` route registrations
- [ ] 1.4 — Remove `get_metrics()`, `get_logs()`, and `LogsQuery` from `web_ui/routes.rs`
- [ ] 1.5 — Fix all test helpers in `middleware/mod.rs`, `google_auth.rs`, and any other test modules that construct AppState with metrics/log_buffer fields; `cargo clippy` + `cargo test --lib` must pass

### Phase 2: Frontend — Remove metrics & logs pages
Agent: react-frontend-engineer

- [ ] 2.1 — Delete `Dashboard.tsx`, `MetricCard.tsx`, `Sparkline.tsx`, `ModelTable.tsx`, `ErrorsPanel.tsx`, `LogViewer.tsx`
- [ ] 2.2 — Update `App.tsx`: remove Dashboard import and route; set new default route (redirect to Profile or Config)
- [ ] 2.3 — Update `Sidebar.tsx`: remove Dashboard/metrics and logs navigation links
- [ ] 2.4 — Clean up `components.css`: remove all metrics/logs/sparkline/card CSS classes
- [ ] 2.5 — Verify no dead imports remain; `npm run lint` + `npm run build` must pass

---

## Sequential (Wave 2 — after Group A completes)

### Phase 3: Infrastructure — Vite dev server + nginx proxy
Agent: devops-engineer

- [ ] 3.1 — Rewrite `frontend/Dockerfile`: single stage, `node:20-alpine`, `npm ci`, run `vite dev --host 0.0.0.0`
- [ ] 3.2 — Update `frontend/nginx.conf`: replace static file serving (`/_ui/*` alias) with proxy to `frontend:5173`; add HMR websocket proxy
- [ ] 3.3 — Update `docker-compose.yml`: volume-mount `./frontend/src` and `./frontend/public` into container; expose port 5173 internally; pass Vite env vars at runtime instead of build args
- [ ] 3.4 — Verify full stack: `docker compose build && docker compose up -d`; confirm HMR, auth flow, and all remaining pages work
