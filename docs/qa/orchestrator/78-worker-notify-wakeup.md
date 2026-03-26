---
self_referential_safe: true
---

# QA #78: Worker Notify Wakeup Governance (FR-027)

## Scope

Verify that daemon worker wakeup no longer depends on `worker.wakeup` filesystem polling, that enqueue/stop paths notify idle workers in-process, and that scheduler correctness remains unchanged under concurrent workers.

## Scenarios

### S-01: Wake-file code is fully removed

**Steps**:
1. Run `rg "worker\\.wakeup|touch_worker_wake_signal|wait_for_wake_signal|worker_wake_signal_path" core crates`

**Expected**:
- No production-code matches are returned

### S-02: Enqueue path wakes workers through `Notify`

**Steps**:
1. Inspect `crates/orchestrator-scheduler/src/service/task.rs` — function `enqueue_task_inner()`
2. Confirm it updates task status to `pending`
3. Confirm it calls `state.worker_notify.notify_waiters()`
4. Confirm `core/src/trigger_engine.rs` calls `state.task_enqueuer.enqueue_task()` (trait dispatch to scheduler)

**Expected**:
- Wakeup is in-memory and immediate
- No wake file is created as part of enqueue
- `trigger_engine` uses the `TaskEnqueuer` trait port, not a direct function call

### S-03: Stop path still works without wake-file coupling

**Steps**:
1. Code review confirms unit tests exist in `core/src/service/system.rs`:
   - `signal_worker_stop_creates_stop_file`
   - `clear_worker_stop_signal_removes_stop_file`
   - `clear_worker_stop_signal_noop_when_no_file`
   - `worker_signal_paths_are_under_data_dir`
2. Run tests (safe: uses isolated temp state):
   ```bash
   cargo test --lib -p agent-orchestrator -- service::system::tests::signal_worker_stop_creates_stop_file
   cargo test --lib -p agent-orchestrator -- service::system::tests::clear_worker_stop_signal_removes_stop_file
   ```

**Expected**:
- `signal_worker_stop()` still writes `data/worker.stop`
- idle waiters are woken through `Notify`

### S-04: Single-winner claim semantics remain intact

**Steps**:
1. Code review confirms `claim_next_pending_task()` exists in `crates/orchestrator-scheduler/src/service/task.rs`
2. Run claim tests (safe: uses isolated temp-db):
   ```bash
   cargo test -p orchestrator-scheduler -- service::task::tests::
   ```

**Expected**:
- only one concurrent claimer wins the pending task
- no duplicate execution path is introduced by waking all idle workers

### S-05: Workspace regression gates stay green

**Steps**:
1. Run `cargo test --workspace --lib` (safe: does not affect running daemon)
2. Code review confirms `.github/workflows/ci.yml` contains clippy job with `-D warnings`

**Expected**:
- all lib tests pass
- CI gate enforces clippy compliance

## Result

Verified on 2026-03-12 (original), updated 2026-03-26 after scheduler_service.rs decomposition:

- wake-file symbols removed from `core/` and `crates/`
- worker stop helpers moved from `core/src/scheduler_service.rs` to `core/src/service/system.rs`
- enqueue/claim/next moved to `crates/orchestrator-scheduler/src/service/task.rs`
- `trigger_engine` now uses `TaskEnqueuer` trait port (`core/src/scheduler_port.rs`)
- `cargo test --workspace`: passed
- `cargo clippy --workspace`: passed

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☑ | S1-S5 PASS (2026-03-26); paths updated after scheduler_service.rs decomposition |
