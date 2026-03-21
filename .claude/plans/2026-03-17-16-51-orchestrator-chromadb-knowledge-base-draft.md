# ChromaDB Knowledge Base for Orchestrator

## Context

The orchestrator service (Wave 1 complete) needs a project knowledge base so agents can learn from past work. Currently, each agent invocation is stateless — agents can't reference prior decisions, task outcomes, or patterns discovered. Adding a ChromaDB-backed KB enables semantic retrieval of project knowledge (architecture decisions, task summaries, agent learnings, incident reports) and automatic context injection into workflow prompts.

**User choices**: ChromaDB (lightweight, embeddable), Project Knowledge only (not codebase RAG), no budget constraints.

## Architecture

```
Workflow invoked
  → KnowledgeRetriever.getContext(task description)
  → ChromaDB query (semantic search, top 5 results)
  → Inject "## Relevant Project Knowledge" into prompt
  → Agent executes with additional context
  → Task completes
  → KnowledgeIngester.ingestTaskResult(task)
  → ChromaDB upsert (auto-indexed for future queries)
```

ChromaDB runs as a sidecar container. Uses default ONNX MiniLM embeddings (local, no extra API key). Single `project_knowledge` collection with metadata type filtering.

## New Module: `src/knowledge/`

| File | Purpose |
|------|---------|
| `types.ts` | `KnowledgeEntry`, `KnowledgeCreateInput`, `KnowledgeSearchResult`, `KnowledgeContext` types |
| `knowledge-base.ts` | ChromaDB client wrapper — CRUD + semantic search on `project_knowledge` collection |
| `ingestion.ts` | Auto-index task outputs after completion (type resolution, title generation, tag extraction) |
| `retrieval.ts` | Build formatted context string for prompt injection (max 5 results, 1500 chars each) |

### Knowledge Types

| Type | Source | Example |
|------|--------|---------|
| `decision` | Plan workflows, manual | "Use Axum for backend because of async performance" |
| `task_summary` | Auto-ingested from completed tasks | "Implemented rate limiting on proxy endpoints" |
| `learning` | Agent discoveries, manual | "cargo clippy must run before fmt to catch borrow issues" |
| `incident` | Auto-ingested from failed tasks | "Build failed because sqlx couldn't find migration" |

## Changes to Existing Files

### `config.ts` — Add optional ChromaDB config
```
chromaUrl: z.string().url().optional()   ← from CHROMA_URL env var
```

### `workflows/base.ts` — Extend WorkflowContext
```typescript
knowledgeRetriever: KnowledgeRetriever | null  // null when KB disabled
```

### `index.ts` — Three additions
1. **Initialize KB** if `config.chromaUrl` is set (creates KnowledgeBase, Ingester, Retriever)
2. **Wire into WorkflowContext** — pass `knowledgeRetriever` to all workflows
3. **Auto-ingest hook** — after task completion, call `ingester.ingestTaskResult(task)`

### Workflow files — Add context retrieval
Each workflow gets KB context injection before prompt construction:

| Workflow | Knowledge types queried |
|----------|----------------------|
| `plan.ts` | `decision`, `learning` |
| `implement.ts` | `decision`, `learning`, `task_summary` |
| `pr-review.ts` | `decision`, `learning`, `incident` |
| `docs.ts` | `decision`, `task_summary` |
| `qa.ts` | `learning`, `incident` |
| `kanban.ts` | No KB injection |

### `api/router.ts` — New KB endpoints
```
GET    /api/v1/knowledge          — List entries (type/limit/offset filters)
GET    /api/v1/knowledge/:id      — Get single entry
POST   /api/v1/knowledge          — Create entry (manual)
PUT    /api/v1/knowledge/:id      — Update entry
DELETE /api/v1/knowledge/:id      — Delete entry
POST   /api/v1/knowledge/search   — Semantic search { query, type?, limit? }
GET    /api/v1/knowledge/stats    — Collection count + type breakdown
```
All return 503 if KB not configured.

### `api/validators.ts` — Add Zod schemas
`knowledgeCreateSchema`, `knowledgeUpdateSchema`, `knowledgeSearchSchema`, `knowledgeListSchema`

### `docker-compose.yml` — Add ChromaDB sidecar
```yaml
chromadb:
  image: chromadb/chroma:0.6.3
  volumes: [chroma_data:/chroma/chroma]
  environment: { IS_PERSISTENT: "true", ANONYMIZED_TELEMETRY: "false" }
  healthcheck: curl http://localhost:8000/api/v1/heartbeat
```
Orchestrator gets `depends_on: chromadb` and `CHROMA_URL: http://chromadb:8000`.

### Other files
- `.env.example` — add `CHROMA_URL=http://chromadb:8000`
- `package.json` — add `chromadb` dependency

## Implementation Waves

**Wave 1 — New knowledge module** (no dependencies on existing code):
1. `src/knowledge/types.ts`
2. `src/knowledge/knowledge-base.ts`
3. `src/knowledge/ingestion.ts`
4. `src/knowledge/retrieval.ts`

**Wave 2 — Integration** (depends on Wave 1):
5. `src/config.ts` — add `chromaUrl`
6. `src/workflows/base.ts` — extend `WorkflowContext`
7. `src/index.ts` — KB init, wiring, auto-ingestion hook
8. All 5 workflow files — KB context retrieval

**Wave 3 — API + Infra** (partially parallel with Wave 2):
9. `src/api/validators.ts` — KB schemas
10. `src/api/router.ts` — KB endpoints
11. `docker-compose.yml` + `.env.example` — ChromaDB sidecar
12. `package.json` — add dependency

**Wave 4 — Tests**:
13. `tests/knowledge-base.test.ts` — unit tests (ingestion logic) + integration tests (ChromaDB ops, gated by CHROMA_URL)

## Verification

1. `npm install` — chromadb installs
2. `npx tsc --noEmit` — TypeScript compiles clean
3. `npx vitest run` — all tests pass (existing 53 + new KB tests)
4. `docker compose up` — ChromaDB + orchestrator start, health checks pass
5. Manual: `POST /api/v1/knowledge` with a decision → `POST /api/v1/knowledge/search` returns it
6. Manual: dispatch a plan task → verify auto-ingestion → next task prompt includes KB context

## Critical Files

- `orchestrator/src/knowledge/knowledge-base.ts` — Core ChromaDB wrapper (new)
- `orchestrator/src/knowledge/ingestion.ts` — Auto-ingestion logic (new)
- `orchestrator/src/knowledge/retrieval.ts` — Context building (new)
- `orchestrator/src/index.ts` — Wiring changes
- `orchestrator/src/workflows/base.ts` — WorkflowContext extension
- `orchestrator/src/api/router.ts` — New endpoints
- `orchestrator/docker-compose.yml` — ChromaDB sidecar
