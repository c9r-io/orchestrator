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
| `core/src/database.rs` | Pool size 20 |

---

## Scenario 1: max_parallel Config Round-Trip via YAML and Serde

### Preconditions
- Orchestrator binary built

### Goal
Verify `max_parallel` field is accepted in workflow YAML, propagated through the execution plan, and round-trips through serde.

### Steps

1. Run the config serde unit tests:
   ```bash
   cd core && cargo test max_parallel -- --nocapture 2>&1
   ```

2. Verify the self-bootstrap YAML parses without errors:
   ```bash
   ./scripts/run-cli.sh apply -f fixtures/manifests/bundles/self-bootstrap-mock.yaml --dry-run 2>&1
   ```

3. Check that the workflow-level `max_parallel: 4` and step-level `max_parallel: 2` (ticket_fix) are present in the dry-run output:
   ```bash
   ./scripts/run-cli.sh apply -f fixtures/manifests/bundles/self-bootstrap-mock.yaml --dry-run 2>&1 | grep -i "max_parallel" || echo "field not in dry-run text output"
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
   cd core && cargo test build_segments -- --nocapture 2>&1
   ```

2. Verify segment resolution logic in tests:
   ```bash
   cd core && cargo test scope_segment -- --nocapture 2>&1
   cd core && cargo test parallel -- --nocapture 2>&1
   ```

### Expected
- `build_segments_groups_contiguous_scopes` passes — segments carry `max_parallel` field
- For `StepScope::Item` segments: `max_parallel = step.max_parallel.or(plan.max_parallel).unwrap_or(1)`
- For `StepScope::Task` segments: `max_parallel` is always 1
- Guard steps are excluded from segments

### Expected Data State

```bash
cd core && cargo test build_segments 2>&1 | grep "test result"
# Expected: test result: ok. N passed; 0 failed
```

---

## Scenario 3: RunningTask::fork() Shares Stop Flag

### Preconditions
- Source code available

### Goal
Verify `RunningTask::fork()` creates a sibling that shares the `stop_flag` but has an independent `child` slot.

### Steps

1. Run the fork unit tests:
   ```bash
   cd core && cargo test running_task -- --nocapture 2>&1
   cd core && cargo test fork -- --nocapture 2>&1
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
cd core && cargo test running_task fork 2>&1 | grep "test result"
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
   ./scripts/run-cli.sh apply -f fixtures/manifests/bundles/echo-workflow.yaml
   ./scripts/run-cli.sh qa project reset "${QA_PROJECT}" --keep-config --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   ./scripts/run-cli.sh qa project create "${QA_PROJECT}" --force
   ```

2. Create and run a task with multiple QA items:
   ```bash
   TASK_ID=$(./scripts/run-cli.sh task create \
     --name "par-seq-test" \
     --goal "Verify sequential item execution" \
     --project "${QA_PROJECT}" \
     --workflow qa_only \
     --json 2>/dev/null | jq -r '.task_id // .id')
   ./scripts/run-cli.sh task start "${TASK_ID}"
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

## Scenario 5: Database Pool Size Increased to 20

### Preconditions
- Source code available

### Goal
Verify the connection pool was increased from 12 to 20 to accommodate parallel item execution.

### Steps

1. Check the constant in source:
   ```bash
   grep "DEFAULT_POOL_MAX_SIZE" core/src/database.rs
   ```

2. Run the pool configuration unit test:
   ```bash
   cd core && cargo test pool -- --nocapture 2>&1
   ```

3. Verify WAL mode is enabled:
   ```bash
   sqlite3 data/agent_orchestrator.db "PRAGMA journal_mode;"
   ```

### Expected
- `DEFAULT_POOL_MAX_SIZE` is `20`
- Pool unit test asserts `pool_max_size() == 20`
- WAL mode is `wal`
- busy_timeout is configured (5000ms)

### Expected Data State

```bash
cd core && cargo test pool 2>&1 | grep "test result"
# Expected: test result: ok. N passed; 0 failed
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | max_parallel Config Round-Trip via YAML and Serde | ☐ | | | |
| 2 | ScopeSegment Resolves max_parallel From Step and Plan | ☐ | | | |
| 3 | RunningTask::fork() Shares Stop Flag | ☐ | | | |
| 4 | Sequential Path Unchanged When max_parallel Absent | ☐ | | | |
| 5 | Database Pool Size Increased to 20 | ☐ | | | |
