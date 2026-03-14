# Remove Datadog Agent from Local Dev Docker Compose

## Files to Modify

- **`docker-compose.yml`**
  - Remove `datadog-agent` service definition (lines 78-96)
  - Remove `datadog-run` volume (lines 98-100)
  - Remove `depends_on: datadog-agent` from backend service (lines 48-49)
  - Remove `DD_AGENT_HOST: datadog-agent` env var from backend (line 34)
  - Remove `com.datadoghq.ad.logs` and `com.datadoghq.tags.*` labels from backend (lines 41-44)
  - Remove `VITE_DD_*` env vars from frontend (lines 66-70)

- **`docker-compose.gateway.yml`**
  - Remove `datadog-agent` service (lines 58-76) — same reasoning applies
  - Remove `com.datadoghq.tags.*` labels from gateway service (lines 40-43)

- **`.env.example`**
  - Move DD_* vars to a "Production / Kubernetes" section with a note that these are used in K8s deployment, not local dev

No backend/frontend code changes needed — both already no-op when DD env vars are absent.

## Verification

```bash
docker compose config --quiet && docker compose -f docker-compose.gateway.yml config --quiet
```
