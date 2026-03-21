# Add show/hide password toggle with eye icon to set password form

## Context

The PasswordChange page (`frontend/src/pages/PasswordChange.tsx`) has 3 password inputs (current, new, confirm) with no visibility toggle. The user wants an eye icon toggle to reveal/hide the raw password text.

## Existing Patterns

- `.btn-reveal` class already exists in `components.css` (line 648) — used in Config.tsx for text-based "reveal"/"hide" toggles
- No icon library or eye SVGs exist in the codebase — all icons are either text or inline SVG
- Password inputs use `.auth-input` class (full-width, block-level, `margin-bottom: 14px`)

## Approach

### 1. Add inline SVG eye icons + toggle to `PasswordChange.tsx`

For each password input, wrap in a container div and add a toggle button with eye/eye-off SVG:

- One `showPassword` boolean state that toggles all 3 fields together (simpler UX — user is comparing new vs confirm)
- Toggle button positioned inside the input area (absolute, right side)
- Input `type` switches between `"password"` and `"text"` based on state
- Eye-open SVG when hidden (click to show), eye-off SVG when visible (click to hide)

### 2. Add CSS for the password input wrapper in `components.css`

```css
.password-input-wrapper {
  position: relative;
  margin-bottom: 14px;
}

.password-input-wrapper .auth-input {
  margin-bottom: 0;         /* wrapper owns the margin now */
  padding-right: 40px;      /* space for the toggle button */
}

.btn-password-toggle {
  position: absolute;
  right: 8px;
  top: 50%;
  transform: translateY(-50%);
  background: none;
  border: none;
  color: var(--text-tertiary);
  cursor: pointer;
  padding: 4px;
  display: flex;
  align-items: center;
}

.btn-password-toggle:hover {
  color: var(--text);
}
```

### Files to modify

1. **`frontend/src/pages/PasswordChange.tsx`** — wrap each input, add toggle state + button with inline SVG
2. **`frontend/src/styles/components.css`** — add `.password-input-wrapper` and `.btn-password-toggle` styles

### Scope note

Only the PasswordChange form (set/change password). Login page is out of scope unless explicitly requested.

## Verification

1. `cd frontend && npm run build` — zero errors
2. `cd frontend && npm run lint` — zero errors
3. Playwright: navigate to set password form → verify eye icon visible → click toggle → password text becomes visible → click again → hidden
