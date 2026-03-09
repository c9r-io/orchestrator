# Self-Bootstrap - Binary Checkpoint & Self-Test Acceptance Gate

**Module**: self-bootstrap
**Scope**: Validate Layer 1 (binary snapshot/restore) and Layer 2 (self-test builtin step) of the survival mechanism
**Scenarios**: 5
**Priority**: High

---

## Background

The self-bootstrap survival mechanism protects the orchestrator from bricking itself during self-referential development. This document covers:

- **Layer 1 (Binary Checkpoint)**: At cycle start, the release binary is copied to `.stable`; on auto-rollback, it is restored. Controlled by `binary_snapshot: true` in safety config.
- **Layer 2 (Self-Test Acceptance Gate)**: A builtin `self_test` step runs `cargo check` + `cargo test --lib` + `manifest validate` after `implement`. After `self_test` passes, `self_restart` (Layer 5) rebuilds the binary and restarts the process.

Key functions: `snapshot_binary()`, `restore_binary_snapshot()`, `execute_self_test_step()` in `core/src/scheduler.rs`.

Fixture: `fixtures/manifests/bundles/self-bootstrap-test.yaml`
Workflow: `fixtures/manifests/bundles/self-bootstrap-mock.yaml`

### Common Preconditions

> **CRITICAL: QA tests MUST use mock fixtures, never real workflows.**
> The real workflow at `docs/workflow/self-bootstrap.yaml` uses live Claude agents
> and will consume API credits rapidly. Always use `fixtures/manifests/bundles/self-bootstrap-mock.yaml`
> which contains deterministic `echo` mock agents.

> **Important**: For self-referential workflows, apply a manifest that explicitly
> sets `self_referential: true` on the workspace. Use `apply -f <manifest> --project`
> with a manifest that preserves `self_referential: true`.

```bash
rm -f fixtures/ticket/auto_*.md

# A global base config must exist first (provides defaults)
orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml

QA_PROJECT="qa-survival"
orchestrator project reset "${QA_PROJECT}" --force --include-config
orchestrator apply -f fixtures/manifests/bundles/self-bootstrap-mock.yaml --project "${QA_PROJECT}"
```

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| `binary_snapshot_created` event never emitted | `self_referential` resolved to `false` at runtime because the applied manifest does not set `self_referential: true` | Use `apply -f <manifest> --project` with a manifest that sets `self_referential: true` on the workspace |
| `snapshot_binary` logs warning "release binary not found" | Binary not built | Run `cargo build --release -p orchestratord` before testing |
| "no agent supports capability" on task create | Project-scoped agents not checked during validation | Fixed: `build_execution_plan_for_project` merges project + global agents |
| Task uses global workspace instead of project workspace | No `--workspace` flag and global default doesn't match project | Fixed: auto-resolves to project's single workspace when not specified |
| "EMPTY_WORKFLOWS" error with project-only config | Global workflow check didn't account for project-scoped workflows | Fixed: validation now checks `has_project_workflows` |
| "defaults.workflow does not exist" with project-only config | A global base config with at least one workflow/workspace/agent must exist before applying project-scoped resources | Apply `echo-workflow.yaml` (or any base fixture) first |
| `orchestratord` appears to hang after self-restart | **Not a hang.** The `self_restart` step triggers `exec()` self-replacement; the daemon replaces itself in-place and resumes the task in cycle 2. The `self_restart` step has `repeatable: false` so it is skipped on cycle 2, and the process exits normally. Total wall time ≈ 2× a single cycle. | Wait for the full run to complete. If testing restart behavior specifically, check the daemon logs for the `exec()` self-replacement event and verify cycle 2 completes with exit 0. |

---

## Scenario 1: Binary Snapshot Created at Cycle Start

### Preconditions
- Common Preconditions applied
- Release binary exists at `target/release/orchestratord` (run `cargo build --release -p orchestratord` if needed)
- Workspace `self` has `self_referential: true` and `binary_snapshot: true`

### Goal
Verify that a `.stable` binary copy is created at the start of each cycle when binary snapshot is enabled on a self-referential workspace.

### Steps
1. Remove any existing `.stable` file:
   ```bash
   rm -f .stable
   ```
2. Create a task (use the binary directly to avoid duplicate-task on restart):
   ```bash
   BINARY="target/release/orchestratord"
   $BINARY task create --project "${QA_PROJECT}" --workflow self-bootstrap --goal "test binary snapshot"
   TASK_ID=$($BINARY task list -s restart_pending -o json 2>/dev/null | jq -r '.[0].id // empty')
   # If task auto-started and exited 75, it is now restart_pending.
   # If still pending, start it via the daemon which handles the restart loop:
   [ -z "$TASK_ID" ] && TASK_ID=$($BINARY task list -s pending -o json 2>/dev/null | jq -r '.[0].id')
   ```
3. Start (or resume) the task via the daemon (which handles restart via `exec()` self-replacement):
   ```bash
   orchestrator task start "${TASK_ID}"
   ```
   > **Note**: On cycle 1, `self_restart` triggers `exec()` to replace the daemon in-place.
   > Cycle 2 completes normally (`self_restart` skipped via `repeatable: false`).
   > This is expected behavior, not a hang. Total wall time ≈ 2× a single cycle.
4. Wait for the first cycle to begin (checkpoint_created event).
5. Query events for `binary_snapshot_created`.

### Expected
- `.stable` file exists in the workspace root
- Event `binary_snapshot_created` is emitted with `cycle: 1` and the `.stable` path
- The `.stable` file is a valid copy of the release binary (same size or executable)

### Expected Data State
```sql
SELECT event_type, json_extract(payload_json, '$.cycle') AS cycle,
       json_extract(payload_json, '$.path') AS path
FROM events WHERE task_id = '{task_id}' AND event_type = 'binary_snapshot_created';
-- Expected: 1 row with cycle=1, path ending in '.stable'
```

---

## Scenario 2: Binary Snapshot Restored on Auto-Rollback

### Preconditions
- Common Preconditions applied
- `.stable` binary exists (from a previous successful snapshot or manual copy)
- Workspace has `auto_rollback: true`, `checkpoint_strategy: git_tag`, `binary_snapshot: true`, `max_consecutive_failures: 3`

### Goal
Verify that when auto-rollback triggers (after max consecutive failures), the `.stable` binary is restored over the live release binary.

### Steps
1. Ensure `.stable` binary exists:
   ```bash
   cp target/release/orchestratord .stable
   ```
2. Create a task that will fail repeatedly (e.g., introduce a compile error in the implement step output).
3. Start the task and wait for 3 consecutive failures to trigger auto-rollback.
4. Query events for `auto_rollback` and `binary_snapshot_restored`.

### Expected
- Event `auto_rollback` is emitted after 3 consecutive failures
- Event `binary_snapshot_restored` is emitted in the same cycle as auto-rollback
- The release binary at `target/release/orchestratord` matches the `.stable` file
- `consecutive_failures` counter is reset to 0 after rollback

### Expected Data State
```sql
SELECT event_type, json_extract(payload_json, '$.cycle') AS cycle
FROM events WHERE task_id = '{task_id}'
  AND event_type IN ('auto_rollback', 'binary_snapshot_restored')
ORDER BY created_at;
-- Expected: auto_rollback followed by binary_snapshot_restored, same cycle
```

---

## Scenario 3: Binary Snapshot Skipped When Disabled or Non-Self-Referential

### Preconditions
- Common Preconditions applied
- Release binary exists

### Goal
Verify that binary snapshot is NOT created when `binary_snapshot: false` or when the workspace is not `self_referential`.

### Steps
1. Apply a workflow manifest with `binary_snapshot: false` (or omit the field, default is false):
   ```yaml
   safety:
     max_consecutive_failures: 3
     auto_rollback: true
     checkpoint_strategy: git_tag
     # binary_snapshot not set (defaults to false)
   ```
2. Create and start a task.
3. Wait for the first cycle checkpoint.
4. Query events for `binary_snapshot_created`.

### Expected
- No `binary_snapshot_created` event exists for this task
- No `.stable` file is created (or if it existed before, it is not updated)
- `checkpoint_created` event still fires normally (git tag checkpoint is independent)

### Expected Data State
```sql
SELECT COUNT(*) FROM events
WHERE task_id = '{task_id}' AND event_type = 'binary_snapshot_created';
-- Expected: 0
```

---

## Scenario 4: Self-Test Step Passes (All Three Phases)

### Preconditions
- Common Preconditions applied
- Codebase is in a clean, compilable state (`cargo check` and `cargo test --lib` pass)
- `orchestrator` CLI is available (for manifest validate phase)

### Goal
Verify that the `self_test` builtin step executes all three phases successfully and sets pipeline variables correctly.

### Steps
1. Create and start a task using the `self-bootstrap` workflow.
2. Wait for the `self_test` step to execute (after `implement` step).
3. Query the `step_finished` event for `self_test` in the events table.

### Expected
- Three `self_test_phase` in-memory events emitted in order (visible in SSE stream, not persisted to SQLite):
  1. `{"phase": "cargo_check", "passed": true}`
  2. `{"phase": "cargo_test_lib", "passed": true}`
  3. `{"phase": "manifest_validate", "passed": true}`
- `step_finished` event persisted to SQLite with `{"step": "self_test", "exit_code": 0, "success": true}`
- Pipeline variable `self_test_passed` is `"true"`
- Pipeline variable `self_test_exit_code` is `"0"`
- Task continues to `qa_testing` step (self_test does not block)

### Expected Data State
```sql
-- step_finished is persisted to the events table via insert_event()
SELECT json_extract(payload_json, '$.exit_code') AS exit_code,
       json_extract(payload_json, '$.success') AS success
FROM events WHERE task_id = '{task_id}' AND event_type = 'step_finished'
  AND json_extract(payload_json, '$.step') = 'self_test';
-- Expected: exit_code=0, success=true

-- NOTE: self_test_phase events are emitted via emit_event() (in-memory/SSE only)
-- and are NOT persisted to the events table. Verify them via the SSE event stream
-- or process logs, not via SQL queries.
```

---

## Scenario 5: Self-Test Step Fails (cargo check failure)

### Preconditions
- Common Preconditions applied
- Introduce a deliberate compile error (e.g., add `let x: i32 = "bad";` to a source file)

### Goal
Verify that when `cargo check` fails in the self-test step, the step returns a non-zero exit code, sets pipeline variables to indicate failure, and marks the item as `self_test_failed`.

### Steps
1. Introduce a compile error in a source file:
   ```bash
   echo 'fn _qa_break() { let x: i32 = "bad"; }' >> core/src/lib.rs
   ```
2. Create and start a task using the `self-bootstrap` workflow.
3. Wait for the `self_test` step to execute.
4. Query events for `self_test_phase` and `step_finished`.
5. Clean up the compile error:
   ```bash
   # Revert the last line added
   sed -i '' '$ d' core/src/lib.rs
   ```

### Expected
- `self_test_phase` in-memory event with `{"phase": "cargo_check", "passed": false}` (visible in SSE stream, not in SQLite)
- No `cargo_test_lib` or `manifest_validate` phase events (execution stops at first failure)
- `step_finished` event persisted to SQLite with `exit_code != 0` and `success: false`
- Item status set to `self_test_failed`
- Pipeline variable `self_test_passed` is `"false"`
- Subsequent `qa_testing` step may still run (self_test failure does not hard-abort the pipeline, but the item is marked failed)

### Expected Data State
```sql
-- step_finished is persisted to the events table
SELECT json_extract(payload_json, '$.exit_code') AS exit_code
FROM events WHERE task_id = '{task_id}' AND event_type = 'step_finished'
  AND json_extract(payload_json, '$.step') = 'self_test';
-- Expected: exit_code != 0

-- NOTE: self_test_phase events use emit_event() (in-memory/SSE only).
-- Verify cargo_check failure via SSE stream or process logs, not SQL.
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Binary Snapshot Created at Cycle Start | ☐ | | | |
| 2 | Binary Snapshot Restored on Auto-Rollback | ☐ | | | |
| 3 | Binary Snapshot Skipped When Disabled | ☐ | | | |
| 4 | Self-Test Step Passes (All Three Phases) | ☐ | | | |
| 5 | Self-Test Step Fails (cargo check failure) | ☐ | | | |
