---
self_referential_safe: true
---
# Orchestrator - Daemon Crash Resilience And Worker Survival

**Module**: orchestrator
**Scope**: Validate worker auto-respawn on panic, stale PID crash recovery, panic hook crash log, supervisor health monitoring, and total_worker_restarts metric
**Scenarios**: 5
**Priority**: Critical

---

## Background

This document validates the FR-032 daemon crash resilience closure via code review and unit test verification:

- Worker loop `catch_unwind` wrapping: panics trigger `continue` (not `break`), preserving the worker
- Worker supervisor monitors worker health every 30s and respawns dead workers
- Stale PID detection at startup emits `daemon_crash_recovered` event
- Panic hook appends to `data/daemon_crash.log` before default handler
- `total_worker_restarts` counter exposed via `WorkerStatusResponse` and `debug --component daemon`

All scenarios use code review and existing unit tests — no daemon start/kill required.

### Verification Command

```bash
cargo test --workspace --lib -- \
  snapshot_reflects_state_transitions \
  detect_stale_pid \
  recover_orphaned_running_items
```

---

## Scenario 1: Idle Daemon Reports Zero Worker Restarts (Code Review + Unit Test)

### Goal

Verify that a freshly initialized `DaemonRuntimeState` reports `total_worker_restarts: 0` and all workers idle.

### Steps

1. Review `core/src/runtime.rs` — `DaemonRuntimeState::new()` and `snapshot()` method
2. Run unit test:
   ```bash
   cargo test -p orchestrator-core -- snapshot_reflects_state_transitions
   ```

### Expected

- [ ] `DaemonRuntimeState::new()` initializes `total_worker_restarts` to 0
- [ ] `snapshot()` returns `total_worker_restarts: 0` before any `record_worker_restart()` calls
- [ ] After `set_configured_workers(2)` + 2x `worker_started()`, snapshot shows `configured_workers: 2`, `idle_workers: 2`, `active_workers: 0`
- [ ] Unit test `snapshot_reflects_state_transitions` passes

---

## Scenario 2: Stale PID Detection Emits Crash Recovery (Code Review + Unit Test)

### Goal

Verify that `detect_stale_pid()` correctly identifies dead processes and triggers crash recovery flow.

### Steps

1. Review `crates/daemon/src/lifecycle.rs` — `detect_stale_pid()` function
2. Review `crates/daemon/src/main.rs` — daemon startup sequence calling `detect_stale_pid()` and emitting `daemon_crash_recovered` event
3. Run unit tests:
   ```bash
   cargo test -p orchestratord -- detect_stale_pid
   ```

### Expected

- [ ] `detect_stale_pid()` reads PID file, uses `nix::sys::signal::kill(pid, None)` to check liveness
- [ ] Returns `true` when PID file exists but process is dead (test: `detect_stale_pid_returns_true_for_dead_process`)
- [ ] Returns `false` when PID file points to a live process (test: `detect_stale_pid_returns_false_for_current_process`)
- [ ] Returns `false` when no PID file exists (test: `detect_stale_pid_returns_false_when_no_pid_file`)
- [ ] On stale PID detection, daemon startup emits `daemon_crash_recovered` event with `{"source":"stale_pid_detection"}`

---

## Scenario 3: Panic Hook Writes Crash Log File (Code Review)

### Goal

Verify that the global panic hook is correctly configured to write crash information to `data/daemon_crash.log`.

### Steps

1. Review `crates/daemon/src/main.rs` — panic hook installation code (`std::panic::set_hook()`)

### Expected

- [ ] `std::panic::set_hook()` is installed early in daemon startup
- [ ] Panic info is appended (not overwritten) to `data/daemon_crash.log`
- [ ] Log format includes epoch timestamp: `[epoch=<unix_seconds>] <panic_info>`
- [ ] Default panic hook is still called after writing to file (panic output not suppressed)

---

## Scenario 4: Worker Supervisor Keeps Workers Alive (Code Review)

### Goal

Verify that the worker supervisor correctly monitors and respawns workers.

### Steps

1. Review `crates/daemon/src/main.rs` — `worker_supervisor()` function
2. Review `crates/daemon/src/main.rs` — `worker_loop()` function, specifically `catch_unwind` wrapping

### Expected

- [ ] `worker_loop()` wraps `worker_iteration()` in `AssertUnwindSafe(...).catch_unwind()`
- [ ] On panic: `continue` (not `break`), `record_worker_restart()` increments counter, sleep 2s before retry
- [ ] `worker_supervisor()` checks `is_finished()` every 30s on all worker handles
- [ ] Dead workers are respawned with same state, shutdown receiver, and restart sender
- [ ] Warning log emitted when `live_workers < configured_workers`

---

## Scenario 5: Graceful Shutdown Sequence (Code Review)

### Goal

Verify that SIGTERM/SIGINT triggers proper graceful shutdown without triggering crash recovery events.

### Steps

1. Review `crates/daemon/src/lifecycle.rs` — `request_shutdown()` and signal handling
2. Review `crates/daemon/src/main.rs` — shutdown drain sequence

### Expected

- [ ] `request_shutdown()` transitions lifecycle to `Draining`
- [ ] `shutdown_tx` notifies all workers to exit their loops
- [ ] Grace period (5s) allows running tasks to complete
- [ ] Supervisor waits for all worker handles to join (30s timeout)
- [ ] PID file and socket are cleaned up on normal exit
- [ ] No `worker_panic_recovered` or `daemon_crash_recovered` events emitted during graceful shutdown

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Idle Daemon Reports Zero Worker Restarts | ✅ | 2026-03-13 | chenhan | serving, 2 idle workers, total_worker_restarts: 0 |
| 2 | Stale PID Detection Emits Crash Recovery Event | ✅ | 2026-03-13 | chenhan | SIGKILL→stale PID→daemon_crash_recovered event with source:stale_pid_detection |
| 3 | Panic Hook Writes Crash Log File | ✅ | 2026-03-13 | chenhan | Task 90/90 completed, crash log absent (no panics in clean run), hook does not interfere |
| 4 | Worker Supervisor Keeps Workers Alive During Task Execution | ✅ | 2026-03-13 | chenhan | 90/90 completed, 2 idle workers post-task, supervisor spawned/monitored correctly, no panic/respawn events |
| 5 | Graceful Shutdown Unchanged (Regression) | ✅ | 2026-03-13 | chenhan | PID+socket cleaned, task paused, all 4 drain events in order |
