# Design Doc 47: Daemon 进程崩溃韧性与 Worker 存活保障 (FR-032)

## Problem

During a self-bootstrap test (2026-03-13), the daemon process silently exited
during Cycle 2's `qa_testing` phase, orphaning two running agent processes and
leaving their task items permanently stuck in `running` state. Root causes
included: worker panic causing permanent slot loss (`break` after
`catch_unwind`), no worker respawn mechanism, no crash logging, and no crash
recovery detection on restart.

## Design Decision

**Multi-layered crash resilience with worker supervisor, panic recovery, and
crash detection.**

### 1. Worker Supervisor (`crates/daemon/src/main.rs`)

A dedicated `worker_supervisor()` async function manages the lifecycle of all
worker tasks:

- Spawns `worker_count` workers via `tokio::spawn(worker_loop(...))`
- Maintains a `Vec<(usize, JoinHandle<()>)>` of worker handles
- Runs a 30-second health check loop:
  - Detects finished workers via `handle.is_finished()`
  - Respawns dead workers with a 2-second delay
  - Warns when `live_workers < configured_workers`
- On shutdown signal: breaks loop and awaits all remaining handles

### 2. Worker-Level Panic Recovery (`crates/daemon/src/main.rs`)

Each worker iteration is wrapped in `AssertUnwindSafe(...).catch_unwind()`:

- On panic: increments `total_worker_restarts` counter, fixes
  idle/active counters, emits `worker_panic_recovered` event, sleeps 2s,
  then **continues** the loop (no `break`)
- This ensures single-iteration panics never terminate the worker

### 3. Panic Hook Crash Log (`crates/daemon/src/main.rs:116-136`)

A custom `std::panic::set_hook()` appends crash info to
`data/daemon_crash.log` with epoch timestamp before delegating to the default
hook. This ensures crash traces are persisted even when stdout is unavailable.

### 4. Crash Recovery on Startup (`crates/daemon/src/lifecycle.rs`)

- `detect_stale_pid(pid_path)`: reads `data/daemon.pid`, checks if the
  recorded PID is still alive via `nix::sys::signal::kill(pid, None)`
- If stale: emits `daemon_crash_recovered` event with the stale PID
- `write_pid_file()` then records the new daemon PID

### 5. Runtime Counters (`core/src/runtime.rs`)

`DaemonRuntimeSnapshot` exposes atomic counters:

| Field | Purpose |
|-------|---------|
| `configured_workers` | Target worker count from `--workers N` |
| `live_workers` | Currently alive workers |
| `idle_workers` | Workers waiting for tasks |
| `active_workers` | Workers executing tasks |
| `total_worker_restarts` | Cumulative panic-recovery restarts |

All transitions use `SeqCst` atomic ordering for strict consistency.

### Why this approach

1. **Defense in depth** — worker-level catch_unwind handles most panics
   inline; supervisor catches anything that escapes (e.g., tokio task abort)
2. **No external dependencies** — uses only `std::panic`, `nix`, and tokio
   primitives; no systemd/launchd integration required
3. **Observable** — every recovery action emits a daemon event and updates
   atomic counters, visible via `runtime_snapshot` gRPC endpoint
4. **Non-invasive** — `RestartRequestedError` exec-restart path and graceful
   SIGTERM shutdown are unchanged

### Alternatives considered

- **Process-level supervisor** (systemd, launchd): rejected as FR explicitly
  scoped to in-process resilience; external supervision is orthogonal
- **Worker thread pool** (rayon/threadpool): rejected; tokio tasks are
  lighter-weight and align with existing async architecture

## Key Files

| File | Role |
|------|------|
| `crates/daemon/src/main.rs` | Worker supervisor, worker loop, panic hook, crash recovery |
| `crates/daemon/src/lifecycle.rs` | PID file management, stale PID detection |
| `core/src/runtime.rs` | `DaemonRuntimeState` / `DaemonRuntimeSnapshot` with atomic counters |
