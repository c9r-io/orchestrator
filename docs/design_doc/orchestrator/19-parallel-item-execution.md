# Parallel Item-Scoped Step Execution

**Module**: orchestrator
**Status**: Implemented
**Related QA**: `docs/qa/orchestrator/44-parallel-item-execution.md`

---

## Background and Goals

QA test documents are project-scoped (isolated `root_path`, `ticket_dir`, agents). Items within an item-scoped segment are independent — no cross-item data flow. Yet execution was strictly sequential (`for item in &items { ... .await?; }`).

**Goals**:
- Add configurable parallelism for item-scoped segments to reduce wall-clock time
- Preserve the exact sequential path when parallelism is disabled (zero overhead)
- Share stop-flag semantics so `task stop` halts all parallel items

**Non-goals**:
- Task-scoped parallelism (steps like plan/implement are inherently serial)
- Automatic concurrency tuning — the user explicitly sets `max_parallel`

## Scope

### In Scope

| Area | Detail |
|------|--------|
| Config schema | `max_parallel` on `WorkflowConfig`, `WorkflowStepConfig`, `TaskExecutionStep`, `TaskExecutionPlan`, `WorkflowSpec`, `WorkflowStepSpec` |
| Config propagation | step override > plan default > 1 |
| Execution engine | Semaphore-gated `JoinSet` dispatch in `StepScope::Item` branch |
| RunningTask | `fork()` method sharing atomic stop_flag |
| Owned request wrapper | `OwnedProcessItemRequest` delegating to existing `process_item_filtered` |
| Database pool | Increased from 12 → 20 connections |
| YAML spec layer | `WorkflowSpec` and `WorkflowStepSpec` accept `max_parallel` |

### Out of Scope

- Dynamic auto-scaling based on system load
- Per-item timeout (existing step_timeout_secs applies uniformly)
- Parallel task-scoped segments

## Key Design Decisions

### 1. Zero-Change Sequential Path

When `max_parallel` is absent or 1, the existing `for` loop runs unchanged. No Arc wrapping, no semaphore, no JoinSet. This eliminates overhead for the common case and preserves exact behavior.

### 2. Owned Wrapper Instead of Refactor

`OwnedProcessItemRequest` wraps borrowed fields into `Arc`/owned types for `tokio::spawn` (requires `'static`), then borrows back to call the existing `process_item_filtered`. This adds zero changes to the 600+ line unified step loop.

### 3. Semaphore-Gated Concurrency

`tokio::Semaphore` controls how many items run simultaneously. Items acquire a permit before spawning. This prevents unbounded resource consumption.

### 4. Collect-All-Errors

All items complete even if some fail. Errors are aggregated post-join and reported as a combined message. This prevents one failed QA doc from hiding results of others.

### 5. Shared Stop Flag via fork()

`RunningTask::fork()` creates a sibling with the same `stop_flag` (Arc<AtomicBool>) but a separate `child` process slot. When `stop_task_runtime` sets the flag, all parallel items observe it on their next step boundary.

## Interfaces and Data Changes

### Config YAML

```yaml
# Workflow-level default
max_parallel: 4

steps:
  - id: qa_testing
    scope: item
    # inherits max_parallel: 4
  - id: ticket_fix
    scope: item
    max_parallel: 2  # per-step override
```

### Resolution Order

```
step.max_parallel  >  execution_plan.max_parallel  >  1 (sequential)
```

### Database

No schema changes. Pool size increased: 12 → 20 connections. WAL mode + busy_timeout=5s handles concurrent writes.

## Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| SQLite write contention | WAL mode serializes writes; busy_timeout=5s retries; writes are short single-INSERT operations |
| Agent process leaks on panic | `JoinSet` awaits all futures; forked `RunningTask` drops clean up child processes |
| Memory pressure with many items | Semaphore caps concurrent items; pool size 20 provides headroom |
| Stop-flag race | Atomic bool with SeqCst ordering; checked at step boundaries inside `process_item_filtered` |

## Observability

- Existing `step_started` / `step_finished` events are emitted per-item (unchanged)
- Parallel items have overlapping timestamps in the events table — `task trace` shows this
- Error aggregation message includes all failed item errors separated by `;`

## Testing and Acceptance

- **Unit tests**: `build_scope_segments` resolves `max_parallel` from step/plan/default
- **Unit tests**: `RunningTask::fork()` shares stop flag, independent child slots
- **Unit tests**: Config round-trip with `max_parallel` field
- **QA doc**: `docs/qa/orchestrator/44-parallel-item-execution.md`

## Key Files

| File | Change |
|------|--------|
| `core/src/config/workflow.rs` | `max_parallel` on `WorkflowConfig`, `WorkflowStepConfig` |
| `core/src/config/execution.rs` | `max_parallel` on `TaskExecutionStep`, `TaskExecutionPlan` |
| `core/src/cli_types.rs` | `max_parallel` on `WorkflowSpec`, `WorkflowStepSpec` |
| `core/src/config_load/build.rs` | Propagation through plan building |
| `core/src/state.rs` | `RunningTask::fork()` |
| `core/src/scheduler/item_executor.rs` | `OwnedProcessItemRequest`, `process_item_filtered_owned` |
| `core/src/scheduler/loop_engine.rs` | `ScopeSegment.max_parallel`, parallel dispatch branch |
| `core/src/database.rs` | Pool size 12 → 20 |
