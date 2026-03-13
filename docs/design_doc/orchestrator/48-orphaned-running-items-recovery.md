# Design Doc 48: Daemon 重启后孤立 Running Items 自动恢复 (FR-033)

## Problem

When the daemon crashes (SIGKILL, OOM, panic) while task items are in `running`
state, those items become permanently orphaned. The existing `prepare_task_for_start_batch`
only resets `unresolved` items, and `running` tasks are rejected by the
re-entry guard. No mechanism existed to detect or recover these orphans, requiring
manual SQL intervention.

## Design Decision

**Three-tier recovery: startup scan, runtime stall detection, and CLI manual recovery.**

### 1. Startup Orphan Recovery (`core/src/task_repository/state.rs`)

`recover_orphaned_running_items()` runs during daemon boot, before workers spawn:

- Queries all `task_items` with `status='running'`
- Resets each to `status='pending'`, clears `started_at` and `completed_at`
- Resets parent tasks from `running` to `restart_pending`
- Returns `Vec<(task_id, Vec<item_id>)>` for audit logging
- Uses `unchecked_transaction()` for atomic state transitions
- Operates on all tasks globally — no PID or process affinity required

### 2. Runtime Stall Detection (`crates/daemon/src/main.rs`)

A background sweep (every 5 minutes) detects items stuck beyond a configurable
threshold:

- CLI argument: `--stall-timeout-mins` (default: 30)
- Calls `recover_stalled_running_items(threshold_secs)` which finds items where
  `status='running' AND started_at < cutoff`
- Same state transitions as startup recovery
- Emits `item_stall_recovered` event per recovered item
- Acts as second line of defense for items that become orphaned at runtime
  (e.g., agent process dies without daemon crash)

### 3. CLI Manual Recovery (`orchestrator task recover <task_id>`)

Single-task variant for operator-driven recovery:

- gRPC endpoint: `TaskRecover` on the task service
- Delegates to `recover_orphaned_running_items_for_task(task_id)`
- Returns count of recovered items with user-friendly message
- Allows targeted recovery without daemon restart

### Audit Events

| Event Type | Trigger | Payload |
|-----------|---------|---------|
| `orphaned_items_recovered` | Startup recovery | `{"task_id", "recovered_item_ids", "count"}` |
| `item_stall_recovered` | Stall sweep | `{"task_id", "item_id", "stall_threshold_secs"}` |

### Why this approach

1. **Startup-first** — covers the most common case (daemon crash) with zero
   latency; no waiting for sweep intervals
2. **Defense in depth** — stall sweep catches runtime orphans that startup
   recovery cannot (agent dies while daemon stays alive)
3. **No heartbeat overhead** — avoids agent→daemon heartbeat protocol; uses
   time-based heuristic instead, keeping agent implementation unchanged
4. **Selective recovery** — only `running` items are affected; terminal states
   (`qa_passed`, `fixed`, `completed`) are never modified
5. **Single-machine scope** — explicitly avoids distributed locking, matching
   the project's single-daemon deployment model

### Alternatives considered

- **Agent heartbeat protocol**: rejected; adds complexity to all agent
  implementations for a problem solvable by time-based heuristics
- **PID-based ownership tracking**: rejected; adds coupling between item state
  and OS process identity, complicating daemon self-restart via exec()
- **WAL-based crash journal**: rejected; SQLite's existing WAL recovery is
  sufficient; the problem is semantic state, not data corruption

## Key Files

| File | Role |
|------|------|
| `core/src/task_repository/state.rs` | `recover_orphaned_running_items`, `recover_orphaned_running_items_for_task`, `recover_stalled_running_items` |
| `core/src/task_repository/mod.rs` | Async wrappers on `AsyncSqliteTaskRepository` |
| `crates/daemon/src/main.rs` | Startup recovery call, stall sweep background task, event emission |
| `crates/cli/src/cli.rs` | `TaskCommands::Recover` variant definition |
| `crates/cli/src/commands/task.rs` | CLI handler dispatching to gRPC |
| `crates/daemon/src/server/task.rs` | gRPC `task_recover` endpoint |
| `core/src/service/task.rs` | Service-layer delegation |
| `core/src/task_repository/tests/state_tests.rs` | 5 unit tests covering all recovery paths |
