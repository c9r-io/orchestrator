---
self_referential_safe: false
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
1. Inspect `core/src/scheduler_service.rs`
2. Confirm `enqueue_task()` updates task status to `pending`
3. Confirm `enqueue_task()` calls `state.worker_notify.notify_waiters()`

**Expected**:
- Wakeup is in-memory and immediate
- No wake file is created as part of enqueue

### S-03: Stop path still works without wake-file coupling

**Steps**:
1. Run `cargo test --workspace scheduler_service::tests::signal_worker_stop_creates_stop_file scheduler_service::tests::signal_worker_stop_notifies_waiters`

**Expected**:
- `signal_worker_stop()` still writes `data/worker.stop`
- idle waiters are woken through `Notify`

### S-04: Single-winner claim semantics remain intact

**Steps**:
1. Run `cargo test --workspace scheduler_service::tests::claim_next_pending_task_is_single_winner`

**Expected**:
- only one concurrent claimer wins the pending task
- no duplicate execution path is introduced by waking all idle workers

### S-05: Workspace regression gates stay green

**Steps**:
1. Run `cargo test --workspace`
2. Run `cargo clippy --workspace --all-targets -- -D warnings`

**Expected**:
- all tests pass
- lint gate passes with `-D warnings`

## Result

Verified on 2026-03-12:

- wake-file symbols removed from `core/` and `crates/`
- `scheduler_service::tests::signal_worker_stop_notifies_waiters`: passed
- `cargo test --workspace`: passed
- `cargo clippy --workspace --all-targets -- -D warnings`: passed

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
