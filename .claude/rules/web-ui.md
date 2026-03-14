# Web UI Frontend Rules

Applies to files in `frontend/`.

## Stack

- React 19 + TypeScript 5.9 + Vite 7
- react-router-dom v7 (routes in `App.tsx`, base path `/_ui`)
- No state management library — use `useState`/`useEffect` directly
- No CSS-in-JS or CSS modules — plain CSS with custom properties in `variables.css`
- No external UI component library — all components are hand-rolled

## Build & Dev

```bash
cd frontend && npm run build    # tsc -b && vite build
cd frontend && npm run lint     # eslint
cd frontend && npm run dev      # vite dev server (port 5173, proxies /_ui/api → localhost:8000)
```

Built assets in `frontend/dist/` are used for production deployment.

## Component Conventions

- Named exports for all components: `export function MetricCard() {}`
- Default export only for `App.tsx`
- Props defined with `interface`, not `type`: `interface MetricCardProps { ... }`
- Pages go in `src/pages/`, reusable components in `src/components/`, utilities in `src/lib/`
- Use `className` with CSS classes from `components.css` — avoid inline styles

## Styling

- CRT phosphor terminal aesthetic — dark background, green/cyan glow, monospace font
- All design tokens live in `src/styles/variables.css` as CSS custom properties
- Use existing variables (`--bg`, `--surface`, `--green`, `--text`, `--glow-sm`, etc.) — don't hardcode colors
- Font: `var(--font-mono)` (JetBrains Mono)
- Border radius: `var(--radius)` (2px) — keep it sharp, not rounded
- Component styles go in `src/styles/components.css`
- Import order in `main.tsx`: variables.css → global.css → components.css

## API & Data

- All API calls go through `src/lib/api.ts` using the `apiFetch` wrapper
- API base path: `/_ui/api`
- Auth headers injected via `authHeaders()` from `src/lib/auth.ts`
- Real-time data (metrics, logs) uses SSE via the `useSSE` hook in `src/lib/useSSE.ts`
- SSE authenticates via session cookie with `withCredentials: true`

## TypeScript

- Strict mode enabled (`noUnusedLocals`, `noUnusedParameters`, `strict`)
- Target ES2022, module ESNext, bundler resolution
- Use `verbatimModuleSyntax` — use `import type` for type-only imports
