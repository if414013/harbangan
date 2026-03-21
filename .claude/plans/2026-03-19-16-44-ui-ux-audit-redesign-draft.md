# Plan: UI/UX Audit — Readability, Hierarchy, and Task Ergonomics

**Epic**: #134 | **Wave 1**: #135 | **Wave 2**: #136 | **Wave 3**: #137 | **Wave 4**: #138

## Consultation Summary

### Frontend Styles Investigation
- **Typography**: Entire UI uses single monospace font (`JetBrains Mono`). No secondary UI font defined. Font sizes range from 0.55rem (tree arrows) to 0.82rem (auth headings), with most body content at 0.74rem and headers at 0.62–0.72rem — all undersized for data-heavy admin workflows.
- **Contrast**: Primary text `#b8ccb8` on `#060609` background = ~7.8:1 ratio (good). But secondary `#7a8e7a` = ~4.2:1 and tertiary `#6a7e6a` = ~3.3:1 (fails WCAG AA for small text). Table data uses `--text-secondary` at 0.74rem — double penalty.
- **Visual effects**: Scanlines (z-index 9999), vignette + dot grid overlays are always-on in dark mode. Disabled in light mode. These reduce effective contrast further.
- **CSS size**: `components.css` is 2,106 lines — monolithic, no component-level CSS splitting.

### Page Structure Investigation
- **Layout shell**: `Layout.tsx` (68 lines) — minimal shell with sidebar + topbar + `<Outlet>`. Topbar shows only derived page title + uptime counter + version. No breadcrumbs, no page-level actions, no descriptions.
- **Sidebar**: Flat nav list (profile, providers, usage, config, guardrails, admin). No grouping, no section headers, no badges/counts. User email shown at 0.62rem in tertiary color — barely readable.
- **Config page**: Has search input. CSS defines `.collapsed` class with `[-]`/`[+]` indicators and `cursor: pointer`, but `Config.tsx` never applies the `collapsed` class or attaches click handlers to group headers — collapse is **not functional**. Fields lack helper text, validation hints, and grouping summaries.
- **Tables**: Raw `<table>` elements everywhere — UserTable, Usage, Providers models, API keys, Guardrails. No search, sort, pagination, empty states, or mobile overflow handling. Responsive CSS only changes shell layout at 900px, not table behavior.
- **Destructive actions**: Use `window.confirm()` browser dialogs — no custom confirmation UI. Delete/revoke/disconnect are rendered as small text buttons or inline actions with no visual weight.
- **Providers**: Tree-node UI with collapsible sections. RelayModal exists for curl-based connection flows. Provider status hidden behind tree expansion. No provider summary cards visible by default.
- **Login**: Auth card (380px fixed width) with placeholder-only inputs, no persistent labels. 2FA recovery codes shown inline. Minimal explanatory copy.

### Component Inventory
- **13 components**: AdminGuard, ApiKeyManager, CopilotSetup, DeviceCodeDisplay, DomainManager, KiroSetup, Layout, QwenSetup, SessionGate, Sidebar, ThemeToggle, Toast, UserTable
- **10 pages**: Admin, Config, Guardrails, Login, PasswordChange, Profile, Providers, TotpSetup, Usage, UserDetail
- **Missing shared primitives**: No PageHeader, ConfirmDialog, SearchInput, SortableTable, EmptyState, FormField (with label+hint), Badge, Tooltip, or Breadcrumbs components
- **Modal**: One modal pattern exists (RelayModal in Providers) but not extracted as reusable component
- **Dependencies**: Minimal — React 19, react-router-dom v7, Datadog RUM. No UI component libraries.
- **API layer**: Clean `apiFetch` wrapper in `api.ts` (591 lines, well-typed). SSE via custom `useSSE` hook.

## Scope & Priority

This plan is structured as **6 implementation tracks** across **4 waves**, matching the audit's priority tiers. Each wave is independently shippable.

---

## Wave 1: Typography & Design Token Foundation (P0)

**Goal**: Improve readability across the entire UI without removing the CRT brand identity. This is the highest-leverage change — it touches every page via CSS variables.

### Task 1.1: Add UI font and dual-font system
- **Files**: `frontend/src/styles/variables.css` (modify)
- **Owner**: react-frontend-engineer
- **Work**:
  - Add `--font-ui` variable with a legible sans-serif stack: `'Inter', 'SF Pro Text', system-ui, sans-serif`
  - Add Google Fonts import for Inter (400, 500, 600 weights)
  - Keep `--font-mono` for: code values, config keys, terminal-style labels, provider IDs, sidebar nav
  - Use `--font-ui` for: page titles, descriptions, table body text, form labels, helper text, button labels, card content

### Task 1.2: Increase font sizes and fix contrast
- **Files**: `frontend/src/styles/components.css` (modify), `frontend/src/styles/variables.css` (modify)
- **Owner**: react-frontend-engineer
- **Work**:
  - Add new text color: `--text-muted: #8fa08f` (≥4.5:1 on dark bg, replaces tertiary for body text)
  - Bump base sizes: table body 0.74→0.8125rem, table headers 0.65→0.6875rem, card titles 0.72→0.75rem, section headers 0.65→0.6875rem, config labels 0.74→0.8125rem, sidebar email 0.62→0.6875rem
  - Switch data-table td, config-label, card content, section headers to `font-family: var(--font-ui)`
  - Keep `.card-title`, `.config-group-header`, `.tree-node-toggle`, `.nav-link`, `.auth-card h2` on `--font-mono`
  - Ensure all text used for reading (not decoration) meets WCAG AA (4.5:1 for <18px, 3:1 for ≥18px bold)

### Task 1.3: Soften CRT effects for readability
- **Files**: `frontend/src/styles/global.css` (modify), `frontend/src/components/Layout.tsx` (modify)
- **Owner**: react-frontend-engineer
- **Work**:
  - Reduce scanline opacity from 0.08 to 0.04 (still visible, less intrusive)
  - Reduce vignette opacity from 0.5 to 0.3
  - Reduce dot-grid opacity from 0.03 to 0.015
  - Add `[data-dense="true"]` body attribute override that disables scanlines entirely (for data-heavy pages)
  - In `Layout.tsx`, set `document.body.dataset.dense = "true"` when the current route is `/config`, `/usage`, `/admin`, or `/providers` (via `useEffect` on `location.pathname`); clear it on other routes

---

## Wave 2: Page Scaffolding & Shared Primitives (P1)

**Goal**: Add structure, context, and reusable components that every page can use.

### Task 2.1: Create PageHeader component
- **Files**: `frontend/src/components/PageHeader.tsx` (create), `frontend/src/styles/components.css` (modify)
- **Owner**: react-frontend-engineer
- **Work**:
  - Props: `title`, `description?`, `actions?` (ReactNode slot for primary buttons), `badge?` (status indicator)
  - Renders: page title (h1, `--font-ui`, 1.125rem), optional description paragraph, optional action buttons aligned right
  - Terminal-styled: title prefixed with `>` in green, description in `--text-secondary`
  - Add to every page: Profile, Providers, Config, Usage, Admin, Guardrails

### Task 2.2: Create ConfirmDialog component
- **Files**: `frontend/src/components/ConfirmDialog.tsx` (create), `frontend/src/styles/components.css` (modify)
- **Owner**: react-frontend-engineer
- **Work**:
  - Replace all 7 `window.confirm()` calls:
    - `UserDetail.tsx:34` — remove user
    - `UserTable.tsx:38` — remove user
    - `Guardrails.tsx:84` — delete profile
    - `Guardrails.tsx:298` — delete rule
    - `Admin.tsx:90` — delete pool account
    - `Providers.tsx:197` — remove provider account
    - `Providers.tsx:584` — delete registry model
  - Props: `title`, `message`, `confirmLabel`, `variant` ("danger" | "warning" | "default"), `onConfirm`, `onCancel`
  - Danger variant: red confirm button, warning icon, "what will happen" description text
  - Focus trap, Escape to close, backdrop click to close

### Task 2.3: Create EmptyState component
- **Files**: `frontend/src/components/EmptyState.tsx` (create), `frontend/src/styles/components.css` (modify)
- **Owner**: react-frontend-engineer
- **Work**:
  - Props: `icon?` (ReactNode), `title`, `description`, `action?` (ReactNode for CTA button)
  - Centered layout with muted styling
  - Add to: UserTable (no users), API keys (no keys), Usage (no data), Guardrails (no profiles/rules), Providers models table (no models)

### Task 2.4: Create FormField wrapper component
- **Files**: `frontend/src/components/FormField.tsx` (create), `frontend/src/styles/components.css` (modify)
- **Owner**: react-frontend-engineer
- **Work**:
  - Props: `label`, `hint?`, `error?`, `required?`, `children` (the input)
  - Renders persistent label above input (not placeholder-only), optional hint text below, error message in red
  - Use in: Login page (replace placeholder-only inputs), Config page (add hints per field), PasswordChange, TotpSetup, Admin create user form

---

## Wave 3: Operational Ergonomics (P1)

**Goal**: Make data-heavy pages scannable and actionable.

### Task 3.1: Create DataTable wrapper with search/sort/overflow
- **Files**: `frontend/src/components/DataTable.tsx` (create), `frontend/src/styles/components.css` (modify)
- **Owner**: react-frontend-engineer
- **Work**:
  - Generic wrapper around `<table>` that adds:
    - Search input (filters visible rows client-side)
    - Sortable column headers (click to toggle asc/desc, visual indicator)
    - Horizontal scroll wrapper for mobile (`overflow-x: auto` with shadow hints)
    - Row count display ("showing X of Y")
    - Optional empty state (uses EmptyState component)
  - Props: `data`, `columns` (key, label, sortable?, render?), `searchKeys`, `emptyState`
  - Does NOT add pagination (data volumes are small enough for client-side filtering)

### Task 3.2: Adopt DataTable in UserTable
- **Files**: `frontend/src/components/UserTable.tsx` (modify)
- **Owner**: react-frontend-engineer
- **Work**:
  - Wrap existing table with DataTable
  - Searchable by email, name
  - Sortable by name, role, last login, created
  - Note: `handleRoleChange` is currently a single-click action without confirmation — keep it that way. The `confirm()` in `UserTable.tsx:38` (user deletion) is already covered by Task 2.2.

### Task 3.3: Adopt DataTable in Usage page
- **Files**: `frontend/src/pages/Usage.tsx` (modify)
- **Owner**: react-frontend-engineer
- **Work**:
  - Wrap usage history table with DataTable
  - **Available fields** (from `UsageRecord`): `group_key`, `request_count`, `total_input_tokens`, `total_output_tokens`, `total_cost`. For admin "users" tab (`UserUsageRecord`): `email`, same token/cost fields. No per-request timestamp or latency fields exist in the current API.
  - Searchable by `group_key` (model name or date depending on group_by), and by `email` in the users tab
  - Sortable by request count, input tokens, output tokens, cost
  - Enhance existing summary cards (total requests, total tokens, total cost) with clearer formatting — no trend indicators (would require backend time-series data not currently available)

### Task 3.4: Adopt DataTable in Providers models table
- **Files**: `frontend/src/pages/Providers.tsx` (modify)
- **Owner**: react-frontend-engineer
- **Work**:
  - Wrap models table with DataTable
  - Searchable by model ID, display name
  - Sortable by provider, name, enabled status
  - Replace `window.confirm` for model deletion with ConfirmDialog

### Task 3.5: Rework Config page UX
- **Files**: `frontend/src/pages/Config.tsx` (modify), `frontend/src/styles/components.css` (modify)
- **Owner**: react-frontend-engineer
- **Work**:
  - Add PageHeader with description: "Runtime configuration for the gateway. Changes marked 'live' take effect immediately; 'restart' changes require a service restart."
  - **Implement collapsible groups**: CSS for `.collapsed` already exists (`.config-group-header::before` shows `[+]`/`[-]`, `.config-group.collapsed .config-group-body` is `display: none`), but `Config.tsx` has no collapse state or click handlers. Add `useState` tracking collapsed group IDs, attach `onClick` to `.config-group-header`, toggle the `collapsed` class on `.config-group`.
  - Add helper text to each config field using FormField wrapper (source descriptions from the config schema endpoint `/_ui/api/config/schema`)
  - Add group-level summary line (e.g., "3 fields, 1 modified")
  - Add sticky save bar at bottom when changes exist: shows count of changed fields + Save/Reset buttons

### Task 3.6: Strengthen destructive action styling
- **Files**: `frontend/src/styles/components.css` (modify)
- **Owner**: react-frontend-engineer
- **Work**:
  - Add `.btn-danger` class: red background, white text, slightly larger than default buttons
  - Add `.btn-warning` class: yellow outline, for reversible but impactful actions (disable, disconnect)
  - Apply to: delete buttons (API keys, users, providers, guardrails), disconnect buttons (providers), role change buttons (admin)
  - All destructive actions should use ConfirmDialog instead of `window.confirm()`

---

## Wave 4: Provider Redesign & Login Polish (P2)

**Goal**: Improve the two most complex user-facing flows.

### Task 4.1: Add provider summary cards
- **Files**: `frontend/src/pages/Providers.tsx` (modify), `frontend/src/styles/components.css` (modify)
- **Owner**: react-frontend-engineer
- **Work**:
  - The Providers page has two distinct provider systems:
    1. **Generic OAuth providers** (`PROVIDERS = ["anthropic", "openai_codex"]`) — driven by `getProvidersStatus()`, support multi-account via relay connect flow
    2. **Bespoke setup components** (Kiro via `<KiroSetup>`, Copilot via `<CopilotSetup>`, Qwen via `<QwenSetup>`) — each has its own device code flow and status API
  - Add a provider summary grid at top of page (before the existing sections) showing **only the generic OAuth providers** as cards. Bespoke providers (Kiro, Copilot, Qwen) keep their existing setup components unchanged — they already have inline status indicators.
  - Each generic provider card shows: provider name, connection status (green/red dot), connected account count, "Connect" button (opens relay modal flow) / "Disconnect" button
  - Provider cards use existing `.providers-grid` and `.provider-card` CSS (already partially defined at line 1792)
  - Keep the model registry section below as-is

### Task 4.2: Improve login flow
- **Files**: `frontend/src/pages/Login.tsx` (modify), `frontend/src/styles/components.css` (modify)
- **Owner**: react-frontend-engineer
- **Work**:
  - Replace placeholder-only inputs with FormField (persistent labels + hints)
  - Add explanatory copy: "Sign in to manage your API gateway" under the logo
  - Style 2FA input more prominently — larger input, centered digits, clear "Enter your 6-digit code" label
  - Make recovery code option more visible: styled link instead of text toggle
  - Add "Forgot password? Contact your administrator." helper text for password auth

### Task 4.3: Improve TotpSetup page
- **Files**: `frontend/src/pages/TotpSetup.tsx` (modify)
- **Owner**: react-frontend-engineer
- **Work**:
  - Add step numbering: "Step 1: Scan QR code", "Step 2: Enter verification code", "Step 3: Save recovery codes"
  - Add clear instructions for each step
  - Make recovery codes more prominent with a bordered box and "Copy all" button
  - Add warning: "Save these codes — they cannot be shown again"

---

## File Manifest

| File | Action | Owner | Wave |
|------|--------|-------|------|
| `frontend/src/styles/variables.css` | modify | react-frontend-engineer | 1 |
| `frontend/src/styles/global.css` | modify | react-frontend-engineer | 1 |
| `frontend/src/components/Layout.tsx` | modify | react-frontend-engineer | 1 |
| `frontend/src/styles/components.css` | modify | react-frontend-engineer | 1-4 |
| `frontend/src/components/PageHeader.tsx` | create | react-frontend-engineer | 2 |
| `frontend/src/components/ConfirmDialog.tsx` | create | react-frontend-engineer | 2 |
| `frontend/src/components/EmptyState.tsx` | create | react-frontend-engineer | 2 |
| `frontend/src/components/FormField.tsx` | create | react-frontend-engineer | 2 |
| `frontend/src/components/DataTable.tsx` | create | react-frontend-engineer | 3 |
| `frontend/src/components/UserTable.tsx` | modify | react-frontend-engineer | 3 |
| `frontend/src/pages/Usage.tsx` | modify | react-frontend-engineer | 3 |
| `frontend/src/pages/Providers.tsx` | modify | react-frontend-engineer | 3-4 |
| `frontend/src/pages/Config.tsx` | modify | react-frontend-engineer | 3 |
| `frontend/src/pages/Profile.tsx` | modify | react-frontend-engineer | 2 |
| `frontend/src/pages/Admin.tsx` | modify | react-frontend-engineer | 2-3 |
| `frontend/src/pages/Guardrails.tsx` | modify | react-frontend-engineer | 2-3 |
| `frontend/src/pages/Login.tsx` | modify | react-frontend-engineer | 4 |
| `frontend/src/pages/TotpSetup.tsx` | modify | react-frontend-engineer | 4 |
| `frontend/src/pages/UserDetail.tsx` | modify | react-frontend-engineer | 2 |
| `frontend/src/pages/PasswordChange.tsx` | modify | react-frontend-engineer | 2 |

## Interface Contracts

No backend changes required. All changes are frontend-only.

- The config schema endpoint (`/_ui/api/config/schema`) already exists and returns field metadata that can be used for helper text in Task 3.5.
- Usage DataTable (Task 3.3) is scoped to fields available in the current API: `UsageRecord` has `group_key`, `request_count`, `total_input_tokens`, `total_output_tokens`, `total_cost`. No per-request latency or timestamp fields exist. If those are desired later, a separate backend track would be needed.
- Provider summary cards (Task 4.1) cover only generic OAuth providers (`anthropic`, `openai_codex`). Bespoke providers (Kiro, Copilot, Qwen) retain their existing setup components.

## Verification

| Gate | Command | Must Pass |
|------|---------|-----------|
| Build | `cd frontend && npm run build` | Zero errors |
| Lint | `cd frontend && npm run lint` | Zero errors |
| Visual | Manual review of each modified page in dark + light mode | No regressions |

## Dependency Graph

```
Wave 1 (typography/tokens) ─── no dependencies, can start immediately
     │
     ▼
Wave 2 (PageHeader, ConfirmDialog, EmptyState, FormField) ─── depends on Wave 1 for font variables
     │
     ▼
Wave 3 (DataTable, Config rework, destructive styling) ─── depends on Wave 2 components
     │
     ▼
Wave 4 (Providers cards, Login polish) ─── depends on Wave 2 FormField + Wave 3 ConfirmDialog
```

Waves 1 and 2 can be done as a single PR. Waves 3 and 4 are separate PRs.

## Recommended Preset

```
/team-implement --preset frontend-feature
```

Single-service (frontend only), single agent (`react-frontend-engineer`), with `frontend-qa` for verification after each wave.

## Estimated Scope

| Wave | Tasks | Complexity | New Files | Modified Files |
|------|-------|------------|-----------|----------------|
| 1 | 3 | Small | 0 | 3 |
| 2 | 4 | Medium | 4 | 8 |
| 3 | 6 | Large | 1 | 6 |
| 4 | 3 | Medium | 0 | 3 |
| **Total** | **16** | | **5 new** | **15 modified** |
