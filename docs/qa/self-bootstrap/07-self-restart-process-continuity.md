# Self-Bootstrap - Self-Restart and Process Continuity

**Module**: self-bootstrap
**Scope**: Validate the self_restart builtin step (Layer 5), orchestrator.sh restart loop, restart_pending task resumption, and priority claiming
**Scenarios**: 5
**Priority**: High

---

## Background

The `self_restart` step extends the self-bootstrap survival mechanism with a 5th layer: after `self_test` passes, the orchestrator rebuilds its own binary, verifies it, snapshots `.stable`, sets the task to `restart_pending`, and exits with code 75. The daemon's foreground mode (`orchestrator daemon start -f`) detects exit 75 and relaunches the new binary, which auto-claims the `restart_pending` task and resumes the loop.

Key functions:
- `execute_self_restart_step()` in `core/src/scheduler/safety.rs`
- `EXIT_RESTART = 75` constant
- `prepare_task_for_start_batch()` restart_pending branch in `core/src/task_repository/state.rs`
- `claim_next_pending_task()` priority SQL in `core/src/scheduler_service.rs`
- Restart loop in `orchestrator daemon start -f`

Workflow: `fixtures/manifests/bundles/self-bootstrap-mock.yaml`

### Database Schema Reference

### Table: tasks
| Column | Type | Notes |
|--------|------|-------|
| id | TEXT | Primary key |
| status | TEXT | Now includes `restart_pending` as valid value |
| current_cycle | INTEGER | Preserved across restart boundary |

### Table: events
| Column | Type | Notes |
|--------|------|-------|
| event_type | TEXT | New types: `self_restart_phase`, `self_restart_ready`, `step_finished` with `step=self_restart` |
| payload_json | TEXT | Contains phase details, exit codes, restart flag, `build_git_hash`, `build_timestamp` (since build-version-hash) |

---

## Scenario 1: self_restart Step Build + Verify + Snapshot Success (Unit Level)

### Preconditions
- `cargo check` and `cargo test --lib` pass (codebase compiles)
- Release binary exists at `target/release/orchestratord`

### Goal
Verify that the unit tests for `execute_self_restart_step` pass: build succeeds, binary verification succeeds, `.stable` is created, EXIT_RESTART (75) is returned, and task status is set to `restart_pending`.

### Steps
1. Run the self_restart success unit test:
   ```bash
   cd /Volumes/Yotta/ai_native_sdlc/core
   cargo test --lib -- --test-threads=1 test_execute_self_restart_step_success_returns_exit_restart 2>&1
   ```
2. Verify it passes.
3. Run the EXIT_RESTART constant test:
   ```bash
   cargo test --lib -- test_exit_restart_constant 2>&1
   ```

### Expected
- `test_execute_self_restart_step_success_returns_exit_restart` passes
- `test_exit_restart_constant` passes — EXIT_RESTART == 75
- Test verifies: exit code == 75, task status == `restart_pending`, `.stable` file exists

---

## Scenario 2: self_restart Step Build Failure (Unit Level)

### Preconditions
- Unit test environment available

### Goal
Verify that when `cargo build --release` fails, `execute_self_restart_step` returns the cargo exit code (not 75), does NOT set task status to `restart_pending`, and the pipeline can continue via `on_failure: continue`.

### Steps
1. Run the build failure unit test:
   ```bash
   cd /Volumes/Yotta/ai_native_sdlc/core
   cargo test --lib -- --test-threads=1 test_execute_self_restart_step_build_fails 2>&1
   ```
2. Verify it passes.

### Expected
- `test_execute_self_restart_step_build_fails` passes
- Test verifies: exit code == 7 (mock cargo exit), not 75
- Task status is NOT `restart_pending`

---

## Scenario 3: restart_pending Task Resumption Preserves Item State

### Preconditions
- Unit test environment available

### Goal
Verify that `prepare_task_for_start_batch` handles `restart_pending` status by transitioning to `running` WITHOUT resetting item statuses (unlike `failed` which resets unresolved items to pending).

### Steps
1. Run the restart_pending preservation test:
   ```bash
   cd /Volumes/Yotta/ai_native_sdlc/core
   cargo test --lib -- --test-threads=1 prepare_task_restart_pending_preserves_items 2>&1
   ```
2. Run the status behavior test:
   ```bash
   cargo test --lib -- --test-threads=1 set_task_status_restart_pending_clears_completed_at 2>&1
   ```

### Expected
- `prepare_task_restart_pending_preserves_items` passes — items remain `qa_passed` (not reset to `pending`)
- `set_task_status_restart_pending_clears_completed_at` passes — `completed_at` is cleared

### Expected Data State
```sql
-- After prepare_task_for_start_batch on restart_pending task:
SELECT status FROM tasks WHERE id = '{task_id}';
-- Expected: 'running'

SELECT status FROM task_items WHERE task_id = '{task_id}';
-- Expected: original item statuses preserved (e.g., 'qa_passed'), NOT 'pending'
```

---

## Scenario 4: claim_next_pending_task Prioritizes restart_pending

### Preconditions
- Unit test environment available

### Goal
Verify that the worker's `claim_next_pending_task` picks up `restart_pending` tasks before `pending` tasks, ensuring restart continuity takes priority over new work.

### Steps
1. Run the priority claiming test:
   ```bash
   cd /Volumes/Yotta/ai_native_sdlc/core
   cargo test --lib -- --test-threads=1 claim_next_prioritizes_restart_pending 2>&1
   ```
2. Run the resumability test:
   ```bash
   cargo test --lib -- --test-threads=1 find_latest_resumable_task_id_includes_restart_pending 2>&1
   ```

### Expected
- `claim_next_prioritizes_restart_pending` passes — `restart_pending` task claimed before `pending` task
- `find_latest_resumable_task_id_includes_restart_pending` passes — `restart_pending` is a resumable status

### Expected Data State
```sql
-- Claim ordering: restart_pending before pending
SELECT id FROM tasks WHERE status IN ('restart_pending', 'pending')
ORDER BY CASE status WHEN 'restart_pending' THEN 0 ELSE 1 END, created_at ASC
LIMIT 1;
-- Expected: returns the restart_pending task ID
```

---

## Scenario 5: Daemon Restart Loop and Step Registration

### Preconditions
- Repository checked out at `/Volumes/Yotta/ai_native_sdlc`
- `orchestrator daemon start -f` is available (built-in restart loop with exit code 75 handling)

### Goal
Verify that (a) the daemon's foreground mode contains the restart-aware loop detecting exit code 75, (b) `self_restart` is registered as a known builtin step, and (c) the `self-bootstrap.yaml` workflow includes the `self_restart` step in the correct position.

### Steps
1. Verify the daemon handles exit code 75 restart loop (built into the binary).
2. Verify self_restart is registered as a known step and builtin:
   ```bash
   cd /Volumes/Yotta/ai_native_sdlc/core
   cargo test --lib -- test_validate_step_type_known_ids 2>&1
   ```
3. Verify self_restart appears in the workflow YAML after self_test:
   ```bash
   grep -A2 'id: self_restart' fixtures/manifests/bundles/self-bootstrap-mock.yaml
   ```
4. Verify self_restart has `repeatable: false` (only runs in Cycle 1):
   ```bash
   grep -A8 'id: self_restart' fixtures/manifests/bundles/self-bootstrap-mock.yaml | grep 'repeatable'
   ```
5. Verify manifest validates with the new step:
   ```bash
   orchestrator manifest validate -f fixtures/manifests/bundles/self-bootstrap-mock.yaml 2>&1
   ```

### Expected
- Exit code 75 restart loop is built into the daemon's foreground mode
- `test_validate_step_type_known_ids` passes (includes `self_restart`)
- `self_restart` step appears in YAML with `builtin: self_restart`, `repeatable: false`, `on_failure: continue`
- `manifest validate` passes without errors

---

## Binary Identity Verification

The `self_restart` step records the SHA256 hash of the newly built binary in a persisted `self_restart_ready` event (SQLite `events` table). After the process restarts and the worker claims the `restart_pending` task, `verify_post_restart_binary()` is called automatically to compare:

1. **Expected**: SHA256 from `self_restart_ready` event payload (`binary_sha256` field)
2. **Actual**: SHA256 of `std::env::current_exe()` (the currently running binary)

Results are persisted as a `binary_verification` event with `verified: true/false`. Both `self_restart_ready` and `binary_verification` events also include `build_git_hash` and `build_timestamp` fields for build provenance traceability (see `docs/qa/self-bootstrap/08-build-version-hash.md`).

### Unit test coverage
- `test_verify_post_restart_binary_no_event_returns_true` — no event = skip (safe default)
- `test_verify_post_restart_binary_with_matching_event` — SHA256 match = verified
- `test_verify_post_restart_binary_with_mismatch` — SHA256 mismatch = warning logged

### E2E verification note
Full end-to-end testing (exit 75 → daemon restart loop relaunch → new binary claims restart_pending → SHA256 verification) requires an actual `cargo build --release` cycle and is validated during real self-bootstrap runs, not in unit tests. The unit tests validate each layer independently.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | self_restart Step Build + Verify + Snapshot Success | PASS | 2026-03-05 | claude | Unit tests pass: exit_code==75, status==restart_pending, .stable exists |
| 2 | self_restart Step Build Failure | PASS | 2026-03-05 | claude | Unit test pass: exit_code==7 (mock), not 75, no restart_pending |
| 3 | restart_pending Task Resumption Preserves Item State | PASS | 2026-03-05 | claude | Items preserved as qa_passed, completed_at cleared |
| 4 | claim_next_pending_task Prioritizes restart_pending | PASS | 2026-03-05 | claude | restart_pending claimed before pending; resumable includes restart_pending |
| 5 | Daemon Restart Loop and Step Registration | PASS | 2026-03-05 | claude | Daemon restart loop handles exit 75, step registered, manifest validates |
