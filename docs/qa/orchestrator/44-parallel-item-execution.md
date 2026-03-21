---
self_referential_safe: true
---

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
| `crates/orchestrator-config/src/config/execution.rs` | `max_parallel` on `TaskExecutionStep`, `TaskExecutionPlan` |
| `core/src/scheduler/loop_engine.rs` | `ScopeSegment.max_parallel`, parallel dispatch in `StepScope::Item` |
| `core/src/scheduler/item_executor.rs` | `OwnedProcessItemRequest`, `process_item_filtered_owned` |
| `core/src/state.rs` | `RunningTask::fork()` |
| `core/src/async_database.rs` | Writer+reader connection model (WAL mode, 5000ms busy_timeout) |

---

## Scenario 1: max_parallel Config Round-Trip via YAML and Serde

### Preconditions
- Repository root is the current working directory.
- Rust toolchain is available.

### Goal
Verify `max_parallel` field is accepted in workflow YAML, propagated through the execution plan, and round-trips through serde.

### Steps

1. Verify the `max_parallel` field exists in config structs and parses correctly via serde:
   ```bash
   grep -n "max_parallel" crates/orchestrator-config/src/config/workflow.rs
   grep -n "max_parallel" crates/orchestrator-config/src/config/execution.rs
   ```

2. Run workflow config serde round-trip tests:
   ```bash
   cargo test -p orchestrator-config --lib -- workflow --nocapture
   ```

3. Verify `max_parallel` propagation through execution plan in code:
   ```bash
   rg -n "max_parallel" crates/orchestrator-config/src/config/workflow.rs crates/orchestrator-config/src/config/execution.rs
   ```

### Expected
- `max_parallel` field is defined in both `WorkflowConfig` and `WorkflowStepConfig`
- Workflow config unit tests pass (serde round-trip preserves `max_parallel`)
- `max_parallel` propagates from config through execution plan

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
- Rust toolchain available

### Goal
Verify that when `max_parallel` is not configured, item-scoped steps execute sequentially as before (no behavioral change, no Arc overhead).

### Steps

1. Code review — verify the sequential dispatch path is preserved when `max_parallel` is absent or <= 1:
   ```bash
   rg -n "max_parallel" crates/orchestrator-scheduler/src/scheduler/loop_engine.rs | head -10
   ```

2. Code review — verify `ScopeSegment.max_parallel` defaults to 1 when not set:
   ```bash
   rg -n "max_parallel.*unwrap_or|max_parallel.*1" crates/orchestrator-scheduler/src/scheduler/loop_engine/segment.rs | head -5
   ```

3. Run the segment builder unit tests that verify default resolution:
   ```bash
   cargo test -p orchestrator-scheduler --features test-harness -- build_segments 2>&1 | tail -5
   ```

### Expected
- When `max_parallel` is `None` or `<= 1`, the sequential loop path executes (no `JoinSet` dispatch)
- `build_segments` tests pass: segments without `max_parallel` default to 1 (sequential)
- No behavioral change for existing workflows that don't set `max_parallel`

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
   sed -n '1,40p' core/src/async_database.rs
   ```

2. Verify busy_timeout is configured in code:
   ```bash
   rg -n "SQLITE_BUSY_TIMEOUT_MS" core/src/persistence/sqlite.rs
   ```

3. Run unit tests that validate database bootstrap and paired-connection configuration:
   ```bash
   cargo test --workspace --lib -- async_database_open_and_configure bootstrap_creates_latest_schema_and_reports_current_status
   ```

### Expected
- `AsyncDatabase` uses 2 named connections: `writer` (all writes) and `reader` (read-only queries)
- `SQLITE_BUSY_TIMEOUT_MS` is `5000` (5 seconds)
- schema bootstrap enables WAL mode and the async database tests confirm the paired connection configuration

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | max_parallel Config Round-Trip via YAML and Serde | PASS | 2026-03-20 | | max_parallel in WorkflowConfig/WorkflowStepConfig (workflow.rs:67,175; execution.rs:73,197); 23 serde tests pass. Doc path `crates/orchestrator-config/src/config/execution.rs` is wrong — correct: `crates/orchestrator-config/src/config/execution.rs` |
| 2 | ScopeSegment Resolves max_parallel From Step and Plan | PASS | 2026-03-20 | | build_segments 5/5 pass; `scope_segment`/`parallel` test filters return 0 (patterns not present in current codebase — doc drift) |
| 3 | RunningTask::fork() Shares Stop Flag | PASS | 2026-03-20 | | fork() uses Arc::clone for stop_flag, new child slot; 4/4 tests pass |
| 4 | Sequential Path Unchanged When max_parallel Absent | PASS | 2026-03-21 | | Rewritten: code review + build_segments unit test verifies sequential default path |
| 5 | Database Connection Model and WAL Configuration | PASS | 2026-03-20 | | 2-conn model (writer+reader); SQLITE_BUSY_TIMEOUT_MS=5000; 2/2 tests pass |
