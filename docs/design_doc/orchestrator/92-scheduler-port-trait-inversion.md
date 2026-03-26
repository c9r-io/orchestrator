# Design Doc 92: Scheduler Port — Trait-Based Inversion for scheduler_service.rs Decomposition

**Module**: orchestrator
**Status**: Implemented
**Related QA**: `docs/qa/orchestrator/102-core-crate-split-scheduler.md`, `docs/qa/orchestrator/78-worker-notify-wakeup.md`
**Created**: 2026-03-26
**Last Updated**: 2026-03-26

## Background

Design Doc 60 extracted the scheduler module (~25K LOC) from core into `crates/orchestrator-scheduler/` using an inverted dependency model. However, `core/src/scheduler_service.rs` (367 LOC) remained in core because `trigger_engine.rs` needed to call `enqueue_task_as_service()`, and core cannot depend on the scheduler crate (would create a cycle).

This left three issues:
1. **`scheduler_service.rs` contained pure scheduling primitives** (`enqueue_task`, `claim_next_pending_task`, `next_pending_task_id`) that belong in the scheduler crate
2. **`enqueue_task_as_service` was a bridge wrapper** existing solely to let `trigger_engine.rs` enqueue tasks
3. **Worker stop helpers** (`worker_stop_signal_path`, `signal_worker_stop`, `clear_worker_stop_signal`) and `pending_task_count` were co-located with scheduling primitives despite being pure infrastructure

## Goals

- Move scheduling primitives to the scheduler crate where they belong
- Break the compile-time coupling between `trigger_engine.rs` and `scheduler_service.rs`
- Delete `scheduler_service.rs` from core's public API surface
- Keep worker stop helpers in core (they are infrastructure, not scheduling)

## Non-goals

- Moving `runner/` or `prehook/` out of core (they are cross-cutting; see Design Doc 60)
- Changing runtime semantics of enqueue, claim, or worker stop behavior

## Key Design

### 1. TaskEnqueuer Trait Port (Hexagonal Architecture)

A new `core/src/scheduler_port.rs` defines:

```rust
#[async_trait]
pub trait TaskEnqueuer: Send + Sync {
    async fn enqueue_task(&self, state: &InnerState, task_id: &str) -> Result<()>;
}
```

`InnerState` gains a `task_enqueuer: Arc<dyn TaskEnqueuer>` field.

### 2. Scheduler Provides the Concrete Implementation

`crates/orchestrator-scheduler/src/service/task.rs` provides `SchedulerTaskEnqueuer` implementing the trait. The canonical `enqueue_task_inner()` function lives alongside it.

### 3. Daemon Wires at Startup

`init_state_async_with_enqueuer(false, Arc::new(SchedulerTaskEnqueuer))` injects the real implementation. Tests and CLI use `noop_task_enqueuer()`.

### 4. Worker Helpers Stay in Core

`pending_task_count`, `worker_stop_signal_path`, `clear_worker_stop_signal`, `signal_worker_stop` moved to `core/src/service/system.rs` — they only depend on `InnerState` and filesystem operations, not scheduling logic.

## Alternatives and Tradeoffs

| Option | Pros | Cons | Decision |
|--------|------|------|----------|
| Keep `scheduler_service.rs` in core | No changes needed | Boundary remains unclear; scheduling primitives leak into core | Rejected |
| Channel/message-passing | No trait, no dyn dispatch | Over-engineered for a single call site; async channel adds complexity | Rejected |
| **Trait-based port inversion** | Clean boundary; one `dyn` dispatch only on trigger path; zero-cost for direct scheduler calls | Adds one trait + one field to `InnerState` | **Chosen** |

## Risks and Mitigations

- **Risk**: `dyn` dispatch overhead on trigger enqueue path
  - **Mitigation**: Only `trigger_engine.rs` uses the trait; the scheduler crate's own `enqueue_task()` calls `enqueue_task_inner()` directly (zero-cost)
- **Risk**: Test contexts silently swallow enqueue calls via `NoopTaskEnqueuer`
  - **Mitigation**: Tests that need real enqueue behavior use the test helper `enqueue_task_for_test()` which inlines the logic

## Decomposition Map

| Function | Old Location | New Location |
|----------|-------------|--------------|
| `enqueue_task` | `core/src/scheduler_service.rs` | `crates/orchestrator-scheduler/src/service/task.rs` |
| `claim_next_pending_task` | `core/src/scheduler_service.rs` | `crates/orchestrator-scheduler/src/service/task.rs` |
| `next_pending_task_id` | `core/src/scheduler_service.rs` | `crates/orchestrator-scheduler/src/service/task.rs` |
| `enqueue_task_as_service` | `core/src/scheduler_service.rs` | Deleted (replaced by `TaskEnqueuer` trait) |
| `pending_task_count` | `core/src/scheduler_service.rs` | `core/src/service/system.rs` |
| `worker_stop_signal_path` | `core/src/scheduler_service.rs` | `core/src/service/system.rs` |
| `clear_worker_stop_signal` | `core/src/scheduler_service.rs` | `core/src/service/system.rs` |
| `signal_worker_stop` | `core/src/scheduler_service.rs` | `core/src/service/system.rs` |

## Observability

No new logs, metrics, or tracing spans. The `scheduler_enqueued` event emission is preserved in `enqueue_task_inner()`.

## Operations / Release

- No migration required — pure code restructuring
- No configuration changes
- Backward compatible — all runtime behavior preserved

## Test Plan

- Unit tests: `cargo test -p agent-orchestrator` (core tests including worker stop helpers)
- Unit tests: `cargo test -p orchestrator-scheduler` (scheduler tests including enqueue/claim)
- Unit tests: `cargo test -p orchestratord` (daemon tests)
- Workspace: `cargo clippy --workspace` — zero warnings

## QA Docs

- `docs/qa/orchestrator/102-core-crate-split-scheduler.md` — updated S-04 and S-05
- `docs/qa/orchestrator/78-worker-notify-wakeup.md` — updated test paths

## Acceptance Criteria

- `core/src/scheduler_service.rs` deleted
- `scheduler_service` absent from core's `lib.rs` public API
- `trigger_engine.rs` uses `state.task_enqueuer.enqueue_task()`
- Daemon injects `SchedulerTaskEnqueuer` via `init_state_async_with_enqueuer`
- All workspace tests pass; zero clippy warnings
