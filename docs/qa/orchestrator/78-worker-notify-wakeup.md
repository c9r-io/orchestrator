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
1. Inspect `core/src/scheduler_service.rs`
2. Confirm `enqueue_task()` updates task status to `pending`
3. Confirm `enqueue_task()` calls `state.worker_notify.notify_waiters()`

**Expected**:
- Wakeup is in-memory and immediate
- No wake file is created as part of enqueue

### S-03: Stop path still works without wake-file coupling

**Steps**:
1. Code review confirms unit tests exist in `core/src/scheduler_service.rs`:
   - `signal_worker_stop_creates_stop_file`
   - `signal_worker_stop_notifies_waiters`
2. Run tests (safe: uses isolated temp state):
   ```bash
   cargo test --lib -p agent-orchestrator -- scheduler_service::tests::signal_worker_stop_creates_stop_file
   cargo test --lib -p agent-orchestrator -- scheduler_service::tests::signal_worker_stop_notifies_waiters
   ```

**Expected**:
- `signal_worker_stop()` still writes `data/worker.stop`
- idle waiters are woken through `Notify`

### S-04: Single-winner claim semantics remain intact

**Steps**:
1. Code review confirms unit test exists in `core/src/scheduler_service.rs`:
   - `claim_next_pending_task_is_single_winner`
2. Run test (safe: uses isolated temp-db):
   ```bash
   cargo test --lib -p agent-orchestrator -- scheduler_service::tests::claim_next_pending_task_is_single_winner
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

Verified on 2026-03-12:

- wake-file symbols removed from `core/` and `crates/`
- `scheduler_service::tests::signal_worker_stop_notifies_waiters`: passed
- `cargo test --workspace`: passed
- `cargo clippy --workspace --all-targets -- -D warnings`: passed

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☑ | S1-S5 PASS (2026-03-19); S3-S5 rewritten as safe (cargo test --lib + CI gate) |
