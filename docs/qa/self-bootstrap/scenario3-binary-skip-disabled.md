# Self-Bootstrap Tests - Scenario 3: Binary Snapshot Skip When Disabled

**Module**: self-bootstrap  
**Scenario**: Binary Snapshot Skip When Disabled  
**Status**: IN PROGRESS  
**Test Date**: 2026-03-05  
**Tester**: QA Bot

---

## Goal
Verify that binary snapshot is NOT created when `binary_snapshot: false` or when the workspace is not `self_referential`.

---

### Preconditions

> **IMPORTANT: Must use mock fixture — never use `docs/workflow/self-bootstrap.yaml` (real Claude agents).**
> See parent doc `01-survival-binary-checkpoint-self-test.md` for full Common Preconditions.

```bash
rm -f fixtures/ticket/auto_*.md
orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml
QA_PROJECT="qa-survival"
orchestrator project reset "${QA_PROJECT}" --force --include-config
orchestrator apply -f fixtures/manifests/bundles/self-bootstrap-mock.yaml --project "${QA_PROJECT}"
```

- ✅ Common Preconditions applied (qa-survival project, **mock** self-bootstrap workflow)
- ✅ Release binary exists at `target/release/orchestratord`
- ✅ `.stable` file may exist from previous tests

### Steps
1. Apply a workflow manifest with `binary_snapshot: false` (or omit the field, default is false)
2. Create and start a task
3. Wait for the first cycle checkpoint
4. Query events for `binary_snapshot_created`

---

### Current Progress
1. ✅ Environment prepared (clean compile error, binary rebuilt)
2. 🔄 Applying workflow with binary_snapshot disabled
3. ⏳ Will create task and monitor cycle execution
4. ⏳ Will verify no binary snapshot created

---

### Expected Results
- No `binary_snapshot_created` event exists for this task
- No `.stable` file is created (or if it existed before, it is not updated)
- `checkpoint_created` event still fires normally (git tag checkpoint is independent)

---

### Expected Data State
```sql
SELECT COUNT(*) FROM events
WHERE task_id = '{task_id}' AND event_type = 'binary_snapshot_created';
-- Expected: 0
```

---

## Checklist

- [ ] Workflow applied with `binary_snapshot: false`
- [ ] Task completes at least one cycle
- [ ] No `binary_snapshot_created` event exists for this task
- [ ] `.stable` file not created or updated
- [ ] `checkpoint_created` event still fires normally