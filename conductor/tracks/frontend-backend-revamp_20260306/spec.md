# frontend-backend-revamp_20260306: Frontend Backend Communication Revamp

**Type**: refactor
**Created**: 2026-03-06
**Preset**: fullstack
**Services**: frontend, backend, infra

## Problem Statement
The current architecture pre-builds React into static files and serves them via nginx. This eliminates HMR and requires a full Docker rebuild for any UI change. Additionally, the metrics and logs pages duplicate observability that Datadog now handles. The backend should be a pure API server — no static file serving.

## Acceptance Criteria
1. Metrics page and all related components removed from frontend
2. Logs page and LogViewer component removed from frontend
3. Backend metrics endpoints, MetricsCollector, log_buffer, and log streaming SSE removed
4. Frontend container runs Vite dev server with HMR enabled
5. nginx proxies `/_ui/*` to the Vite server instead of serving static files
6. Frontend source is volume-mounted for live editing without rebuilds
7. All auth (Google SSO, session cookies, CSRF) and RBAC (admin/user roles) continue working
8. Backend serves zero static files — pure API only

## Scope Boundaries
- OUT: Datadog integration (already handled separately)
- OUT: Changes to `/v1/*` proxy API endpoints
- OUT: Changes to auth flow logic (only wiring changes)
- OUT: Adding new UI pages — this is a removal + architecture refactor
- IN: Removing metrics + logs from both frontend and backend
- IN: Dockerfile, nginx.conf, docker-compose.yml restructuring
- IN: Frontend dev server configuration

## Dependencies
- Datadog APM integration (completed, archived) handles observability going forward
