# Design Doc #39: Worker Notify Wakeup Governance (FR-027)

## Status

Implemented

## Context

`orchestratord` worker idle waiting previously combined two polling layers:

- daemon worker loop slept for up to 2 seconds before retrying `claim_next_pending_task()`
- `wait_for_wake_signal()` polled `data/worker.wakeup` every 200ms

That design introduced avoidable wake latency, repeated filesystem `stat()` calls during idle periods, and a fragile file-based signal path inside a process that already shares memory.

FR-027 required the daemon to move worker wakeup to an in-process `tokio::sync::Notify` mechanism while preserving:

- existing SQLite-backed atomic claim semantics
- the shutdown watch channel
- a bounded timeout fallback in case a wake signal is missed

## Decision

Add `worker_notify: Arc<Notify>` to shared runtime state and route all in-process worker wakeups through it.

### Shared State

`core/src/state.rs` now exposes `InnerState::worker_notify`, initialized by all state constructors:

- bootstrap path
- test fixtures
- scheduler runtime helper constructors

This keeps wakeup orchestration in the same shared state object already used by services and the daemon worker loop.

### Enqueue Path

`crates/orchestrator-scheduler/src/service/task.rs::enqueue_task_inner()` (moved from `core/src/scheduler_service.rs` in Design Doc 92):

1. marks the task `pending`
2. calls `state.worker_notify.notify_waiters()`
3. emits `scheduler_enqueued`

`core/src/trigger_engine.rs` reaches this via `state.task_enqueuer.enqueue_task()` (the `TaskEnqueuer` trait port defined in `core/src/scheduler_port.rs`).

The wakeup side effect is in-memory and immediate. No `worker.wakeup` file is created.

### Stop Path

`signal_worker_stop()` still writes `data/worker.stop` for explicit stop-file compatibility, but it now wakes idle workers with `notify_waiters()` instead of touching a second marker file.

This preserves existing shutdown semantics while removing the old wake-file coupling.

### Worker Loop

`crates/daemon/src/main.rs::worker_loop()` now waits on:

- `state.worker_notify.notified()` for immediate wakeup
- `tokio::time::sleep(2s)` as a safety-net fallback
- `shutdown.changed()` for daemon shutdown

The dedicated `wait_for_wake_signal()` helper and all `worker.wakeup` path handling were removed.

## Trade-offs

1. `Notify` over file signals: lower latency and zero idle filesystem polling, but only valid for in-process wakeups. This matches the current daemon architecture.
2. Keep `worker.stop` file: stop signaling remains externally observable and backward-compatible, even though wake signaling moved in-process.
3. Retain 2-second fallback sleep: slightly redundant under healthy operation, but it protects against missed registration races and any future code path that changes task state without issuing a notify.

## Acceptance Mapping

- No worker filesystem wake polling: `worker.wakeup`, `touch_worker_wake_signal()`, `worker_wake_signal_path()`, and `wait_for_wake_signal()` are removed from production code.
- Immediate worker wake on enqueue: `enqueue_task()` now issues `notify_waiters()`.
- Multi-worker safety: existing `claim_next_pending_task()` atomic claim path is unchanged.
- Shutdown compatibility: `shutdown.changed()` remains in the worker loop; `signal_worker_stop()` still writes `worker.stop`.

## Verification

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `rg "worker\\.wakeup|touch_worker_wake_signal|wait_for_wake_signal|worker_wake_signal_path" core crates`

All checks passed on 2026-03-12.
