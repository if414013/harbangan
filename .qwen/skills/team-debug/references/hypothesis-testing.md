# Hypothesis Testing for Harbangan

Use this reference when running `team-debug`.

## Hypothesis template

Each hypothesis should be explicit and falsifiable.

```markdown
Hypothesis: <short title>
Category: <logic error | state corruption | resource exhaustion | integration failure | configuration error | race condition>
Statement: <single-sentence root-cause claim>
Scope:
- files or directories to inspect
- related tests or logs
Confirming evidence:
- what should exist if the hypothesis is true
Falsifying evidence:
- what would prove it wrong
```

## Evidence standards

Every investigation report should label evidence by type:

- `direct`: code or behavior that directly proves or disproves the hypothesis
- `correlational`: timing or pattern that strongly suggests a cause
- `testimonial`: logs, error strings, or user reports
- `absence`: something that should exist but does not

Always include `file:line` references for code evidence.

## Investigator report template

```markdown
Verdict: <confirmed | probable | inconclusive | ruled out>
Confidence: <0-100%>

Confirming evidence:
1. [type] `path:line` - explanation

Contradicting evidence:
1. [type] `path:line` - explanation

Causal chain:
1. root cause
2. intermediate effect
3. observed symptom

Recommended fix path:
- file or module
- concrete change to verify next
```

## Verdict guide

- `confirmed`: strong direct evidence and a coherent causal chain
- `probable`: meaningful evidence but at least one important gap remains
- `inconclusive`: weak or mixed evidence
- `ruled out`: direct contradictory evidence or an impossible causal chain

## Harbangan failure categories

### Logic error

- converter field mapping missed a supported field
- streaming parser mishandles a chunk boundary
- frontend SSE hook fails to reconnect or clean up

### State corruption

- cache invalidation missing after config or API-key updates
- session or user state becomes stale after mutation

### Resource exhaustion

- connection pool saturation
- unbounded in-memory buffers or caches
- repeated expensive work in render or request hot paths

### Integration failure

- upstream schema mismatch
- OAuth or provider callback contract mismatch
- Docker or service wiring mismatch between frontend and backend

### Configuration error

- missing env var
- wrong runtime mode assumptions
- model alias or provider configuration not loaded

### Race condition

- stale token refresh timing
- concurrent mutation without a safe ownership boundary
- SSE or async shutdown ordering bugs

## Arbitration rules

- Prefer direct evidence over correlational evidence.
- If two hypotheses are both partially true, describe the dependency order instead of pretending there is only one cause.
- If no hypothesis is convincing, say what extra log, repro step, or targeted test would settle the issue.
