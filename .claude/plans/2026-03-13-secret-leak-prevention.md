# Secret Leak Prevention Strategy

## Consultation Summary

- **Codebase scan**: No hardcoded secrets found. `.gitignore` covers `.env*`, certs, IDE files. Docker images clean. Test data uses obviously fake values.
- **GitHub repo**: Public (`if414013/harbangan`). Branch protection enabled (1 review, no force push). No secret scanning, no CODEOWNERS, no Dependabot.
- **Claude hooks**: One PreToolUse hook exists (`enforce-edit-for-large-files.sh`). Pattern: shell script reads JSON stdin, outputs `hookSpecificOutput` JSON.
- **Known allowlist**: `QWEN_OAUTH_CLIENT_ID=f0304373b74a44d2b584a3fb70ca9e56` (public device flow ID, appears in 10+ files).
- **Gap found**: `e2e-tests/.auth/` not in `.gitignore` — session cookies at risk.

## 6-Layer Defense (all layers, @if414013 as CODEOWNER)

### Layer 1: GitHub Platform (manual — repo settings)
- Enable **Secret Scanning** + **Push Protection** (Settings → Code security and analysis)
- Add `gitleaks` job as required status check in branch protection rules

### Layer 2: Claude Code Hooks (`.claude/hooks/`)
- `scan-secrets-before-write.sh` — PreToolUse for Write/Edit, regex scan for:
  - AWS access keys (`AKIA[0-9A-Z]{16}`), private keys (`-----BEGIN.*PRIVATE KEY-----`)
  - DB connection strings with passwords (`postgres://.*:.*@`)
  - High-entropy values assigned to `_SECRET`/`_TOKEN`/`_PASSWORD`/`_KEY` vars
  - Allowlist: `.env.example`, test files, Qwen public client ID, placeholder values
- `block-sensitive-commits.sh` — PreToolUse for Bash, blocks `git add`/`git commit` of `.env*` (except `.env.example`), `certs/`, `*.pem`, `*.key`, `e2e-tests/.auth/`
- Update `.claude/settings.json` hooks array with new matchers

### Layer 3: Agent Rules
- `.claude/rules/secrets.md` — never write real credentials, use env vars, refuse to echo user-provided secrets, use placeholder values, never stage sensitive files

### Layer 4: Local Git Hooks (pre-commit framework + gitleaks)
- `.pre-commit-config.yaml` with `gitleaks/gitleaks` hook
- `.gitleaks.toml` — shared config for local + CI, allowlist for Qwen client ID, `.env.example`, test fixtures
- Developer setup: `pip install pre-commit && pre-commit install`

### Layer 5: CI Pipeline
- `.github/workflows/security.yml` — `gitleaks/gitleaks-action@v2` on PR + push to main, uses `.gitleaks.toml`

### Layer 6: Governance & Documentation
- `CODEOWNERS` — `@if414013` reviews: `.env.example`, `docker-compose*.yml`, `.claude/hooks/`, `.github/workflows/`, `backend/src/auth/`, `backend/src/middleware/`
- `SECURITY.md` — vulnerability reporting policy (email, not public issue)
- `.github/dependabot.yml` — weekly for cargo, npm, github-actions
- PR template + issue templates — add "no credentials" warnings
- `.gitignore` — add `e2e-tests/.auth/`, `.pre-commit-cache/`
- `frontend/.dockerignore` — add `.env*`

## Files to Create/Modify

| File | Action |
|------|--------|
| `.claude/hooks/scan-secrets-before-write.sh` | Create — secret pattern scanner for Write/Edit |
| `.claude/hooks/block-sensitive-commits.sh` | Create — blocks staging sensitive files |
| `.claude/settings.json` | Edit — add 3 new PreToolUse hook entries |
| `.claude/rules/secrets.md` | Create — agent behavioral rules |
| `.gitleaks.toml` | Create — shared scanning config + allowlists |
| `.pre-commit-config.yaml` | Create — pre-commit framework config |
| `.github/workflows/security.yml` | Create — CI secret scanning job |
| `.github/CODEOWNERS` | Create — owner review for security files |
| `SECURITY.md` | Create — vulnerability reporting policy |
| `.github/dependabot.yml` | Create — dependency update automation |
| `.github/PULL_REQUEST_TEMPLATE.md` | Edit — add security checklist item |
| `.github/ISSUE_TEMPLATE/bug-report.yml` | Edit — add "no credentials" warning |
| `.github/ISSUE_TEMPLATE/feature-request.yml` | Edit — add "no credentials" warning |
| `.github/ISSUE_TEMPLATE/task.yml` | Edit — add "no credentials" warning |
| `.gitignore` | Edit — add `e2e-tests/.auth/`, `.pre-commit-cache/` |
| `frontend/.dockerignore` | Edit — add `.env*` |
| `CLAUDE.md` | Edit — add Security Practices section |

## Implementation Waves

- **Wave 1**: `.gitignore` fix, `frontend/.dockerignore`, `.claude/rules/secrets.md`, Claude hooks
- **Wave 2**: `.gitleaks.toml`, `.pre-commit-config.yaml`, `.github/workflows/security.yml`
- **Wave 3**: `CODEOWNERS`, `SECURITY.md`, `.github/dependabot.yml`, template updates, `CLAUDE.md`
- **Manual**: Enable GitHub Secret Scanning + Push Protection in repo settings

## Verification
```bash
# Test Claude hooks
echo '{"tool_input":{"file_path":"test.rs","content":"AKIA1234567890ABCDEF"}}' | .claude/hooks/scan-secrets-before-write.sh
# Test gitleaks config
gitleaks detect --config .gitleaks.toml --no-git --source . -v
# Test pre-commit
pre-commit run --all-files
```
