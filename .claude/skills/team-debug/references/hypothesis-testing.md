# Hypothesis Testing Reference

Templates, evidence formats, arbitration decision trees, and Harbangan-specific debugging patterns for the ACH-based team-debug skill.

---

## Hypothesis Task Template

Send this to each investigator agent, filled in with the specific hypothesis they are assigned.

```markdown
## Hypothesis Investigation: {Hypothesis Title}

### Hypothesis Statement

{Clear, falsifiable statement about the root cause}

### Failure Mode Category

{Logic Error | State Corruption | Resource Exhaustion | Integration Failure | Configuration Error | Race Condition}

### Investigation Scope

- Files to examine: {file list or directory}
- Related tests: {test files or module test blocks}
- Git history: {relevant date range or commits, e.g. git log --oneline -10}

### Evidence Criteria

**Confirming evidence** (if I find these, hypothesis is supported):
1. {Observable condition 1}
2. {Observable condition 2}
3. {Observable condition 3}

**Falsifying evidence** (if I find these, hypothesis is wrong):
1. {Observable condition 1}
2. {Observable condition 2}

### Evidence Classification

Classify every piece of evidence found:
- **Direct**: Code that proves/disproves (include snippet + file:line)
- **Correlational**: Timing/pattern that suggests causation (describe pattern)
- **Testimonial**: Log messages or error strings (quote exact message)
- **Absence**: Expected evidence not found (explain what was expected)

### Deliverable

Fill in the Evidence Report Template below and return it. Do NOT attempt fixes.
```

---

## Evidence Report Template

Each investigator returns this after completing their investigation.

```markdown
## Investigation Report: {Hypothesis Title}

### Verdict: {CONFIRMED | PROBABLE | INCONCLUSIVE | RULED_OUT}

### Confidence: {percentage, e.g. 85%}

### Failure Mode Category: {category}

### Confirming Evidence

1. [{type}] `{file}:{line}` -- {description of what was found}
   ```
   {code snippet if Direct evidence}
   ```
2. [{type}] `{file}:{line}` -- {description}

### Contradicting Evidence

1. [{type}] `{file}:{line}` -- {description of what contradicts the hypothesis}

### Causal Chain (if CONFIRMED or PROBABLE)

1. {Root cause} -->
2. {Intermediate effect} -->
3. {Observable symptom reported by user}

### Recommended Fix (if CONFIRMED or PROBABLE)

{Specific code change with file path, line number, and description}

### Additional Notes

{Anything discovered that may be relevant to other hypotheses or future investigation}
```

---

## Arbitration Decision Tree

Used by the coordinator after all investigators have reported.

```
All investigators reported?
|
+-- NO --> Wait for remaining reports (set timeout: 5 min per investigator)
|
+-- YES --> Classify each hypothesis verdict
    |
    +-- Count CONFIRMED (>80% confidence)
    |   |
    |   +-- 0 confirmed
    |   |   |
    |   |   +-- Any PROBABLE (50-80%)?
    |   |   |   |
    |   |   |   +-- YES --> Promote highest-confidence PROBABLE:
    |   |   |   |   - Narrow investigation scope to its specific files
    |   |   |   |   - Assign 2 investigators (original + fresh eyes)
    |   |   |   |   - Return to Phase 3 with focused task
    |   |   |   |
    |   |   |   +-- NO --> All INCONCLUSIVE or RULED_OUT
    |   |   |       - Review triage: was the error domain correct?
    |   |   |       - Generate 3 NEW hypotheses from DIFFERENT
    |   |   |         failure mode categories than Round 1
    |   |   |       - Return to Phase 2
    |   |   |       - If Round 3+: escalate to user with findings
    |   |
    |   +-- 1 confirmed
    |   |   |
    |   |   +-- Confidence >80%?
    |   |   |   |
    |   |   |   +-- YES --> Declare root cause
    |   |   |   |   - Verify causal chain is complete
    |   |   |   |   - Check no PROBABLE hypotheses contradict it
    |   |   |   |   - Proceed to fix
    |   |   |   |
    |   |   |   +-- NO (50-80%) --> Flag as likely cause
    |   |   |       - Recommend targeted verification:
    |   |   |         write a test that would fail if hypothesis is true
    |   |   |       - Do NOT proceed to fix until verified
    |   |
    |   +-- 2+ confirmed
    |       |
    |       +-- Are they causally related?
    |           |
    |           +-- YES --> Compound issue
    |           |   - Identify dependency order (which causes which)
    |           |   - Fix root cause first, verify downstream resolves
    |           |   - If not, fix contributing factors in order
    |           |
    |           +-- NO --> Independent issues
    |               - Rank by confidence level
    |               - Declare highest-confidence as primary root cause
    |               - File the others as separate issues to address
    |
    +-- Evidence Conflict Resolution
        |
        +-- Contradicting evidence across investigators?
            |
            +-- Weigh by evidence strength:
            |   Direct (strong) > Correlational (medium) > Testimonial (weak)
            |
            +-- Check scope: same code path? same conditions?
            |
            +-- Consider compound causation: both true under different conditions?
            |
            +-- If unresolvable: spawn tiebreaker investigation
                with combined scope from both investigators
```

---

## Verdict Definitions

| Verdict | Confidence | Evidence Required | Action |
|---------|------------|-------------------|--------|
| **CONFIRMED** | >80% | 2+ Direct evidence, complete causal chain, zero contradicting evidence | Declare root cause, propose fix |
| **PROBABLE** | 50-80% | 1 Direct + Correlational, partial causal chain, minor gaps | Flag as likely, recommend verification |
| **INCONCLUSIVE** | <50% | Only Correlational/Testimonial, no clear causal chain | Cannot confirm or deny, needs more data |
| **RULED_OUT** | N/A | Direct falsifying evidence found, OR causal chain impossible | Eliminate from consideration |

---

## Harbangan Failure Mode Examples

### Logic Error

| Symptom | Hypothesis | Key Files | Confirming Evidence |
|---------|-----------|-----------|-------------------|
| Malformed streaming response | AWS Event Stream parser mishandles chunk boundary | `backend/src/streaming/mod.rs` | Buffer split at wrong offset, missing length check |
| Wrong API response format | Converter misses field mapping for new API feature | `backend/src/converters/*.rs` | Field present in input struct but absent in output mapping |
| Guardrails false positive | CEL rule expression has incorrect logic | `backend/src/guardrails/` | Rule evaluates to true on benign input, expression error |
| Model not found | Resolver alias table incomplete or regex mismatch | `backend/src/resolver.rs` | Alias not in map, or regex does not match input string |
| useSSE drops events | Frontend SSE hook does not handle reconnection | `frontend/src/lib/useSSE.ts` | No retry logic after EventSource error event |

### State Corruption

| Symptom | Hypothesis | Key Files | Confirming Evidence |
|---------|-----------|-----------|-------------------|
| Stale user data after update | API key cache not invalidated on DB write | `backend/src/middleware/`, `api_key_cache` in AppState | Cache lookup returns old data, no invalidation call after insert |
| Config resets on restart | Config change written to memory but not DB | `backend/src/web_ui/config_db.rs` | `RwLock<Config>` updated but `ConfigDb::save` not called |
| Session valid but user deleted | Session cache retains entry after user removal | `session_cache` in AppState | DashMap entry persists, no eviction on user delete |

### Resource Exhaustion

| Symptom | Hypothesis | Key Files | Confirming Evidence |
|---------|-----------|-----------|-------------------|
| 503 under load | HTTP connection pool exhausted | `backend/src/http_client.rs` | Pool max reached, no timeout on acquisition |
| Increasing latency | Log buffer grows without bound | `log_buffer` in AppState, `backend/src/log_capture.rs` | VecDeque has no max capacity, `.push_back()` without `.pop_front()` |
| Memory growth | Session or OAuth pending map never cleaned | `session_cache`, `oauth_pending` in AppState | No TTL eviction task, map grows monotonically |

### Integration Failure

| Symptom | Hypothesis | Key Files | Confirming Evidence |
|---------|-----------|-----------|-------------------|
| 400 from Kiro API | Request body does not match Kiro API schema | `backend/src/converters/*_to_kiro.rs` | Field name or type mismatch vs Kiro API docs |
| Bedrock guardrail error | AWS credentials or region misconfigured | `backend/src/guardrails/` | SDK returns auth error, region env var missing |
| MCP JSON-RPC error | Tool call payload does not match MCP server schema | `backend/src/mcp/` | JSON-RPC error response with schema validation message |

### Configuration Error

| Symptom | Hypothesis | Key Files | Confirming Evidence |
|---------|-----------|-----------|-------------------|
| Connection refused | Backend port mismatch between docker-compose and Axum bind | `docker-compose.yml`, `backend/src/main.rs` | Port in compose differs from `SERVER_PORT` |
| CORS rejection | Middleware origin allowlist missing frontend domain | `backend/src/middleware/` | `Access-Control-Allow-Origin` does not include request origin |
| OAuth callback fails | `GOOGLE_CALLBACK_URL` does not match registered redirect URI | `.env`, `backend/src/web_ui/google_auth.rs` | URL mismatch between env var and Google Cloud Console |
| Setup mode stuck | `setup_complete` never set to true | `backend/src/routes/mod.rs` | AtomicBool remains false, 503 returned on `/v1/*` |

### Race Condition

| Symptom | Hypothesis | Key Files | Confirming Evidence |
|---------|-----------|-----------|-------------------|
| Intermittent auth failure | Kiro token refresh races with concurrent request | `backend/src/auth/`, `kiro_token_cache` | Two tasks read expired token, both refresh, one overwrites other |
| Duplicate API key hash | Two concurrent key creations produce same hash | `backend/src/web_ui/api_keys.rs` | No uniqueness check before insert, DB constraint violation |
| Config inconsistency | Concurrent config writes via web UI | `backend/src/web_ui/config_db.rs` | `RwLock` released between read and write, TOCTOU gap |

---

## Common Harbangan Debugging Patterns

### Pattern: Request fails with 401 but API key is valid

Hypotheses to generate:
1. **State Corruption**: API key cache contains stale entry (key was regenerated but old hash still cached)
2. **Logic Error**: SHA-256 hash computation differs between key creation and lookup
3. **Race Condition**: Key was just created and cache has not been populated yet

Key files: `backend/src/middleware/`, `backend/src/web_ui/api_keys.rs`, `api_key_cache` in `backend/src/routes/mod.rs`

### Pattern: Streaming response truncated

Hypotheses to generate:
1. **Logic Error**: Event Stream parser does not handle multi-chunk messages spanning buffer boundaries
2. **Integration Failure**: Kiro API sends unexpected event type not handled by parser
3. **Resource Exhaustion**: Response body exceeds buffer limit, truncation recovery fails

Key files: `backend/src/streaming/mod.rs`, `backend/src/truncation.rs`, `backend/src/http_client.rs`

### Pattern: Frontend shows stale data

Hypotheses to generate:
1. **Logic Error**: useSSE hook does not process reconnection, misses update events
2. **State Corruption**: Backend metrics/log SSE stream stops emitting after error
3. **Configuration Error**: Vite proxy misconfigured, delays SSE events

Key files: `frontend/src/lib/useSSE.ts`, `backend/src/metrics/`, `frontend/vite.config.ts`

### Pattern: Works locally, fails in Docker

Hypotheses to generate:
1. **Configuration Error**: Environment variable set locally but missing from docker-compose
2. **Integration Failure**: Container DNS resolution differs from host (service name vs localhost)
3. **Resource Exhaustion**: Container memory/CPU limits too low for workload

Key files: `docker-compose.yml`, `.env`, `backend/Dockerfile`, `frontend/Dockerfile`

### Pattern: Converter produces wrong output format

Hypotheses to generate:
1. **Logic Error**: Field mapping incomplete for specific message type (e.g. tool_use, image content)
2. **Integration Failure**: Upstream API added new field not present in model structs
3. **Logic Error**: Shared logic in `core.rs` handles base case but not variant

Key files: `backend/src/converters/`, `backend/src/models/`
