---
self_referential_safe: true
---
# Orchestrator - Orphaned Running Items Auto-Recovery

**Module**: orchestrator
**Scope**: Validate startup orphan recovery, runtime stall detection, CLI `task recover` command, and audit events for FR-033
**Scenarios**: 5
**Priority**: Critical

---

## Background

When the daemon crashes (SIGKILL, OOM, panic) while items are in `running` state, those items become permanently stuck. FR-033 adds three recovery mechanisms:

1. **Startup recovery** — On daemon boot, all `running` items are reset to `pending` and their parent tasks to `restart_pending`, before workers spawn
2. **Stall detection sweep** — Background task (every 5 min) detects items running longer than `--stall-timeout-mins` and recovers them
3. **CLI `task recover`** — Manual recovery via `orchestrator task recover <task_id>`

All scenarios use code review and existing unit tests — no daemon start/kill required.

### Verification Command

```bash
cargo test -p orchestrator-core -- recover_orphaned_running_items recover_stalled_running_items
```

### Unit Test Coverage

| Test | File | Covers |
|------|------|--------|
| `recover_orphaned_running_items_resets_items_and_task` | `state_tests.rs:476` | S1 — running items → pending, task → restart_pending |
| `recover_orphaned_running_items_returns_empty_when_no_orphans` | `state_tests.rs:531` | S2 — idempotent when no orphans |
| `recover_orphaned_running_items_for_task_only_affects_target_task` | `state_tests.rs:569` | S3 — task-scoped recovery |
| `recover_orphaned_running_items_does_not_affect_terminal_items` | `state_tests.rs:541` | S4 — terminal items unchanged |
| `recover_stalled_running_items_respects_threshold` | `state_tests.rs:631` | S5 — threshold-based stall detection |
| `recover_orphaned_running_items_skips_paused_task_in_return` | `state_tests.rs:686` | Edge case — paused tasks |

---

## Scenario 1: Startup Recovery Resets Orphaned Running Items (Code Review + Unit Test)

### Goal

Verify that `recover_orphaned_running_items()` resets all running items to `pending` and parent tasks to `restart_pending`.

### Steps

1. Review `core/src/task_repository/state.rs` — `recover_orphaned_running_items()` function
2. Review `crates/daemon/src/main.rs` — startup sequence calling `recover_orphaned_running_items()` before worker spawn
3. Run unit test:
   ```bash
   cargo test -p orchestrator-core -- recover_orphaned_running_items_resets_items_and_task
   ```

### Expected

- [ ] Function queries all items with `status='running'`
- [ ] Running items are reset to `status='pending'`, `started_at=NULL`
- [ ] Parent tasks are set to `status='restart_pending'`
- [ ] Returns list of `(task_id, [item_ids])` that were recovered
- [ ] `orphaned_items_recovered` event emitted at startup (code review of `main.rs`)
- [ ] Unit test passes

---

## Scenario 2: Startup Recovery Is Idempotent (Code Review + Unit Test)

### Goal

Verify that startup recovery does nothing and returns empty when there are no orphaned items.

### Steps

1. Review `core/src/task_repository/state.rs` — `recover_orphaned_running_items()` with no running items
2. Run unit test:
   ```bash
   cargo test -p orchestrator-core -- recover_orphaned_running_items_returns_empty_when_no_orphans
   ```

### Expected

- [ ] When no items have `status='running'`, function returns empty vec
- [ ] No events emitted, no status changes
- [ ] Unit test passes

---

## Scenario 3: CLI `task recover` Resets Orphaned Items for a Specific Task (Code Review + Unit Test)

### Goal

Verify that `recover_orphaned_running_items_for_task()` only recovers items for the specified task, leaving other tasks' items unchanged.

### Steps

1. Review `core/src/task_repository/state.rs` — `recover_orphaned_running_items_for_task()` function
2. Run unit test:
   ```bash
   cargo test -p orchestrator-core -- recover_orphaned_running_items_for_task_only_affects_target_task
   ```

### Expected

- [ ] Only items belonging to the target `task_id` are reset to `pending`
- [ ] Other tasks' running items remain in `running` state
- [ ] Target task status set to `restart_pending`
- [ ] Unit test passes: task A items reset, task B items still running

---

## Scenario 4: Terminal Items Are Not Affected by Recovery (Code Review + Unit Test)

### Goal

Verify that items in terminal states (`qa_passed`, `fixed`, `completed`) are not modified by recovery.

### Steps

1. Review `core/src/task_repository/state.rs` — recovery SQL `WHERE status='running'` filter
2. Run unit test:
   ```bash
   cargo test -p orchestrator-core -- recover_orphaned_running_items_does_not_affect_terminal_items
   ```

### Expected

- [ ] Recovery SQL only targets `status='running'` items
- [ ] Items in `qa_passed`, `fixed`, `completed` states are untouched
- [ ] Function returns empty vec when only terminal items exist
- [ ] Unit test passes: `qa_passed` item remains unchanged

---

## Scenario 5: Stall Detection Sweep Recovers Long-Running Items (Code Review + Unit Test)

### Goal

Verify that `recover_stalled_running_items()` correctly uses the time threshold to identify and recover stalled items.

### Steps

1. Review `core/src/task_repository/state.rs` — `recover_stalled_running_items()` function
2. Review `crates/daemon/src/main.rs` — background stall sweep loop and `--stall-timeout-mins` CLI flag
3. Run unit test:
   ```bash
   cargo test -p orchestrator-core -- recover_stalled_running_items_respects_threshold
   ```

### Expected

- [ ] Items with `started_at` older than threshold are recovered (reset to `pending`)
- [ ] Items within threshold are NOT recovered (remain `running`)
- [ ] Threshold comparison uses `started_at` vs `now() - threshold_secs`
- [ ] Unit test passes: 2h-old item NOT recovered at 3h threshold, IS recovered at 1h threshold
- [ ] Background sweep configured via `--stall-timeout-mins` flag (code review of `main.rs`)

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Startup Recovery Resets Orphaned Running Items | ✅ | 2026-03-28 | claude | Code review + unit test pass. running items→pending, started_at=NULL cleared, task→restart_pending. orphaned_items_recovered event emitted in main.rs:289-321 |
| 2 | Startup Recovery Is Idempotent (No Orphans) | ✅ | 2026-03-28 | claude | Code review + unit test pass. No running items→returns empty vec, no events emitted, no status changes |
| 3 | CLI `task recover` Resets Orphaned Items for Specific Task | ✅ | 2026-03-28 | claude | Code review + unit test pass. recover_orphaned_running_items_for_task() only affects target task, others untouched |
| 4 | Terminal Items Are Not Affected by Recovery | ✅ | 2026-03-28 | claude | Code review + unit test pass. SQL WHERE status='running' filter only, terminal items unchanged |
| 5 | Stall Detection Sweep Recovers Long-Running Items | ✅ | 2026-03-28 | claude | Code review + unit test pass. recover_stalled_running_items() uses started_at < cutoff threshold. Background sweep via main.rs:488-507 with --stall-timeout-mins flag |
