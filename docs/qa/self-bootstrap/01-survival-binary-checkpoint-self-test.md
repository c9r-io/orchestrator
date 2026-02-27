# Self-Bootstrap - Binary Checkpoint & Self-Test Acceptance Gate

**Module**: self-bootstrap
**Scope**: Validate Layer 1 (binary snapshot/restore) and Layer 2 (self-test builtin step) of the survival mechanism
**Scenarios**: 5
**Priority**: High

---

## Background

The self-bootstrap survival mechanism protects the orchestrator from bricking itself during self-referential development. This document covers:

- **Layer 1 (Binary Checkpoint)**: At cycle start, the release binary is copied to `.stable`; on auto-rollback, it is restored. Controlled by `binary_snapshot: true` in safety config.
- **Layer 2 (Self-Test Acceptance Gate)**: A builtin `self_test` step runs `cargo check` + `cargo test --lib` + `manifest validate` between `implement` and `qa_testing`.

Key functions: `snapshot_binary()`, `restore_binary_snapshot()`, `execute_self_test_step()` in `core/src/scheduler.rs`.

Fixture: `fixtures/manifests/bundles/self-bootstrap-test.yaml`
Workflow: `docs/workflow/self-bootstrap.yaml`

### Common Preconditions

```bash
rm -f fixtures/ticket/auto_*.md

QA_PROJECT="qa-survival-${USER}-$(date +%Y%m%d%H%M%S)"
./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
./scripts/orchestrator.sh apply -f docs/workflow/self-bootstrap.yaml
```

---

## Scenario 1: Binary Snapshot Created at Cycle Start

### Preconditions
- Common Preconditions applied
- Release binary exists at `core/target/release/agent-orchestrator` (run `cd core && cargo build --release` if needed)
- Workspace `self` has `self_referential: true` and `binary_snapshot: true`

### Goal
Verify that a `.stable` binary copy is created at the start of each cycle when binary snapshot is enabled on a self-referential workspace.

### Steps
1. Remove any existing `.stable` file:
   ```bash
   rm -f .stable
   ```
2. Create and start a task using the `self-bootstrap` workflow:
   ```bash
   ./scripts/orchestrator.sh task create "${QA_PROJECT}" --workflow self-bootstrap --goal "test binary snapshot"
   TASK_ID=$(./scripts/orchestrator.sh task list "${QA_PROJECT}" --json | jq -r '.[0].id')
   ./scripts/orchestrator.sh task start "${QA_PROJECT}" "${TASK_ID}"
   ```
3. Wait for the first cycle to begin (checkpoint_created event).
4. Query events for `binary_snapshot_created`.

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
   cp core/target/release/agent-orchestrator .stable
   ```
2. Create a task that will fail repeatedly (e.g., introduce a compile error in the implement step output).
3. Start the task and wait for 3 consecutive failures to trigger auto-rollback.
4. Query events for `auto_rollback` and `binary_snapshot_restored`.

### Expected
- Event `auto_rollback` is emitted after 3 consecutive failures
- Event `binary_snapshot_restored` is emitted in the same cycle as auto-rollback
- The release binary at `core/target/release/agent-orchestrator` matches the `.stable` file
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
- `scripts/orchestrator.sh` exists (for manifest validate phase)

### Goal
Verify that the `self_test` builtin step executes all three phases successfully and sets pipeline variables correctly.

### Steps
1. Create and start a task using the `self-bootstrap` workflow.
2. Wait for the `self_test` step to execute (after `implement` step).
3. Query events for `self_test_phase` entries and the `step_finished` event for `self_test`.

### Expected
- Three `self_test_phase` events emitted in order:
  1. `{"phase": "cargo_check", "passed": true}`
  2. `{"phase": "cargo_test_lib", "passed": true}`
  3. `{"phase": "manifest_validate", "passed": true}`
- `step_finished` event with `{"step": "self_test", "exit_code": 0, "success": true}`
- Pipeline variable `self_test_passed` is `"true"`
- Pipeline variable `self_test_exit_code` is `"0"`
- Task continues to `qa_testing` step (self_test does not block)

### Expected Data State
```sql
SELECT json_extract(payload_json, '$.phase') AS phase,
       json_extract(payload_json, '$.passed') AS passed
FROM events WHERE task_id = '{task_id}' AND event_type = 'self_test_phase'
ORDER BY created_at;
-- Expected: cargo_check/true, cargo_test_lib/true, manifest_validate/true

SELECT json_extract(payload_json, '$.exit_code') AS exit_code,
       json_extract(payload_json, '$.success') AS success
FROM events WHERE task_id = '{task_id}' AND event_type = 'step_finished'
  AND json_extract(payload_json, '$.step') = 'self_test';
-- Expected: exit_code=0, success=true
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
- `self_test_phase` event with `{"phase": "cargo_check", "passed": false}`
- No `cargo_test_lib` or `manifest_validate` phase events (execution stops at first failure)
- `step_finished` event with `exit_code != 0` and `success: false`
- Item status set to `self_test_failed`
- Pipeline variable `self_test_passed` is `"false"`
- Subsequent `qa_testing` step may still run (self_test failure does not hard-abort the pipeline, but the item is marked failed)

### Expected Data State
```sql
SELECT json_extract(payload_json, '$.phase') AS phase,
       json_extract(payload_json, '$.passed') AS passed
FROM events WHERE task_id = '{task_id}' AND event_type = 'self_test_phase'
ORDER BY created_at;
-- Expected: only cargo_check/false (no further phases)

SELECT json_extract(payload_json, '$.exit_code') AS exit_code
FROM events WHERE task_id = '{task_id}' AND event_type = 'step_finished'
  AND json_extract(payload_json, '$.step') = 'self_test';
-- Expected: exit_code != 0
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
