# .claude Workflow Overhaul — Simplified Architecture

## Summary of Changes

- **Consolidate 7 team skills → 4**: team-plan (NEW), team-implement (NEW, absorbs 5 old skills), team-review (keep), team-debug (keep)
- **Add database-engineer agent** — owns migrations, executes DB queries, schema advisory
- **Agent autonomy** — `permissionMode: bypassPermissions` + `maxTurns` on all 8 agents
- **PostToolUse hook** — auto-format after edits (async)
- **Memory & infrastructure** — agent-memory for remaining agents, agent-colors.json, .trees/ gitignore, Playwright plugin

---

## Wave 1: Skill Consolidation

### DELETE these skill directories:
- `.claude/skills/team-spawn/` — absorbed into team-implement
- `.claude/skills/team-feature/` — absorbed into team-implement
- `.claude/skills/team-delegate/` — absorbed into team-implement
- `.claude/skills/team-status/` — absorbed into team-implement
- `.claude/skills/team-shutdown/` — absorbed into team-implement

### CREATE `.claude/skills/team-plan/SKILL.md`

Replaces plan-mode agent consultation. Single skill for analysis and planning.

```yaml
name: team-plan
description: Analyze scope, explore codebase, and produce implementation plans. Spawns Explore agents for parallel investigation, consults domain expertise, and writes structured plans. Use when user says 'plan this feature', 'analyze scope', 'what would it take to build X', or 'explore before implementing'.
argument-hint: "[feature-description] [--scope path] [--output plan|review]"
allowed-tools:
  - Bash
  - Read
  - Grep
  - Glob
  - Write
  - AskUserQuestion
  - Agent
```

**Phases:**

1. **Load Project Context** — read CLAUDE.md Service Map + `.claude/agents/*.md` to build agent registry and service map (reuse logic from old team-spawn Step 1)

2. **Parallel Exploration** — spawn up to 3 Explore agents focused on:
   - Affected code areas (file discovery, existing patterns)
   - Related tests and coverage gaps
   - Integration points and dependencies
   Each agent searches from its domain perspective (backend patterns, frontend patterns, infra patterns)

3. **Scope Analysis** — classify affected services, identify file ownership boundaries, estimate complexity (small/medium/large per service)

4. **Wave Decomposition** — break into waves with dependency graph:
   - Wave 1: foundations (types, schema, core logic)
   - Wave 2: consumers (handlers, UI, integration)
   - Wave 3: verification (tests, E2E)
   - Wave 4: documentation (if needed)

5. **Plan Output** — write plan to `.claude/plans/` with:
   - File manifest (files to create/modify, one owner per file)
   - Wave structure with dependencies
   - Interface contracts between services
   - Verification commands per service
   - Recommended team preset for `/team-implement`

### CREATE `.claude/skills/team-implement/SKILL.md`

One-stop lifecycle: spawn → assign → monitor → verify → PR → shutdown. Absorbs all orchestration from the 5 deleted skills.

```yaml
name: team-implement
description: Full lifecycle feature implementation — spawns teams, assigns tasks, monitors progress, verifies quality, creates PRs, and shuts down. Absorbs team-spawn, team-feature, team-delegate, team-status, and team-shutdown into one unified workflow. Use when user says 'implement this', 'build this feature', 'start working on X', or 'execute the plan'.
argument-hint: "[feature-or-plan] [--preset name] [--worktree] [--no-worktree] [--shutdown team-name] [--status team-name] [--delegate team-name]"
allowed-tools:
  - Bash
  - Read
  - Write
  - Grep
  - Glob
  - SendMessage
  - AskUserQuestion
  - TeamCreate
  - TeamDelete
  - Agent
  - TaskCreate
  - TaskUpdate
  - TaskList
```

**Sub-commands via flags:**
- No flags: full lifecycle (spawn → implement → verify → PR → shutdown)
- `--status team-name`: show team status only (replaces /team-status)
- `--delegate team-name`: interactive task assignment (replaces /team-delegate)
- `--shutdown team-name`: graceful shutdown only (replaces /team-shutdown)

**Full Lifecycle Phases:**

1. **Load Context** — CLAUDE.md Service Map + agents registry + agent-colors.json (from old team-spawn Step 1)

2. **Resolve Composition** — preset selection with keyword matching (from old team-spawn Step 2). Presets:

   | Preset | Composition |
   |--------|-------------|
   | fullstack | coordinator + all service agents + QA agents |
   | backend-feature | coordinator + backend + database + backend-qa |
   | frontend-feature | coordinator + frontend + frontend-qa |
   | infra | coordinator + infra + backend |
   | docs | coordinator + document-writer |
   | research | 3 general-purpose agents |
   | security | 4 reviewer agents (OWASP, auth, deps, config) |
   | migration | coordinator + 2 service agents + 1 reviewer |
   | refactor | coordinator + 2 service agents + 1 reviewer |
   | hotfix | 1 service agent + 1 QA agent |

3. **Worktree Resolution** — auto-detect active teams, create worktree if needed (from old team-spawn Step 3.5)

4. **Plan Decomposition** — wave-based task breakdown with file ownership (from old team-feature Step 4). If a plan file exists in `.claude/plans/`, use it as input instead of re-analyzing.

5. **GitHub Issues** — create issues with labels, priority, service fields, dependencies (from old team-feature Step 4.5)

6. **Spawn** — lazy per-wave spawning (from old team-feature Step 5):
   - Wave 1 agents spawn immediately
   - Wave 2+ agents deferred in team config
   - Spawn deferred agents when previous wave completes

7. **Assign** — send each agent their owned files, requirements, interface contracts, wave number (from old team-feature Step 6 + old team-delegate)

8. **Monitor** — health monitoring loop (from old team-feature Step 6):
   - Track idle notifications per agent
   - 3+ consecutive idles with in_progress task → context exhaustion
   - Auto-respawn: reuse agent name, transfer task ownership, pass handoff summary
   - Spawn deferred Wave N+1 agents when Wave N completes

9. **Verify** — run quality gates per affected service (from old team-feature Step 7):
   - Backend: `cargo clippy --all-targets && cargo test --lib && cargo fmt --check`
   - Frontend: `npm run build && npm run lint`
   - Cross-service contract validation (grep for shared types/endpoints)

10. **PR** — if worktree active, push branch + `gh pr create` (from old team-feature Step 7.5)

11. **Shutdown** — ordered termination (from old team-shutdown):
    - Persist incomplete work to GitHub Issues
    - Commit uncommitted changes in worktree
    - Push unpushed commits
    - Remove worktree + prune
    - TeamDelete

12. **Report** — final status with work streams, GitHub Issues, verification results

**Status sub-command** (`--status`):
- Load team config, check agent activity (git log, file mtime)
- Classify: active/quiet/stale
- Cross-reference TaskList vs GitHub Issues for drift
- Context exhaustion heuristic (3+ idle notifications)
- Alerts for stale/stuck agents

**Delegate sub-command** (`--delegate`):
- Interactive menu: assign task, send message, broadcast, rebalance, reclaim
- Dynamic agent validation from team config (NOT hardcoded names)
- Blocked task detection via GitHub Issue labels

**Shutdown sub-command** (`--shutdown`):
- Ordered: workers first, coordinator last
- Worktree cleanup with uncommitted/unpushed checks
- Persist to GitHub Issues before cleanup

### KEEP `.claude/skills/team-review/SKILL.md` — no changes needed
### KEEP `.claude/skills/team-debug/SKILL.md` — no changes needed
### KEEP `.claude/skills/team-coordination/SKILL.md` — update preset table only

Update Section 5 Harbangan Team Presets table to match team-implement presets (add refactor, hotfix, research, security, migration; use dynamic descriptions instead of hardcoded agent names).

---

## Wave 2: Database Engineer Agent

### CREATE `.claude/agents/database-engineer.md`

```yaml
---
name: database-engineer
description: PostgreSQL database specialist. Use for designing schemas, writing migrations, optimizing SQL queries, executing database operations, and advising on data modeling. Expert in sqlx compile-time checked queries, PostgreSQL 16, and the project's migration system in config_db.rs.
tools: Read, Edit, Write, Bash, Grep, Glob
permissionMode: bypassPermissions
maxTurns: 80
memory: project
---

You are the Database Engineer for Harbangan. You manage the PostgreSQL schema, migrations, and query optimization.

## Ownership

### You Own
- Database schema design and data modeling
- Migration DDL in `backend/src/web_ui/config_db.rs` (CREATE TABLE, ALTER TABLE, migration version blocks)
- Query performance analysis and optimization
- Database operations via `psql` or sqlx CLI

### You Advise On (but don't own the Rust code)
- sqlx query patterns in handler code (review, suggest improvements)
- Index strategy for query patterns
- Data integrity constraints

### rust-backend-engineer Owns
- Rust handler code that calls sqlx queries
- AppState and connection pool configuration
- Business logic that uses the database

## Migration System

Migrations live in `backend/src/web_ui/config_db.rs` as sequential version blocks:

```rust
// Migration v{N}
sqlx::query("CREATE TABLE IF NOT EXISTS ...")
    .execute(&pool)
    .await?;
```

Key patterns:
- Migrations run sequentially on startup via `run_migrations()`
- Each migration checks a version number in `schema_version` table
- Use `IF NOT EXISTS` / `IF EXISTS` for idempotency
- Never modify existing migration blocks — always add new versioned blocks
- Test migrations against a fresh database AND existing database

## Database Operations

```bash
# Connect to PostgreSQL
docker compose exec db psql -U postgres -d harbangan

# Check schema
docker compose exec db psql -U postgres -d harbangan -c "\dt"
docker compose exec db psql -U postgres -d harbangan -c "\d table_name"

# Check migration version
docker compose exec db psql -U postgres -d harbangan -c "SELECT * FROM schema_version"

# Run EXPLAIN ANALYZE on slow queries
docker compose exec db psql -U postgres -d harbangan -c "EXPLAIN ANALYZE SELECT ..."
```

## Key Tables (current schema)

- `config` — runtime configuration key-value store
- `config_history` — configuration change audit log
- `users` — user accounts (Google SSO + password auth)
- `sessions` — active user sessions
- `api_keys` — per-user API keys (SHA-256 hashed)
- `user_kiro_tokens` — per-user Kiro credentials (encrypted)
- `domain_allowlist` — allowed email domains
- `model_registry` — available AI models with enabled/disabled state
- `guardrail_profiles` / `guardrail_rules` — content safety rules

## After Making Changes

```bash
# Verify migration runs cleanly
cd /Users/hikennoace/ai-gateway/harbangan/backend && cargo build
cd /Users/hikennoace/ai-gateway/harbangan/backend && cargo test --lib config_db::
```
```

### CREATE `.claude/agent-memory/database-engineer/MEMORY.md`

```markdown
# Database Engineer Memory

## Schema Patterns & Gotchas
(populated as the agent learns from database sessions)
```

### UPDATE `.claude/skills/team-coordination/SKILL.md`

Add to File Ownership table (Section 2):
```
| `backend/src/web_ui/config_db.rs` (DDL) | database-engineer | Schema migrations, table creation |
```

Add to Service Map in `CLAUDE.md`:
```
| Database | `backend/src/web_ui/config_db.rs` | PostgreSQL 16, sqlx 0.8, migrations | database, postgresql, schema, migration | `cargo test --lib config_db::` |
```

---

## Wave 3: Agent Autonomy (8 agent files)

Add to YAML frontmatter of each agent (between `---` markers):

| Agent | Add Fields |
|-------|-----------|
| `scrum-master.md` | `permissionMode: bypassPermissions`, `maxTurns: 100`, `skills: [team-coordination]` |
| `rust-backend-engineer.md` | `permissionMode: bypassPermissions`, `maxTurns: 100` |
| `react-frontend-engineer.md` | `permissionMode: bypassPermissions`, `maxTurns: 100` |
| `database-engineer.md` | already set in Wave 2 |
| `devops-engineer.md` | `permissionMode: bypassPermissions`, `maxTurns: 80` |
| `document-writer.md` | `permissionMode: bypassPermissions`, `maxTurns: 60` |
| `backend-qa.md` | `permissionMode: bypassPermissions`, `maxTurns: 80` |
| `frontend-qa.md` | `permissionMode: bypassPermissions`, `maxTurns: 80` |

---

## Wave 4: Hooks & Settings

### CREATE `.claude/hooks/auto-format-after-edit.sh`

```bash
#!/bin/bash
# PostToolUse hook: auto-format files after Edit/Write (async, non-blocking)
INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')

if [ -z "$FILE_PATH" ] || [ ! -f "$FILE_PATH" ]; then
  exit 0
fi

case "$FILE_PATH" in
  *.rs)
    rustfmt "$FILE_PATH" 2>/dev/null || true
    ;;
  *.ts|*.tsx|*.js|*.jsx|*.css)
    PROJ_DIR="$(echo "$FILE_PATH" | sed 's|/frontend/.*|/frontend|')"
    if [ -d "$PROJ_DIR" ]; then
      cd "$PROJ_DIR" && npx prettier --write "$FILE_PATH" 2>/dev/null || true
    fi
    ;;
esac

exit 0
```

### EDIT `.claude/settings.json`

1. Add `PostToolUse` to `hooks` object:
```json
"PostToolUse": [
  {
    "matcher": "Edit|Write",
    "hooks": [
      {
        "type": "command",
        "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/auto-format-after-edit.sh",
        "async": true
      }
    ]
  }
]
```

2. Enable Playwright plugin:
```json
"playwright@claude-plugins-official": true
```

---

## Wave 5: Memory & Infrastructure

### CREATE files:
- `.claude/agent-memory/scrum-master/MEMORY.md` — `# Scrum Master Memory`
- `.claude/agent-memory/react-frontend-engineer/MEMORY.md` — `# React Frontend Engineer Memory`
- `.claude/agent-memory/devops-engineer/MEMORY.md` — `# DevOps Engineer Memory`
- `.claude/agent-memory/document-writer/MEMORY.md` — `# Document Writer Memory`
- `.claude/agent-memory/database-engineer/MEMORY.md` — (created in Wave 2)

### CREATE `.claude/agent-colors.json`
```json
{
  "scrum-master": "#FF6B6B",
  "rust-backend-engineer": "#4ECDC4",
  "react-frontend-engineer": "#45B7D1",
  "database-engineer": "#9B59B6",
  "backend-qa": "#FFA07A",
  "frontend-qa": "#DDA0DD",
  "devops-engineer": "#F0E68C",
  "document-writer": "#98D8C8"
}
```

### EDIT `.gitignore` — add `.trees/`

### UPDATE `.claude/README.md`
- Add database-engineer to agent tables
- Update skills section: 4 team skills (team-plan, team-implement, team-review, team-debug)
- Update preset table with all 10 presets
- Fix model column to show actual values (opus)

### UPDATE `CLAUDE.md`
- Add Database service to Service Map table
- Add database-engineer to agent role keywords
- Remove references to deleted skills (team-spawn, team-feature, team-delegate, team-status, team-shutdown)

---

## Verification

```bash
# 1. Agent frontmatter validation
for f in .claude/agents/*.md; do echo "=== $f ==="; head -20 "$f" | grep -E '^(permissionMode|maxTurns):'; done

# 2. New files exist
ls .claude/skills/team-plan/SKILL.md .claude/skills/team-implement/SKILL.md
ls .claude/agents/database-engineer.md
ls .claude/agent-colors.json
ls .claude/hooks/auto-format-after-edit.sh

# 3. Old skills deleted
test ! -d .claude/skills/team-spawn && echo "team-spawn deleted OK"
test ! -d .claude/skills/team-feature && echo "team-feature deleted OK"
test ! -d .claude/skills/team-delegate && echo "team-delegate deleted OK"
test ! -d .claude/skills/team-status && echo "team-status deleted OK"
test ! -d .claude/skills/team-shutdown && echo "team-shutdown deleted OK"

# 4. JSON valid
python3 -c "import json; json.load(open('.claude/settings.json'))"
python3 -c "import json; json.load(open('.claude/agent-colors.json'))"

# 5. Gitignore
grep '.trees/' .gitignore

# 6. Hook executable
test -x .claude/hooks/auto-format-after-edit.sh
```
