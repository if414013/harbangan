# {Project Name} — Product Guidelines

## Voice & Tone

### Brand Personality

{Describe the product's core voice attributes — e.g., technical and precise, confident but not arrogant, direct without being terse.}

| Attribute | We Are | We Are Not |
|-----------|--------|------------|
| {attribute-1} | {positive description} | {anti-pattern} |
| {attribute-2} | {positive description} | {anti-pattern} |
| {attribute-3} | {positive description} | {anti-pattern} |

### Context-Specific Tones

| Context | Tone | Example |
|---------|------|---------|
| Success / Confirmation | {e.g., concise, affirming} | {example message} |
| Error / Failure | {e.g., direct, diagnostic, actionable} | {example message} |
| Onboarding / Setup | {e.g., guided, step-by-step} | {example message} |
| Warning / Caution | {e.g., clear, non-alarmist} | {example message} |
| Empty State | {e.g., instructive, encouraging} | {example message} |

### rkgw Defaults

> Adapted for a CRT terminal aesthetic and technical developer audience.

- Use monospace-friendly language: short, precise, unambiguous.
- Prefer technical terms over plain-language synonyms when the audience expects them (e.g., "token" not "credential unit").
- Avoid marketing-speak, exclamation marks, and filler words.
- Status messages should read like structured log output: `[OK] Configuration loaded` not "Your configuration has been successfully loaded!"

---

## Design Principles

Define 2-3 core principles that guide every product decision. Each principle includes concrete do/don't guidance.

### Principle 1: {Name}

> {One-sentence description of the principle.}

| Do | Don't |
|----|-------|
| {Concrete positive example} | {Concrete anti-pattern} |
| {Concrete positive example} | {Concrete anti-pattern} |
| {Concrete positive example} | {Concrete anti-pattern} |

### Principle 2: {Name}

> {One-sentence description of the principle.}

| Do | Don't |
|----|-------|
| {Concrete positive example} | {Concrete anti-pattern} |
| {Concrete positive example} | {Concrete anti-pattern} |
| {Concrete positive example} | {Concrete anti-pattern} |

### Principle 3: {Name}

> {One-sentence description of the principle.}

| Do | Don't |
|----|-------|
| {Concrete positive example} | {Concrete anti-pattern} |
| {Concrete positive example} | {Concrete anti-pattern} |
| {Concrete positive example} | {Concrete anti-pattern} |

### rkgw Defaults

- **Transparency over magic**: Show what the gateway is doing (streaming status, token counts, model resolution). Never hide complexity from a technical audience.
- **Density over whitespace**: The CRT aesthetic rewards information-dense layouts. Prefer tables and compact displays over cards and spacious padding.
- **Fail loud, recover fast**: Surface errors immediately with diagnostic detail. Never swallow errors silently.

---

## Accessibility Standards

### Target Compliance

**WCAG 2.1 Level AA** — All user-facing interfaces must meet this baseline.

### Checklist by Pillar

#### Perceivable

- [ ] All non-text content has text alternatives (alt text, aria-label)
- [ ] Color is never the sole means of conveying information
- [ ] Minimum contrast ratio: 4.5:1 for normal text, 3:1 for large text
- [ ] Text can be resized to 200% without loss of content or functionality
- [ ] CRT glow effects do not reduce readability below contrast thresholds

#### Operable

- [ ] All functionality is available via keyboard
- [ ] No keyboard traps — users can navigate away from any component
- [ ] Focus indicators are visible (use `var(--glow-sm)` or equivalent outline)
- [ ] Skip-to-content link is available on pages with repeated navigation
- [ ] No content flashes more than 3 times per second

#### Understandable

- [ ] Page language is declared (`lang="en"`)
- [ ] Form inputs have visible labels (not placeholder-only)
- [ ] Error messages identify the field and describe the issue
- [ ] Navigation is consistent across pages

#### Robust

- [ ] Valid, semantic HTML (use `<main>`, `<nav>`, `<section>`, `<button>`)
- [ ] ARIA roles used only when native semantics are insufficient
- [ ] Interactive components work with assistive technologies
- [ ] SSE-driven updates announced via `aria-live` regions

### Testing Protocol

| Method | Frequency | Tools |
|--------|-----------|-------|
| Automated scan | Every build | axe-core, Lighthouse |
| Keyboard navigation | Every new component | Manual |
| Screen reader validation | Before release | VoiceOver (macOS), NVDA (Windows) |
| Color contrast check | Every style change | Browser DevTools |

---

## Error Handling Patterns

### Severity Levels

| Level | Definition | User Impact | Example |
|-------|-----------|-------------|---------|
| **Critical** | Service is unavailable or data loss risk | Blocks all workflows | Database connection lost, auth provider down |
| **Error** | Operation failed but service continues | Blocks current action | API request rejected, model not found |
| **Warning** | Degraded but functional | May affect results | Token limit approaching, rate limit near threshold |
| **Info** | Notable event, no action required | Awareness only | Config reloaded, model cache refreshed |

### User-Facing Message Structure

Every error message visible to users must follow this three-part structure:

```
1. WHAT happened  — State the problem clearly.
2. WHY it happened — Provide the cause or context (when known).
3. HOW to fix it   — Give a concrete next step or recovery action.
```

#### Examples

**API error (structured JSON):**
```json
{
  "error": {
    "type": "authentication_error",
    "message": "API key is invalid or expired.",
    "cause": "The provided key does not match any active keys in the system.",
    "action": "Generate a new API key from the admin panel at /_ui/settings/api-keys."
  }
}
```

**Web UI error (terminal-style):**
```
[ERR] Model "claude-4-opus" not found
      Cause: No matching model ID or alias in the resolver
      Fix:   Check available models at /v1/models or update your request
```

**Inline form validation:**
```
Invalid domain format. Use a fully qualified domain (e.g., example.com). Do not include protocol or path.
```

### Anti-Patterns

| Instead of | Write |
|------------|-------|
| "Something went wrong" | "Request failed: upstream returned 503 (Service Unavailable)" |
| "Error" | "Authentication failed: token expired 12 minutes ago" |
| "Please try again later" | "Kiro API is rate-limited. Retry in 30 seconds or reduce request frequency." |
| "Invalid input" | "Model ID must be alphanumeric with hyphens (e.g., claude-3-sonnet)" |

### rkgw-Specific Conventions

- API responses: Use the `ApiError` enum from `backend/src/error.rs`. Every variant maps to an HTTP status code and structured JSON body.
- Streaming errors: Emit an SSE `error` event with the same three-part structure before closing the stream.
- Web UI: Display errors in the terminal-style log panel. Use `var(--red)` for critical/error, `var(--yellow)` for warnings.
- Never expose internal stack traces, file paths, or database details in user-facing messages. Log them at `error!()` level server-side.
