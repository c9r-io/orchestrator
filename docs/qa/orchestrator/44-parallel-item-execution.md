# Orchestrator - Parallel Item-Scoped Step Execution

**Module**: orchestrator
**Scope**: Validate configurable parallel execution of item-scoped segments via `max_parallel`
**Scenarios**: 5
**Priority**: High

---

## Background

Item-scoped steps (qa_testing, ticket_fix) previously ran sequentially even though items are independent. The `max_parallel` config field now controls concurrency:

- **Workflow-level**: `WorkflowConfig.max_parallel` — default for all item segments
- **Step-level**: `WorkflowStepConfig.max_parallel` — per-step override
- **Resolution**: step override > plan default > 1 (sequential)

When `max_parallel <= 1`, the existing sequential loop runs unchanged. When `> 1`, a semaphore-gated `JoinSet` dispatches items concurrently.

**Design doc**: `docs/design_doc/orchestrator/19-parallel-item-execution.md`

### Key Files

| File | Role |
|------|------|
| `core/src/config/workflow.rs` | `max_parallel` on `WorkflowConfig`, `WorkflowStepConfig` |
| `core/src/config/execution.rs` | `max_parallel` on `TaskExecutionStep`, `TaskExecutionPlan` |
| `core/src/scheduler/loop_engine.rs` | `ScopeSegment.max_parallel`, parallel dispatch in `StepScope::Item` |
| `core/src/scheduler/item_executor.rs` | `OwnedProcessItemRequest`, `process_item_filtered_owned` |
| `core/src/state.rs` | `RunningTask::fork()` |
| `core/src/async_database.rs` | Writer+reader connection model (WAL mode, 5000ms busy_timeout) |

---

## Scenario 1: max_parallel Config Round-Trip via YAML and Serde

### Preconditions
- Orchestrator binary built

### Goal
Verify `max_parallel` field is accepted in workflow YAML, propagated through the execution plan, and round-trips through serde.

### Steps

1. Verify the `max_parallel` field exists in config structs and parses correctly via serde:
   ```bash
   grep -n "max_parallel" crates/orchestrator-config/src/config/workflow.rs
   grep -n "max_parallel" core/src/config/execution.rs
   ```

2. Verify the self-bootstrap YAML parses without errors:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/self-bootstrap-mock.yaml --dry-run 2>&1
   ```

3. Check that the workflow-level `max_parallel: 4` and step-level `max_parallel: 2` (ticket_fix) are present in the dry-run output:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/self-bootstrap-mock.yaml --dry-run 2>&1 | grep -i "max_parallel" || echo "field not in dry-run text output"
   ```

### Expected
- All `max_parallel` unit tests pass
- `--dry-run` output shows no validation errors for `max_parallel` field
- YAML with `max_parallel` at both workflow-level and step-level is accepted

---

## Scenario 2: ScopeSegment Resolves max_parallel From Step and Plan

### Preconditions
- Source code available

### Goal
Verify `build_scope_segments()` resolves `max_parallel` correctly: step override > plan default > 1.

### Steps

1. Run the segment builder unit tests:
   ```bash
   cargo test -p orchestrator-scheduler --features test-harness -- build_segments --nocapture 2>&1
   ```

2. Verify segment resolution logic in tests:
   ```bash
   cargo test -p orchestrator-scheduler --features test-harness -- scope_segment --nocapture 2>&1
   cargo test -p orchestrator-scheduler --features test-harness -- parallel --nocapture 2>&1
   ```

### Expected
- `build_segments_groups_contiguous_scopes` passes — segments carry `max_parallel` field
- For `StepScope::Item` segments: `max_parallel = step.max_parallel.or(plan.max_parallel).unwrap_or(1)`
- For `StepScope::Task` segments: `max_parallel` is always 1
- Guard steps are excluded from segments

### Expected Data State

```bash
cargo test -p orchestrator-scheduler --features test-harness -- build_segments 2>&1 | grep "test result"
# Expected: test result: ok. N passed; 0 failed
```

---

## Scenario 3: RunningTask::fork() Shares Stop Flag

### Preconditions
- Source code available

### Goal
Verify `RunningTask::fork()` creates a sibling that shares the `stop_flag` but has an independent `child` slot.

### Steps

1. Run the running_task unit tests (includes fork verification):
   ```bash
   cargo test -p agent-orchestrator --lib -- running_task --nocapture 2>&1
   ```

2. Manually verify the fork semantics in code:
   ```bash
   grep -A 10 "pub fn fork" core/src/state.rs
   ```

### Expected
- `fork()` returns a `RunningTask` where:
  - `stop_flag` is `Arc::clone` of the original (same atomic bool)
  - `child` is a new `Arc<Mutex<Option<Child>>>` (independent)
- Setting `stop_flag` on original or fork is visible to both
- Each fork's child process slot is independent

### Expected Data State

```bash
cargo test -p agent-orchestrator --lib -- running_task 2>&1 | grep "test result"
# Expected: test result: ok. N passed; 0 failed
```

---

## Scenario 4: Sequential Path Unchanged When max_parallel Absent

### Preconditions
- Orchestrator binary built
- A workflow without `max_parallel` set (defaults to None → sequential)

### Goal
Verify that when `max_parallel` is not configured, item-scoped steps execute sequentially as before (no behavioral change, no Arc overhead).

### Steps

1. Apply the echo-workflow fixture (no `max_parallel`):
   ```bash
   rm -f fixtures/ticket/auto_*.md
   QA_PROJECT="qa-par-seq-${USER}-$(date +%Y%m%d%H%M%S)"
   orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml
   orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project "${QA_PROJECT}"
   ```

2. Create and run a task with multiple QA items:
   ```bash
   TASK_ID=$(orchestrator task create \
     --name "par-seq-test" \
     --goal "Verify sequential item execution" \
     --project "${QA_PROJECT}" \
     --workflow qa_only \
     --json 2>/dev/null | jq -r '.task_id // .id')
   orchestrator task start "${TASK_ID}"
   sleep 10
   ```

3. Verify sequential execution order (events are strictly ordered, no overlapping timestamps):
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT e.task_item_id, e.created_at,
             json_extract(e.payload_json, '$.step') AS step,
             e.event_type
      FROM events e
      WHERE e.task_id = '${TASK_ID}'
        AND e.event_type IN ('step_started', 'step_finished')
        AND json_extract(e.payload_json, '$.step_scope') = 'item'
      ORDER BY e.created_at"
   ```

### Expected
- Item events are strictly sequential: item1 step_started → item1 step_finished → item2 step_started → ...
- No overlapping step_started timestamps for different items in the same segment
- Task completes successfully

---

## Scenario 5: Database Connection Model and WAL Configuration

### Preconditions
- Source code available

### Goal
Verify the database uses a writer+reader connection model with WAL mode and busy_timeout configured for parallel item execution.

> **Note**: The design doc originally proposed a 20-connection pool, but the implementation uses a 2-connection model (one dedicated writer, one dedicated reader). This matches SQLite's WAL single-writer constraint and avoids pool contention. Parallel item execution is supported because items serialize writes through the single writer connection.

### Steps

1. Verify the connection model in source:
   ```bash
   grep -A5 "pub struct AsyncDatabase" core/src/async_database.rs
   ```

2. Verify busy_timeout is configured:
   ```bash
   grep "SQLITE_BUSY_TIMEOUT_MS" core/src/persistence/sqlite.rs
   ```

3. Verify WAL mode is enabled:
   ```bash
   sqlite3 data/agent_orchestrator.db "PRAGMA journal_mode;"
   ```

### Expected
- `AsyncDatabase` uses 2 named connections: `writer` (all writes) and `reader` (read-only queries)
- `SQLITE_BUSY_TIMEOUT_MS` is `5000` (5 seconds)
- WAL mode is `wal`

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | max_parallel Config Round-Trip via YAML and Serde | PASS | 2026-03-15 | claude | Unit tests pass |
| 2 | ScopeSegment Resolves max_parallel From Step and Plan | PASS | 2026-03-15 | claude | 5 build_segments tests pass (orchestrator-scheduler --features test-harness) |
| 3 | RunningTask::fork() Shares Stop Flag | PASS | 2026-03-15 | claude | running_task tests pass; fork method verified in code |
| 4 | Sequential Path Unchanged When max_parallel Absent | ☐ | | | Requires live task execution |
| 5 | Database Connection Model and WAL Configuration | PASS | 2026-03-15 | claude | Writer+reader model, WAL enabled, busy_timeout 5000ms |
