# Plan: Remove Production Docker Compose

## Context

The project currently has 4 docker-compose files. We want to simplify to just 2 deployment modes:
- **Development** (`docker-compose.yml` + `docker-compose.dev.yml`) — full stack with DB, backend, frontend
- **Gateway proxy** (`docker-compose.gateway.yml`) — single backend container, no DB/SSO

The production compose (`docker-compose.prod.yml`) pulls pre-built images from ghcr.io and deploys to a VPS. It's being removed along with its env template and the CI deploy workflow that depends on it.

## File Manifest

| File | Action | Owner |
|------|--------|-------|
| `docker-compose.prod.yml` | delete | devops-engineer |
| `.env.prod.example` | delete | devops-engineer |
| `.github/workflows/deploy.yml` | delete | devops-engineer |
| `.claude/agent-memory/devops-engineer/MEMORY.md` | modify — remove prod references | devops-engineer |
| `CLAUDE.md` | no change needed — already says "two modes: full deployment + proxy-only" | — |

## Wave 1: Delete files and clean references

- [ ] Delete `docker-compose.prod.yml` (assigned: devops-engineer)
- [ ] Delete `.env.prod.example` (assigned: devops-engineer)
- [ ] Delete `.github/workflows/deploy.yml` (assigned: devops-engineer)
- [ ] Update `.claude/agent-memory/devops-engineer/MEMORY.md` — remove the `docker-compose.prod.yml` row from the compose table, remove the `docker compose -f docker-compose.prod.yml config --quiet` validation line, remove it from the file list (assigned: devops-engineer)

## Verification

```bash
# Ensure remaining compose files still validate
docker compose config --quiet
docker compose -f docker-compose.dev.yml config --quiet
docker compose -f docker-compose.gateway.yml config --quiet

# Confirm no dangling references
grep -r "docker-compose.prod" --include="*.yml" --include="*.md" --include="*.rs" --include="*.ts" .
grep -r "\.env\.prod" --include="*.yml" --include="*.md" --include="*.rs" --include="*.ts" .
```

Note: References in `.claude/plans/` (old completed plans) are historical and don't need updating.

## Branch

`chore/remove-prod-compose`
