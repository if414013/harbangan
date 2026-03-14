# Remove Manual TLS/nginx Artifacts

Docker Compose is dev-only. TLS will be handled by k8s Ingress later.

## Files to Delete
- `frontend/nginx.conf` — unused production nginx config
- `certs/cert.pem`, `certs/key.pem` — unused self-signed certs

## Files to Edit

### Core Config
- `.env.example` — remove `DOMAIN`, `EMAIL`, `USE_LOCAL_CA`, `STAGING` vars; change `GOOGLE_CALLBACK_URL` to `http://localhost:9999/_ui/api/auth/google/callback`
- `.gitignore` — remove `certs/` entry if present
- `CLAUDE.md` — update architecture diagram: remove nginx-certbot layer, show `frontend:5173` and `backend:9999` directly. Remove TLS env vars from table. Update "Docker Services" section.
- `.claude/CLAUDE.md` — remove nginx reference if present
- `.claude/agents/devops-engineer.md` — remove nginx/TLS/certbot from agent scope
- `.claude/rules/web-ui.md` — remove "served by nginx" references (it's Vite in dev)

### Documentation (gh-pages/)
- `gh-pages/docs/deployment.md` — remove TLS/cert setup instructions
- `gh-pages/docs/architecture/index.md` — update to remove nginx layer
- `gh-pages/docs/architecture/request-flow.md` — remove nginx from flow
- `gh-pages/docs/troubleshooting.md` — remove cert/nginx troubleshooting
- `gh-pages/docs/configuration.md` — remove TLS config section
- `gh-pages/docs/quickstart.md` — remove TLS setup steps
- `gh-pages/docs/getting-started.md` — remove TLS setup steps

## Verification
```bash
grep -ri "nginx\|certbot\|letsencrypt\|ssl_cert\|USE_LOCAL_CA\|jonasal" --include='*.md' --include='*.yml' --include='*.conf' --include='*.toml' --include='*.example' . | grep -v node_modules | grep -v target | grep -v '.git/'
```
