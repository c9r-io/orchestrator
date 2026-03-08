# Self-Bootstrap Tests - Scenario 2: Binary Snapshot Restoration on Auto-Rollback

**Module**: self-bootstrap  
**Scenario**: Binary Snapshot Restoration on Auto-Rollback  
**Status**: IN PROGRESS  
**Test Date**: 2026-03-05  
**Tester**: QA Bot

---

## Goal
Verify that when auto-rollback triggers (after max consecutive failures), the `.stable` binary is restored over the live release binary.

---

### Preconditions

> **IMPORTANT: Must use mock fixture — never use `docs/workflow/self-bootstrap.yaml` (real Claude agents).**
> See parent doc `01-survival-binary-checkpoint-self-test.md` for full Common Preconditions.

```bash
rm -f fixtures/ticket/auto_*.md
orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml
QA_PROJECT="qa-survival"
orchestrator qa project reset "${QA_PROJECT}" --force
orchestrator apply -f fixtures/manifests/bundles/self-bootstrap-mock.yaml --project "${QA_PROJECT}"
```

- ✅ Common Preconditions applied (qa-survival project, **mock** self-bootstrap workflow)
- ✅ `.stable` binary exists (from previous successful snapshot)
- ✅ Workspace has `auto_rollback: true`, `checkpoint_strategy: git_tag`, `binary_snapshot: true`, `max_consecutive_failures: 3`

### Steps
1. Ensure `.stable` binary exists:
   ```bash
   cp target/release/orchestratord .stable
   ```
2. Create a task that will fail repeatedly (introduce a compile error in the implement step output)
3. Start the task and wait for 3 consecutive failures to trigger auto-rollback
4. Query events for `auto_rollback` and `binary_snapshot_restored`

---

### Current Progress
1. ✅ Copying .stable binary from successful snapshot
2. 🔄 Preparing compile error scenario
3. ⏳ Awaiting task execution and failure sequence
4. ⏳ Will monitor auto-rollback event sequence

---

### Expected Results
- Event `auto_rollback` emitted after 3 consecutive failures
- Event `binary_snapshot_restored` emitted in same cycle as auto-rollback
- Release binary at `target/release/orchestratord` matches `.stable` file
- `consecutive_failures` counter reset to 0 after rollback

---

### Database Validation (Expected)
```sql
SELECT event_type, json_extract(payload_json, '$.cycle') AS cycle
FROM events 
WHERE task_id = '{task_id}' 
  AND event_type IN ('auto_rollback', 'binary_snapshot_restored')
ORDER BY created_at;
-- Expected: auto_rollback followed by binary_snapshot_restored, same cycle
```

---

## Checklist

- [ ] `.stable` binary exists before task starts
- [ ] Task fails 3 consecutive times triggering auto-rollback
- [ ] `auto_rollback` event emitted
- [ ] `binary_snapshot_restored` event emitted in same cycle
- [ ] Release binary matches `.stable` file after rollback
- [ ] `consecutive_failures` counter reset to 0