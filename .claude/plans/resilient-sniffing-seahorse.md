# Provider Page UI/UX Redesign

## Context

The Providers page (`frontend/src/pages/Providers.tsx`, 904 lines) crams three unrelated concerns into one scrollable page: OAuth account connections, device code auth flows, and model registry management. The result is cognitive overload — models are scattered, providers are duplicated across sections, rate limit info is buried in tree nodes, and OAuth client IDs live on a separate Config page. This plan restructures the page into a tabbed layout with clear information hierarchy.

## Consultation Summary

- **Frontend exploration**: Identified 6 UX pain points — page combines 3 concerns, visual hierarchy confusion, scattered config, fragmented model operations, buried rate limits, no status dashboard
- **Backend exploration**: Mapped the 3-tier data model (Providers → Accounts → Models), confirmed all API endpoints available, no backend changes needed
- **Styling exploration**: Documented full CRT design system — CSS variables, component patterns, responsive breakpoints. All new UI follows existing aesthetic

## Design: Tab-Based Provider Page

Replace the single scrollable page with **3 tabs** on the same `/providers` route:

```
> providers
  Connect provider accounts and manage model access.

  [ status ]    [ connections ]    [ models ]
```

### Tab 1: Status (Default) — At-a-Glance Dashboard

```
┌──────────────┐  ┌──────────────┐  ┌──────────────┐
│ > KIRO        │  │ > COPILOT    │  │ > QWEN       │
│ ● Connected   │  │ ● Connected  │  │ ○ Offline    │
│ 12 models     │  │ 8 models     │  │ 0 models     │
└──────────────┘  └──────────────┘  └──────────────┘
┌──────────────┐  ┌──────────────┐  ┌──────────────┐
│ > ANTHROPIC   │  │ > OPENAI     │  │ > CUSTOM     │
│ ● Connected   │  │ ○ Offline    │  │ ○ No accounts│
│ 2 accounts    │  │              │  │              │
│ ⚠ 1 limited   │  │              │  │              │
└──────────────┘  └──────────────┘  └──────────────┘

// SUMMARY
42 models enabled / 67 total · 4/6 providers connected
```

Clicking a card → switches to Connections tab for that provider.

### Tab 2: Connections — Provider Setup & Account Management

Two clear sections replacing the confusing tree structure:

```
// DEVICE CODE PROVIDERS
┌ Kiro ──────────────────────────────────────────────┐
│ [existing KiroSetup component, unchanged]           │
└────────────────────────────────────────────────────┘
┌ GitHub Copilot ────────────────────────────────────┐
│ [existing CopilotSetup component, unchanged]        │
└────────────────────────────────────────────────────┘
┌ Qwen Coder ───────────────────────────────────────┐
│ [existing QwenSetup component, unchanged]           │
└────────────────────────────────────────────────────┘

// MULTI-ACCOUNT PROVIDERS
┌ Anthropic ─────────────────────────────────────────┐
│ [existing ProviderCard with accounts + rate limits]  │
└────────────────────────────────────────────────────┘
┌ OpenAI Codex ──────────────────────────────────────┐
│ [existing ProviderCard with accounts + rate limits]  │
└────────────────────────────────────────────────────┘

// OAUTH SETTINGS (admin only)
┌────────────────────────────────────────────────────┐
│ Anthropic OAuth Client ID  [________________]       │
│ OpenAI OAuth Client ID     [________________]       │
│ Qwen OAuth Client ID       [________________]       │
│                             [ $ save ]              │
└────────────────────────────────────────────────────┘
```

The tree-node hierarchy is eliminated. Each provider gets a named card section. OAuth client IDs move here from Config.tsx (where they're contextually disconnected).

### Tab 3: Models — Full Registry Management

```
[ $ populate all ]  42 models across 4 providers  [Search models...]

┌ kiro (12/15 enabled) ─────────────────────────────┐
│ [ $ populate ] [ enable all ] [ disable all ]      │
│ ┌────────┬──────────────────┬──────────┬───┐       │
│ │ enabled│ prefixed id      │ context  │del│       │
│ │  [on]  │ claude-sonnet-4..│ 200,000  │ x │       │
│ └────────┴──────────────────┴──────────┴───┘       │
└───────────────────────────────────────────────────┘
```

Existing `ProviderSection` + `DataTable` extracted verbatim — now on its own tab instead of appended below the provider tree.

## File Changes

### New Files (9)

| File | Lines | Purpose |
|------|-------|---------|
| `frontend/src/components/TabBar.tsx` | ~40 | Generic reusable tab bar component |
| `frontend/src/components/ProviderHealthCard.tsx` | ~65 | Status dashboard health card |
| `frontend/src/components/ProviderCard.tsx` | ~180 | Extracted from Providers.tsx (multi-account provider) |
| `frontend/src/components/RelayModal.tsx` | ~100 | Extracted from Providers.tsx (OAuth relay script modal) |
| `frontend/src/components/ProviderModelGroup.tsx` | ~170 | Extracted from Providers.tsx (model table per provider) |
| `frontend/src/components/OAuthSettings.tsx` | ~90 | Admin-only OAuth client ID form (moved from Config.tsx) |
| `frontend/src/pages/providers/StatusTab.tsx` | ~120 | Health grid + summary bar |
| `frontend/src/pages/providers/ConnectionsTab.tsx` | ~100 | Device code + OAuth provider sections |
| `frontend/src/pages/providers/ModelsTab.tsx` | ~80 | Model registry with grouped tables |

### Modified Files (3)

| File | Change |
|------|--------|
| `frontend/src/pages/Providers.tsx` | 904 → ~200 lines — shell with tabs, state, data loading |
| `frontend/src/pages/Config.tsx` | Remove "Provider OAuth" group (lines 156-176, ~20 lines) |
| `frontend/src/styles/components.css` | Add ~120 lines for tab bar, health cards, summary bar |

### No Backend Changes Required

All API endpoints remain the same. The frontend restructures how it presents existing data.

## Implementation Waves

### Wave 1: Foundation (parallel, no dependencies)

| Task | File | Agent |
|------|------|-------|
| Create TabBar component | `components/TabBar.tsx` | react-frontend-engineer |
| Create ProviderHealthCard component | `components/ProviderHealthCard.tsx` | react-frontend-engineer |
| Add new CSS classes (tab bar, health cards, summary) | `styles/components.css` | react-frontend-engineer |

### Wave 2: Extract Existing Components (depends on CSS from Wave 1)

| Task | File | Agent |
|------|------|-------|
| Extract ProviderCard + RelayModal | `components/ProviderCard.tsx`, `components/RelayModal.tsx` | react-frontend-engineer |
| Extract ProviderModelGroup | `components/ProviderModelGroup.tsx` | react-frontend-engineer |
| Create OAuthSettings | `components/OAuthSettings.tsx` | react-frontend-engineer |

### Wave 3: Tab Content Components (depends on Wave 2)

| Task | File | Agent |
|------|------|-------|
| Create StatusTab | `pages/providers/StatusTab.tsx` | react-frontend-engineer |
| Create ConnectionsTab | `pages/providers/ConnectionsTab.tsx` | react-frontend-engineer |
| Create ModelsTab | `pages/providers/ModelsTab.tsx` | react-frontend-engineer |

### Wave 4: Integration & Cleanup (depends on Wave 3)

| Task | File | Agent |
|------|------|-------|
| Rewrite Providers.tsx shell | `pages/Providers.tsx` | react-frontend-engineer |
| Remove Provider OAuth from Config.tsx | `pages/Config.tsx` | react-frontend-engineer |

### Wave 5: Verification

| Task | Agent |
|------|-------|
| `cd frontend && npm run build` — zero errors | frontend-qa |
| `cd frontend && npm run lint` — zero errors | frontend-qa |
| Visual testing of all 3 tabs (Playwright screenshots) | frontend-qa |
| Test tab switching, provider cards, model operations | frontend-qa |

## Data Flow

```
Providers.tsx (page shell)
  ├── state: providerStatus, models, accounts, rateLimits, activeTab
  ├── loads all data on mount (same 4 API calls as today)
  │
  ├── StatusTab (props: status, rateLimits, models counts, onNavigate)
  │   └── ProviderHealthCard × 6
  │
  ├── ConnectionsTab (props: status, accounts, rateLimits, isAdmin, onRefresh)
  │   ├── KiroSetup, CopilotSetup, QwenSetup (self-contained)
  │   ├── ProviderCard × N (multi-account providers)
  │   └── OAuthSettings (admin only, reads/writes /config)
  │
  └── ModelsTab (props: models, populating, onToggle, onDelete, onPopulate)
      └── ProviderModelGroup × N
```

Tab switching is instant — all data already loaded. No extra API calls per tab.

## Team Composition

| Agent | Role |
|-------|------|
| `react-frontend-engineer` | All component creation, extraction, styling |
| `frontend-qa` | Build verification, lint, Playwright visual tests |

Small team (2 agents) — this is a single-service frontend-only refactor with no backend or infrastructure changes.

## Verification

1. `cd frontend && npm run build` — zero TypeScript errors
2. `cd frontend && npm run lint` — zero ESLint errors
3. Navigate to `/providers` — Status tab shows with health cards
4. Click Connections tab — provider setup forms render correctly
5. Click Models tab — model registry with search/sort works
6. Verify existing flows: connect provider, populate models, enable/disable models
7. Verify admin: OAuth settings visible and saveable on Connections tab
8. Verify Config page: "Provider OAuth" group no longer appears
