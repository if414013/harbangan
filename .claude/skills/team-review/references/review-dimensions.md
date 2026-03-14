# Review Dimensions -- Harbangan

Detailed checklists for each quality dimension. Reviewers follow the checklist for their assigned dimension(s) and report findings using the severity scale defined in the parent SKILL.md.

---

## 1. Security

### API Key and Token Handling

- [ ] API keys are SHA-256 hashed before storage and cache lookup (`middleware/`)
- [ ] Per-user Kiro tokens use short TTL (4-min) in `kiro_token_cache`
- [ ] Token refresh happens before expiry, not after failure
- [ ] No plaintext secrets in logs (tracing fields must not log tokens, keys, or passwords)
- [ ] `api_key_cache` entries are invalidated on key deletion/rotation

### Session and Authentication

- [ ] Session cookie `kgw_session` uses `Secure`, `HttpOnly`, `SameSite=Strict`
- [ ] CSRF token validated on all mutation endpoints (POST/PUT/DELETE under `/_ui/api/`)
- [ ] OAuth PKCE state entries expire (10-min TTL) and have a size cap (10k)
- [ ] Google SSO callback validates `state` parameter against `oauth_pending`
- [ ] First-user setup mode blocks `/v1/*` with 503 until admin exists

### Input Validation and Injection

- [ ] All SQL uses parameterized queries via `sqlx` (no string interpolation in queries)
- [ ] User-supplied model names validated through `ModelResolver` before use
- [ ] Request body size limits enforced
- [ ] Path parameters sanitized (no path traversal via model IDs or API key names)
- [ ] CEL rule expressions in guardrails are validated before storage

### CORS and Headers

- [ ] CORS middleware allows only expected origins
- [ ] `Authorization` and `x-api-key` headers handled consistently
- [ ] No sensitive headers forwarded to upstream Kiro API unnecessarily

### Docker and Infrastructure

- [ ] No secrets passed as Docker build args (use runtime env vars)
- [ ] `.env` files listed in `.dockerignore` and `.gitignore`
- [ ] Backend listens on plain HTTP only (TLS handled by k8s Ingress in production)

---

## 2. Performance

### Async and Concurrency

- [ ] No blocking operations inside `tokio::spawn` or async handlers (no `std::sync::Mutex` held across `.await`)
- [ ] `tokio::sync::RwLock` used for shared state that is read-heavy (`config`, `auth_manager`)
- [ ] `DashMap` used correctly for concurrent caches (no holding refs across await points)
- [ ] Streaming responses use chunked transfer, not buffering the entire response

### Connection Pooling and HTTP

- [ ] `KiroHttpClient` reuses connections (connection pool, not per-request clients)
- [ ] `sqlx::PgPool` configured with appropriate `max_connections`
- [ ] HTTP client timeouts set for upstream Kiro API calls
- [ ] MCP client connections use keep-alive where supported

### Caching and TTLs

- [ ] `ModelCache` TTL is reasonable and cache is invalidated on config change
- [ ] `kiro_token_cache` TTL (4-min) is shorter than actual token expiry
- [ ] `session_cache` does not grow unbounded (24h TTL enforced)
- [ ] `api_key_cache` has eviction strategy for deleted keys

### Streaming Efficiency

- [ ] AWS Event Stream binary parsing in `streaming/mod.rs` does not allocate unnecessarily
- [ ] Thinking block extraction in `thinking_parser.rs` uses zero-copy where possible
- [ ] SSE responses flush each event promptly (no buffering multiple events)
- [ ] Converter pipelines (e.g., `openai_to_kiro`) do not clone large message bodies

### Frontend Rendering

- [ ] React components avoid unnecessary re-renders (stable references, memoization where needed)
- [ ] SSE hooks (`useSSE`) clean up event sources on unmount
- [ ] Large lists use pagination or virtualization, not full DOM rendering
- [ ] No expensive computations in render paths without memoization

---

## 3. Architecture

### Module Boundaries

- [ ] Each module in `backend/src/` has a clear, single responsibility
- [ ] No circular dependencies between modules (e.g., `converters` should not import `routes`)
- [ ] Shared types live in `models/`, not duplicated across modules
- [ ] `AppState` fields accessed through well-defined interfaces, not raw field access everywhere

### Error Handling Patterns

- [ ] Error enums defined with `thiserror` in each module's `error.rs`
- [ ] `anyhow::Result` with `.context()` used for propagation, not `.unwrap()` or bare `?`
- [ ] `ApiError` implements `IntoResponse` with appropriate HTTP status codes
- [ ] Error messages are informative for debugging but do not leak internals to clients

### Converter Symmetry

- [ ] Each format direction has its own converter file (e.g., `openai_to_kiro.rs`, `kiro_to_openai.rs`)
- [ ] Shared conversion logic lives in `core.rs`, not duplicated
- [ ] Round-trip fidelity: converting A->B->A preserves semantics for supported fields
- [ ] New model fields are handled in all relevant converters, not just one direction

### Middleware Ordering

- [ ] CORS middleware runs before auth middleware
- [ ] API key auth middleware runs before route handlers
- [ ] Debug logging middleware captures request/response without modifying them
- [ ] Middleware does not silently swallow errors

### Configuration and State

- [ ] Runtime config changes via web UI propagate correctly through `Arc<RwLock<Config>>`
- [ ] Model aliases resolved through `ModelResolver`, never hardcoded model IDs
- [ ] Feature flags (`guardrails_enabled`) checked at the right layer
- [ ] Proxy-only mode (`config_db: None`) gracefully degrades all DB-dependent features

### Frontend Structure

- [ ] Pages in `src/pages/`, reusable components in `src/components/`, utilities in `src/lib/`
- [ ] All API calls go through `apiFetch` wrapper in `src/lib/api.ts`
- [ ] Named exports for components (default export only for `App.tsx`)
- [ ] Props defined with `interface`, not `type`

---

## 4. Testing

### Coverage

- [ ] Critical paths have unit tests: auth flows, converter transformations, streaming parsing
- [ ] Edge cases tested: empty input, malformed payloads, expired tokens, missing fields
- [ ] Error paths tested: network failures, invalid API keys, DB connection loss
- [ ] Guardrail rule matching and MCP tool execution have dedicated test cases

### Test Quality

- [ ] Tests are deterministic (no reliance on timing, external services, or random data)
- [ ] Tests are isolated (no shared mutable state between tests)
- [ ] Assertions are specific: check exact values, not just `is_ok()` or `is_some()`
- [ ] Test names follow `test_<function>_<scenario>` convention

### Test Patterns (Backend)

- [ ] Async tests use `#[tokio::test]`
- [ ] Helper configs use `create_test_config()` or `Config::with_defaults()`
- [ ] Feature-gated test utilities use `#[cfg(any(test, feature = "test-utils"))]`
- [ ] Tests live in `#[cfg(test)] mod tests` at the bottom of each file
- [ ] Integration tests gated behind `--features test-utils`

### Test Patterns (Frontend)

- [ ] Component tests verify rendered output and user interactions
- [ ] API mocking is consistent (same mock patterns across test files)
- [ ] SSE hook tests verify connection lifecycle (connect, receive, cleanup)

### Mocking

- [ ] Mocks are minimal: only mock what is necessary, not the entire dependency tree
- [ ] Mock behavior matches real implementation contracts
- [ ] No production code paths exist solely to support testing (use feature gates instead)

---

## 5. Accessibility

### Semantic HTML (WCAG 2.1 AA)

- [ ] Semantic elements used: `<nav>`, `<main>`, `<section>`, `<button>`, `<table>`
- [ ] Heading hierarchy is logical (`h1` -> `h2` -> `h3`, no skipped levels)
- [ ] ARIA landmarks identify page regions (`role="navigation"`, `role="main"`)
- [ ] Interactive elements use `<button>` or `<a>`, not `<div onClick>`
- [ ] Form inputs have associated `<label>` elements

### Keyboard Navigation

- [ ] All interactive elements are keyboard-focusable (tab order is logical)
- [ ] Focus is visible on all focusable elements (CRT glow style is acceptable)
- [ ] No keyboard traps (can always tab out of any component)
- [ ] Modal dialogs trap focus correctly and restore focus on close
- [ ] Shortcut keys do not conflict with browser/screen reader defaults

### Screen Reader Support

- [ ] Dynamic content updates use `aria-live` regions (metrics dashboard, log stream)
- [ ] Status messages (success, error, loading) are announced
- [ ] Data tables have proper `<th>` headers with `scope` attributes
- [ ] Icons and decorative images have `aria-hidden="true"` or meaningful `alt` text
- [ ] SSE-driven real-time updates do not overwhelm screen readers (throttle announcements)

### Visual Design

- [ ] Text meets minimum contrast ratio: 4.5:1 for normal text, 3:1 for large text
- [ ] CRT green-on-dark palette passes contrast checks (verify `--green` against `--bg`)
- [ ] Color is not the only means of conveying information (use icons, text labels, patterns)
- [ ] Content is readable and functional at 200% browser zoom
- [ ] Touch/click targets are at least 44x44px on interactive elements
