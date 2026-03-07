# Dependency Graph Patterns

Visual patterns for task dependency design in rkgw agent teams. Choose the pattern that matches your feature's architecture.

## Pattern 1: Fully Independent (Maximum Parallelism)

```
Task A ──┐
Task B ──┼──> Final Integration
Task C ──┘
```

- **Parallelism**: Maximum — all tasks run simultaneously
- **Risk**: Integration may reveal incompatibilities late
- **Use when**: Tasks operate on completely separate modules
- **Mitigation**: Define interface contracts before starting

### rkgw Example: Independent Module Features

```
Guardrails CEL rules ──┐
MCP tool discovery     ──┼──> Integration test
Metrics dashboard CSS  ──┘
```

Each touches separate files (`guardrails/`, `mcp/`, `frontend/src/styles/`) with no overlap.

---

## Pattern 2: Sequential Chain (No Parallelism)

```
Task A ──> Task B ──> Task C ──> Task D
```

- **Parallelism**: None — each task waits for the previous
- **Risk**: Bottleneck at each step; one delay cascades
- **Use when**: Each task depends on the output of the previous (avoid if possible)
- **Mitigation**: Keep chain short; extract independent work into parallel tracks

### rkgw Example: New API Format Support

```
Define Kiro model types ──> Write converter ──> Wire route handler ──> Add tests
(models/)                   (converters/)       (routes/mod.rs)       (tests)
```

Each step needs the previous step's output to compile.

---

## Pattern 3: Diamond (Shared Foundation)

```
           ┌──> Task B ──┐
Task A ──> ┤             ├──> Task D
           └──> Task C ──┘
```

- **Parallelism**: B and C run in parallel after A completes
- **Risk**: A is a bottleneck; D must wait for both B and C
- **Use when**: B and C both need output from A (e.g., shared types)
- **Mitigation**: Keep A minimal — only shared types/interfaces

### rkgw Example: New Feature with Backend + Frontend

```
                  ┌──> Backend route handler    ──┐
Shared types/API ──>┤  (rust-backend-engineer)    ├──> E2E integration
contract           └──> Frontend page/component ──┘    (frontend-qa)
                       (react-frontend-engineer)
```

Shared types define the API contract. Backend and frontend implement in parallel. Integration testing joins them.

---

## Pattern 4: Fork-Join (Phased Parallelism)

```
Phase 1:  A1, A2, A3     (parallel)
          ────────────────
Phase 2:  B1, B2          (parallel, after Phase 1)
          ────────────────
Phase 3:  C1              (after Phase 2)
```

- **Parallelism**: Within each phase, tasks are parallel
- **Risk**: Phase boundaries add synchronization delays
- **Use when**: Natural phases with dependencies (build, test, deploy)
- **Mitigation**: Minimize phase count; keep phases balanced

### rkgw Example: Full-Stack Feature Rollout

```
Phase 1 (Foundation):
  - Backend types/models    (rust-backend-engineer)
  - Frontend API client     (react-frontend-engineer)
  - Docker config updates   (devops-engineer)
  ──────────────────────────────────────────────────
Phase 2 (Implementation):
  - Route handlers + middleware  (rust-backend-engineer)
  - React pages + components     (react-frontend-engineer)
  ──────────────────────────────────────────────────
Phase 3 (Verification):
  - Unit + integration tests     (backend-qa)
  - E2E tests                    (frontend-qa)
```

---

## Pattern 5: Pipeline (Parallel Chains)

```
Task A ──> Task B ──> Task C
  └────> Task D ──> Task E
```

- **Parallelism**: Two independent chains from a common starting point
- **Risk**: Chains may diverge in approach
- **Use when**: Two independent feature branches from a common foundation
- **Mitigation**: Regular sync points between chains

### rkgw Example: Dual-Format Converter

```
Define base converter types
  ├──> OpenAI converter ──> OpenAI streaming tests
  └──> Anthropic converter ──> Anthropic streaming tests
```

Both converter chains share the base types but are otherwise independent.

---

## Anti-Patterns

### Circular Dependency (Deadlock)

```
Task A ──> Task B ──> Task C ──> Task A    DEADLOCK
```

**Fix**: Extract the shared dependency into a separate foundation task.

### Unnecessary Dependencies

```
Task A ──> Task B ──> Task C
(where B doesn't actually need A's output)
```

**Fix**: Remove the dependency. Let B run independently. Common in rkgw when backend and frontend tasks are unnecessarily chained.

### Star Pattern (Single Bottleneck)

```
      ┌──> B
A ──> ├──> C ──> F
      ├──> D
      └──> E
```

**Fix**: If A is slow (e.g., a large refactor of `routes/mod.rs`), all downstream tasks stall. Split A into smaller, independently completable units.

### Over-Sequencing Backend and Frontend

```
Backend types ──> Backend logic ──> Backend tests ──> Frontend API ──> Frontend UI ──> E2E
```

**Fix**: After types are defined, backend logic and frontend UI can proceed in parallel using the agreed API contract. Collapse to a diamond pattern.
