# Plan: Add Provider Enable/Disable Toggle UI

## Context

The backend `PATCH /_ui/api/admin/providers/:provider_id` endpoint is fully implemented, but the frontend never built the UI for it. The backend registry endpoint already returns `enabled: bool` for each provider, but the frontend TypeScript type silently ignores the field. E2E tests already exist expecting this UI. This is a frontend-only fix — 4 files, ~40 lines of new code.

## Consultation Summary

- **rust-backend-engineer**: Backend fully complete. No changes needed. PATCH endpoint at `provider_oauth.rs:918-953`, registry returns `enabled` at line 472. CSRF required. Kiro always-on (400 if disabled). 13+ unit tests cover enable/disable logic across registry, pipeline, error, and cache modules.
- **react-frontend-engineer**: `ProviderRegistryEntry` type missing `enabled` field. 4 files need changes. Reuse `role-badge` toggle pattern from Admin.tsx. Raised nested-button HTML validity concern — recommend `e.stopPropagation()` (consistent with existing patterns). Kiro toggle should be hidden.
- **database-engineer**: Schema complete (`provider_settings` table, migration v25). All 4 query methods exist. No migrations needed.
- **devops-engineer**: No impact. No Docker, env var, or infrastructure changes.
- **backend-qa**: 27+ backend tests across registry, pipeline, error, cache, and integration modules. Handler-level tests for the PATCH endpoint are a gap but not blocking.
- **frontend-qa**: E2E tests ALREADY EXIST — `e2e-tests/specs/api/provider-toggle.spec.ts` (5 tests) and `e2e-tests/specs/ui/provider-toggle.spec.ts` (7 tests). Tests expect `.role-badge` toggles on health cards, `data-connected=false` for disabled providers, Kiro has no toggle. No new tests needed.
- **document-writer**: CLAUDE.md already documents the endpoint. Small pre-existing gap in gh-pages docs (not blocking).

## File Manifest

| File | Action | Owner | Wave |
|------|--------|-------|------|
| `frontend/src/lib/api.ts` | modify | react-frontend-engineer | 1 |
| `frontend/src/components/ProviderHealthCard.tsx` | modify | react-frontend-engineer | 1 |
| `frontend/src/pages/providers/StatusTab.tsx` | modify | react-frontend-engineer | 1 |
| `frontend/src/pages/Providers.tsx` | modify | react-frontend-engineer | 1 |

## Wave 1: Frontend Implementation (all changes)

### Task 1.1: Add `enabled` to type + API function (assigned: react-frontend-engineer)
- **File**: `frontend/src/lib/api.ts`
- **Changes**:
  - Add `enabled: boolean` to `ProviderRegistryEntry` interface (line ~312)
  - Add `toggleProviderEnabled()` function after line 406, following `toggleAdminPoolAccount` pattern:
    ```typescript
    export function toggleProviderEnabled(providerId: string, enabled: boolean) {
      return apiFetch<{ provider_id: string; enabled: boolean }>(
        `/admin/providers/${providerId}`,
        {
          method: "PATCH",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ enabled }),
        },
      );
    }
    ```

### Task 1.2: Add toggle button to ProviderHealthCard (assigned: react-frontend-engineer)
- **File**: `frontend/src/components/ProviderHealthCard.tsx`
- **Changes**:
  - Add props: `enabled: boolean`, `isAdmin: boolean`, `onToggle: (enabled: boolean) => void`
  - Add `role-badge` toggle button in `health-card-header` between name span and dot span
  - Guard: `{isAdmin && providerId !== "kiro" && (...)}`
  - Use `e.stopPropagation()` to prevent outer card click
  - Pattern: green "on" / red "off" badge using `var(--green-dim)` / `var(--red-dim)`

### Task 1.3: Thread props through StatusTab (assigned: react-frontend-engineer)
- **File**: `frontend/src/pages/providers/StatusTab.tsx`
- **Changes**:
  - Add `isAdmin: boolean` and `onProviderToggle: (providerId: string, enabled: boolean) => void` to `StatusTabProps`
  - Pass `enabled={registry.find(r => r.id === p)?.enabled ?? true}`, `isAdmin`, and `onToggle` to each `ProviderHealthCard`

### Task 1.4: Add toggle handler + pass props (assigned: react-frontend-engineer)
- **File**: `frontend/src/pages/Providers.tsx`
- **Changes**:
  - Import `toggleProviderEnabled` from api
  - Add `handleProviderToggle` handler (optimistic update on `registry` state, toast on error)
  - Pass `isAdmin` and `onProviderToggle={handleProviderToggle}` to `StatusTab`

## Interface Contracts

**Existing backend → frontend** (no changes needed):
```
GET /_ui/api/providers/registry
→ { providers: [{ id, display_name, category, supports_pool, enabled }] }

PATCH /_ui/api/admin/providers/:provider_id  (admin + CSRF)
← { enabled: boolean }
→ { provider_id: string, enabled: boolean }
```

## Verification

1. `cd frontend && npm run build` — zero errors
2. `cd frontend && npm run lint` — zero errors
3. Manual: admin sees "on"/"off" toggle on Anthropic, OpenAI Codex, Copilot cards; Kiro has no toggle
4. Manual: toggling calls API and updates card immediately (optimistic)
5. Manual: non-admin user sees no toggles
6. E2E: `cd e2e-tests && npm run test:ui` — existing `provider-toggle.spec.ts` should pass

## Branch

`fix/provider-toggle-ui`

## Review Status
- Codex review: pending
- Findings addressed: 0
- Disputed findings: 0
