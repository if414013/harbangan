# DevOps Engineer Memory

Notes:
- Agent threads always have their cwd reset between bash calls, as a result please only use absolute file paths.
- In your final response, share file paths (always absolute, never relative) that are relevant to the task. Include code snippets only when the exact text is load-bearing (e.g., a bug you found, a function signature the caller asked for) -- do not recap code you merely read.
- For clear communication with the user the assistant MUST avoid using emojis.
- Do not use a colon before tool calls. Text like "Let me read the file:" followed by a read tool call should just be "Let me read the file." with a period.

## Docker Compose Files

| File | Purpose | Usage |
|------|---------|-------|
| `docker-compose.yml` | Full stack (production-like) | `docker compose up` |
| `docker-compose.dev.yml` | Dev hot-reload | `docker compose -f docker-compose.dev.yml up` |
| `docker-compose.gateway.yml` | Proxy-only (no DB/SSO) | `docker compose -f docker-compose.gateway.yml up` |
| `docker-compose.prod.yml` | Pre-built images from ghcr.io | `IMAGE_TAG=v1.0.0 docker compose -f docker-compose.prod.yml up` |

See `docker-dev-setup.md` for detailed notes on the dev Docker infrastructure.

## Quality Gates

```bash
docker compose config --quiet                                          # Validate full stack
docker compose -f docker-compose.dev.yml config --quiet                # Validate dev hot-reload
docker compose -f docker-compose.gateway.yml config --quiet            # Validate proxy-only
docker compose -f docker-compose.prod.yml config --quiet               # Validate prod
docker compose build                                                   # Build images
```

## Owned Files

- `docker-compose.yml`, `docker-compose.dev.yml`, `docker-compose.gateway.yml`, `docker-compose.prod.yml`
- `frontend/Dockerfile`, `backend/Dockerfile`, `backend/Dockerfile.dev`
- `backend/entrypoint.sh`, `backend/rebuild.sh`
- `.env.example`

## Stale Binary Bug (macOS + BuildKit)

Docker builds complete in seconds but binary does not contain new code. Root cause: BuildKit's content-addressed cache on macOS serves stale layers even with `--no-cache`. Fix: `docker builder prune -af` before rebuilding. Automated by `backend/rebuild.sh`.
