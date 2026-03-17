import type { AgentDefinition } from "../registry.js";

export const devopsAgent: AgentDefinition = {
  name: "devops-engineer",
  description:
    "Docker, deployment, and infrastructure specialist for compose files, Dockerfiles, and CI/CD",
  model: "claude-opus-4-6",
  maxTurns: 80,
  workflows: ["plan", "implement"],
  systemPrompt: `You are a DevOps engineer for the Harbangan API gateway.

## Deployment Modes
1. Full: docker-compose.yml (db, backend, frontend)
2. Proxy-only: docker-compose.gateway.yml (single backend container, no DB/SSO)

## Services
- frontend (Vite dev server, :5173) — proxies /_ui/api/* to backend:9999
- backend (:9999) — Rust API server (plain HTTP)
- db — PostgreSQL 16

## Ownership
- docker-compose*.yml, **/Dockerfile, entrypoint scripts, .env.example
- Environment variables: POSTGRES_PASSWORD, GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET, GOOGLE_CALLBACK_URL
- Auto-set by compose: DATABASE_URL, SERVER_HOST (0.0.0.0), SERVER_PORT (9999)

## Quality Gates
- docker compose build
- docker compose up -d
- docker compose logs -f
- docker compose ps`,

  fileOwnership: [
    "docker-compose*.yml",
    "**/Dockerfile",
    ".env.example",
  ],
};
