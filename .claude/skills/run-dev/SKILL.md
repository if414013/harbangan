# Run Dev

Start, stop, or rebuild Harbangan services in gateway (proxy-only) or full (DB + UI) mode.

## Usage

```
/run-dev <mode> <action>
```

- **mode**: `gateway` or `full`
- **action**: `up`, `down`, or `build`

## Modes

| Mode | Compose File | Env File | Services |
|------|-------------|----------|----------|
| `gateway` | `docker-compose.gateway.yml` | `.env.proxy` | Single backend container, no DB/UI |
| `full` | `docker-compose.yml` | `.env` | Backend + Frontend + PostgreSQL |

## Actions

| Action | Command |
|--------|---------|
| `up` | Build (if needed) and start services detached |
| `down` | Stop and remove containers |
| `build` | Rebuild images without starting |

## Steps

1. Parse `<mode>` and `<action>` from arguments. If missing, ask the user.

2. Resolve compose command based on mode:
   - `gateway`: `docker compose -f docker-compose.gateway.yml --env-file .env.proxy`
   - `full`: `docker compose`

3. Execute based on action:
   - `up`: `{compose_cmd} up -d --build`
   - `down`: `{compose_cmd} down`
   - `build`: `{compose_cmd} build`

4. After `up`, show container status: `docker compose ... ps`
5. After `up` in gateway mode, show logs: `docker logs harbangan-gateway-1 2>&1 | tail -30`

## Examples

```
/run-dev gateway up      # Start proxy-only gateway
/run-dev gateway down    # Stop gateway
/run-dev full up         # Start full stack with DB + UI
/run-dev full build      # Rebuild full stack images
```
