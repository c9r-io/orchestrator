---
self_referential_safe: false
---

# Self-Bootstrap - Cycle 2 Validation Chain & Runtime Timestamps

**Module**: self-bootstrap
**Scope**: Regression coverage for fixed two-cycle validation flow and task/item runtime timestamps
**Scenarios**: 2
**Priority**: High

---

## Background

The `self-bootstrap` workflow in `fixtures/manifests/bundles/self-bootstrap-mock.yaml` runs in fixed two-cycle mode:

```text
Cycle 1: plan -> qa_doc_gen -> implement -> self_test -> self_restart (rebuild + exec() hot reload)
Cycle 2: implement -> self_test -> [self_restart skipped: repeatable=false] -> qa_testing -> ticket_fix(if tickets) -> align_tests -> doc_governance
```

This document verifies two regressions:

1. The final-cycle item-scoped validation chain is actually executed instead of being silently short-circuited.
2. `tasks.started_at/completed_at` and `task_items.started_at/completed_at` are persisted during the real run.

Reusable automation:

```bash
./scripts/qa/test-self-bootstrap-cycle2-regression.sh
```

### Common Preconditions

```bash
cargo build --release -p orchestratord -p orchestrator-cli
test -f data/agent_orchestrator.db || orchestrator init

QA_PROJECT="qa-cycle2-${USER}-$(date +%Y%m%d%H%M%S)"
orchestrator apply -f fixtures/manifests/bundles/self-bootstrap-mock.yaml
orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
orchestrator apply -f fixtures/manifests/bundles/self-bootstrap-mock.yaml --project "${QA_PROJECT}"
```

---

## Scenario 1: Cycle 2 Executes QA Validation Chain

### Preconditions
- Common Preconditions applied

### Steps
1. Create and start a `self-bootstrap` task:
   ```bash
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workflow self-bootstrap --target-file docs/qa/self-bootstrap/04-cycle2-validation-and-runtime-timestamps.md --goal "verify cycle2 validation chain" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}"
   ```
2. Confirm the task reaches a terminal state:
   ```bash
   orchestrator task info "${TASK_ID}" -o json | jq '.task.status'
   ```
3. Query persisted events for the final-cycle validation chain:
   ```bash
   sqlite3 data/agent_orchestrator.db "
   SELECT event_type,
          json_extract(payload_json, '$.step') AS step,
          json_extract(payload_json, '$.cycle') AS cycle,
          json_extract(payload_json, '$.reason') AS reason
   FROM events
   WHERE task_id='${TASK_ID}'
     AND event_type IN ('step_started','step_skipped','item_validation_missing')
     AND json_extract(payload_json, '$.step') IN ('qa_testing','ticket_fix','align_tests','doc_governance')
   ORDER BY created_at;"
   ```

### Expected
- `qa_testing` has a `step_started` event in Cycle 2.
- `align_tests` and `doc_governance` have `step_started` events after `qa_testing`.
- `ticket_fix` is either:
  - `step_started` when `qa_testing` creates tickets, or
  - `step_skipped` with `reason = "prehook_false"` when no tickets are present.
- No `item_validation_missing` event exists for the task.

### Expected Data State
```sql
SELECT COUNT(*)
FROM events
WHERE task_id = '{task_id}'
  AND event_type = 'step_started'
  AND json_extract(payload_json, '$.step') = 'qa_testing';
-- Expected: >= 1

SELECT COUNT(*)
FROM events
WHERE task_id = '{task_id}'
  AND event_type = 'item_validation_missing';
-- Expected: 0
```

---

## Scenario 2: Task and Item Runtime Timestamps Persist

### Preconditions
- Scenario 1 completed with a real task id

### Steps
1. Inspect task-level timestamps:
   ```bash
   orchestrator task info "${TASK_ID}" -o json | jq '{task: .task | {status, started_at, completed_at}, items: [.items[] | {id, status, started_at, completed_at}]}'
   ```
2. Query SQLite directly for the same fields:
   ```bash
   sqlite3 data/agent_orchestrator.db "
   SELECT 'task' AS kind, id, status, started_at, completed_at
   FROM tasks
   WHERE id='${TASK_ID}'
   UNION ALL
   SELECT 'item' AS kind, id, status, started_at, completed_at
   FROM task_items
   WHERE task_id='${TASK_ID}'
   ORDER BY kind DESC, id ASC;"
   ```

### Expected
- Task `started_at` is non-null once the run begins.
- Task `completed_at` is non-null after the task reaches `completed` or `failed`.
- Every task item has non-null `started_at`.
- Every finalized task item has non-null `completed_at`.

### Expected Data State
```sql
SELECT started_at IS NOT NULL, completed_at IS NOT NULL
FROM tasks
WHERE id = '{task_id}';
-- Expected: 1 | 1

SELECT COUNT(*)
FROM task_items
WHERE task_id = '{task_id}'
  AND (started_at IS NULL OR completed_at IS NULL);
-- Expected: 0
```

---

## Checklist

| # | Scenario | Status | Date | Tester | Notes |
|---|----------|--------|------|--------|-------|
| 1 | Cycle 2 Executes QA Validation Chain | ☐ | | | |
| 2 | Task and Item Runtime Timestamps Persist | ☐ | | | |
