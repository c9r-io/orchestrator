# Orchestrator - Generate Items Regression Narrowing

**Module**: orchestrator
**Scope**: generate_items post-action surviving self_restart, dynamic item filtering at cycle start, qa_testing scope narrowing
**Scenarios**: 3
**Priority**: High

---

## Background

The self-bootstrap workflow uses `generate_items` as a deferred post-action on `qa_doc_gen` to narrow the QA testing scope. The `qa_doc_gen` step outputs `regression_targets` in its JSON, and `generate_items` extracts those targets into dynamic task items (`source='dynamic'`). When dynamic items exist, item-scoped steps (qa_testing, ticket_fix) fan out only over the dynamic subset — not the full set of static QA items.

Two bugs were fixed:

1. **generate_items lost across self_restart**: The `RestartRequestedError` from `self_restart` short-circuited `process_item_filtered` before the deferred `pending_generate_items` could execute. Fix: catch the error, flush pipeline vars and generate_items to DB, then re-throw.

2. **Dynamic items ignored at cycle start**: `execute_cycle_segments` reloaded all items from DB at each cycle start without checking for dynamic items. Fix: when dynamic items exist in the DB, filter to only dynamic items before processing segments.

### Key Code Paths

- `core/src/scheduler/loop_engine/segment.rs` — `flush_pending_generate_items()` called on RestartRequestedError and after normal task segment completion
- `core/src/scheduler/loop_engine/mod.rs:349-362` — dynamic item filtering at cycle start
- `core/src/scheduler/item_executor/apply.rs:190-198` — generate_items buffering into `pending_generate_items`
- `core/src/scheduler/item_generate.rs` — `extract_dynamic_items()` and `create_dynamic_task_items_async()`

---

## Scenario 1: generate_items Narrows qa_testing to Regression Targets

### Preconditions
- Daemon running with `--workers 2`
- Fixture bundle applied: `fixtures/manifests/bundles/generate-items-narrow-test.yaml`
- Workspace `narrow-test-ws` has `qa_targets: [fixtures/qa-narrow-test]` (5 static files)

### Goal
Verify that `generate_items` extracts regression targets from `qa_doc_gen` output and that `qa_testing` only processes the 2 dynamic items — not all 5 static items.

### Steps
1. Reset environment:
   ```bash
   orchestrator delete --all --yes
   orchestrator init
   ```
2. Apply the fixture:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/generate-items-narrow-test.yaml
   ```
3. Create the task:
   ```bash
   orchestrator task create --project narrow-test --workflow narrow-test --goal "test generate_items narrowing"
   ```
4. Wait for task to complete:
   ```bash
   orchestrator task watch <task_id>
   ```
5. Check task items in DB:
   ```bash
   sqlite3 data/orchestrator.db "SELECT qa_file_path, source, label FROM task_items WHERE task_id='<task_id>' ORDER BY source, qa_file_path;"
   ```
6. Check events to verify qa_testing fan-out:
   ```bash
   orchestrator task trace <task_id>
   ```

### Expected
- Task completes successfully
- DB contains 5 static items + 2 dynamic items (7 total)
- Dynamic items have `source='dynamic'`:
  - `qa_file_path='fixtures/qa-narrow-test/target-a.md'`, `label='target-a'`
  - `qa_file_path='fixtures/qa-narrow-test/target-b.md'`, `label='target-b'`
- `qa_testing` step events show execution for only 2 items (target-a, target-b)
- No `qa_testing` events for `static-skip-1.md`, `static-skip-2.md`, or `static-skip-3.md`

### Expected Data State
```sql
-- Dynamic items created by generate_items
SELECT qa_file_path, source, label
FROM task_items WHERE task_id = '{task_id}' AND source = 'dynamic'
ORDER BY qa_file_path;
-- Expected: 2 rows
-- Row 1: fixtures/qa-narrow-test/target-a.md | dynamic | target-a
-- Row 2: fixtures/qa-narrow-test/target-b.md | dynamic | target-b

-- Static items still exist but are bypassed
SELECT COUNT(*) FROM task_items WHERE task_id = '{task_id}' AND source = 'static';
-- Expected: 5

-- qa_testing only ran for dynamic items
SELECT item_key FROM events
WHERE task_id = '{task_id}' AND step_id = 'qa_testing' AND event_type = 'step_started'
ORDER BY item_key;
-- Expected: 2 rows (target-a, target-b paths only)
```

### Troubleshooting

| Symptom | Likely Cause | Fix |
|---------|-------------|-----|
| qa_testing runs for all 5 items | Dynamic items not created or filtering broken | Check `generate_items` event in trace; verify `qa_doc_gen_output` pipeline var contains `regression_targets` |
| 0 dynamic items in DB | `extract_dynamic_items` failed | Check daemon logs for `failed to extract dynamic items` warning |
| Task stuck after qa_doc_gen | generate_items crash | Check daemon logs for panics in `flush_pending_generate_items` |

---

## Scenario 2: generate_items Survives self_restart (2-Cycle Workflow)

### Preconditions
- Daemon running with `--workers 2`
- Full self-bootstrap mock bundle applied: `fixtures/manifests/bundles/self-bootstrap-mock.yaml`

### Goal
Verify that when `self_restart` fires at the end of Cycle 1's task segment, the deferred `generate_items` is flushed to DB before the process restarts, and Cycle 2 correctly picks up the dynamic items.

### Steps
1. Reset environment:
   ```bash
   orchestrator delete --all --yes
   orchestrator init
   ```
2. Apply the self-bootstrap mock:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/self-bootstrap-mock.yaml
   ```
3. Create the task:
   ```bash
   orchestrator task create --project self-bootstrap-mock --workflow self-bootstrap --goal "test generate_items across restart"
   ```
4. Monitor Cycle 1 completion and Cycle 2 start:
   ```bash
   orchestrator task watch <task_id>
   ```
5. After completion, check DB:
   ```bash
   sqlite3 data/orchestrator.db "SELECT qa_file_path, source FROM task_items WHERE task_id='<task_id>' AND source='dynamic';"
   ```

### Expected
- Cycle 1: plan → qa_doc_gen → implement → self_test → self_restart completes
- Pipeline var `qa_doc_gen_output` is persisted to DB before restart
- Dynamic items are created in DB before restart (via `flush_pending_generate_items`)
- Cycle 2: item-scoped segment (qa_testing) fans out only over dynamic items
- Task completes successfully

### Notes
- This scenario requires `self_restart` builtin to be enabled. In mock mode, `self_restart` may be a no-op depending on the mock agent. The key verification is that pipeline vars and dynamic items survive across cycles — check DB state between cycles.
- If `self_restart` is not applicable in mock mode, verify the generate_items path using Scenario 1 instead.

---

## Scenario 3: Plan Step Does Not Create Files in User Workspace

### Preconditions
- Daemon running with `--workers 2`
- Self-bootstrap mock bundle applied

### Goal
Verify that the `plan` step outputs its plan to stdout only and does not create any files in the user workspace (e.g., `docs/plan/*.md`).

### Steps
1. Record baseline file list:
   ```bash
   ls docs/plan/ 2>/dev/null | sort > /tmp/plan-before.txt
   ```
2. Reset and run a self-bootstrap task:
   ```bash
   orchestrator delete --all --yes
   orchestrator init
   orchestrator apply -f fixtures/manifests/bundles/self-bootstrap-mock.yaml
   orchestrator task create --project self-bootstrap-mock --workflow self-bootstrap --goal "test plan step file creation"
   ```
3. Wait for Cycle 1 plan step to complete:
   ```bash
   orchestrator task watch <task_id>
   ```
4. Check for new files:
   ```bash
   ls docs/plan/ 2>/dev/null | sort > /tmp/plan-after.txt
   diff /tmp/plan-before.txt /tmp/plan-after.txt
   ```

### Expected
- No new files appear in `docs/plan/` after the plan step executes
- The plan output is captured in the pipeline variable `plan_output` (check via DB or trace)
- `diff` shows no differences

### Notes
- The plan step template prompt explicitly instructs agents: "Output ONLY a plan document to stdout — do NOT write any code or create any files. Do NOT use the Write or Edit tools."
- Mock agents use `echo` commands which inherently write only to stdout, so this scenario may always pass with mocks. The real value is verifying with live agents where the prompt constraint matters.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | generate_items narrows qa_testing to regression targets | | | | Fixture: `generate-items-narrow-test.yaml` |
| 2 | generate_items survives self_restart (2-cycle) | | | | Fixture: `self-bootstrap-mock.yaml` |
| 3 | Plan step does not create files in workspace | | | | Prompt constraint verification |
