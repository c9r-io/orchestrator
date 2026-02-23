# Orchestrator - Scheduler Repository Refactor Regression

**Module**: orchestrator
**Scope**: Validate P0/P1 refactor outcomes for task summary mapping, repository-backed lifecycle data flow, and log error observability
**Scenarios**: 5
**Priority**: High

---

## Background

This document validates internal scheduler refactor outcomes that are externally observable from CLI and SQLite:

- `load_task_summary` timestamp mapping correctness
- repository-backed lifecycle/status transitions
- command run persistence after async-path refactor
- `task logs` behavior when log files are missing (explicit error, no silent fallback)

Entry point: `./scripts/orchestrator.sh`

---

## Database Schema Reference

### Table: tasks
| Column | Type | Notes |
|--------|------|-------|
| id | TEXT | Task ID |
| status | TEXT | `pending/running/paused/completed/failed` |
| workflow_id | TEXT | Workflow identifier |
| created_at | TEXT | Task created timestamp |
| updated_at | TEXT | Task updated timestamp |

### Table: command_runs
| Column | Type | Notes |
|--------|------|-------|
| id | TEXT | Run ID |
| task_item_id | TEXT | FK to `task_items.id` |
| phase | TEXT | `qa/fix/retest/guard/...` |
| stdout_path | TEXT | stdout file path |
| stderr_path | TEXT | stderr file path |
| started_at | TEXT | run start timestamp |
| ended_at | TEXT | run end timestamp |
| output_json | TEXT | Structured `AgentOutput` payload |
| artifacts_json | TEXT | Structured artifact payload |
| confidence | REAL | Parsed confidence value |
| quality_score | REAL | Parsed quality score value |
| validation_status | TEXT | Structured output validation result |

---

## Scenario 1: Task Summary Timestamp Mapping Correctness

### Preconditions
- Clean runtime state.
- Manifest applied with at least one QA target file.

### Goal
Ensure summary timestamps are sourced from `tasks.created_at` and `tasks.updated_at`, not shifted columns.

### Steps
1. Initialize and apply fixture:
   ```bash
   QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
   ./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
   ./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
   ./scripts/orchestrator.sh init
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/echo-workflow.yaml
   ```
2. Create task without auto-start:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --name "ts-map" --goal "timestamp mapping" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ```
3. Query DB source-of-truth:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT workflow_id, created_at, updated_at FROM tasks WHERE id='${TASK_ID}';"
   ```
4. Query CLI summary:
   ```bash
   ./scripts/orchestrator.sh task info "${TASK_ID}" -o json
   ```

### Expected
- `task.info.workflow_id` equals DB `workflow_id`.
- `task.info.created_at` equals DB `created_at`.
- `task.info.updated_at` equals DB `updated_at`.

### Expected Data State
```sql
SELECT id, workflow_id, created_at, updated_at
FROM tasks
WHERE id = '{task_id}';
-- Expected: one row; created_at/updated_at are non-empty and match CLI output fields
```

---

## Scenario 2: Start Preparation Transaction Resets Failed-Unresolved Items

### Preconditions
- One task exists with at least one task_item.

### Goal
Verify transactional start preparation resets unresolved items when task status is `failed`.

### Steps
1. Create a task:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --name "prep-reset" --goal "reset unresolved" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ```
2. Simulate failed/unresolved state:
   ```bash
   sqlite3 data/agent_orchestrator.db "UPDATE tasks SET status='failed' WHERE id='${TASK_ID}';"
   sqlite3 data/agent_orchestrator.db "UPDATE task_items SET status='unresolved', fix_required=1, fixed=1, last_error='x' WHERE task_id='${TASK_ID}';"
   ```
3. Start task:
   ```bash
   ./scripts/orchestrator.sh task start "${TASK_ID}" || true
   ```
4. Inspect task items:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT status, fix_required, fixed FROM task_items WHERE task_id='${TASK_ID}' LIMIT 5;"
   ```

### Expected
- Before run loop proceeds, unresolved items are reset to `pending`, `fix_required=0`, `fixed=0`.
- Task status enters `running` (then may finish depending on workflow).

### Expected Data State
```sql
SELECT status, fix_required, fixed
FROM task_items
WHERE task_id = '{task_id}';
-- Expected: rows previously unresolved are now pending/0/0 before subsequent phase execution
```

---

## Scenario 3: Command Run Persistence Remains Complete After Refactor

### Preconditions
- Echo workflow fixture applied.

### Goal
Ensure `command_runs` records persist both legacy execution fields and structured output fields after scheduler mainline integration.

### Steps
1. Create and start task:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --name "run-persist" --goal "persist command runs" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/orchestrator.sh task start "${TASK_ID}" || true
   ```
2. Verify run records:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT COUNT(*) FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='${TASK_ID}');"
   sqlite3 data/agent_orchestrator.db "SELECT phase, stdout_path, stderr_path, validation_status, confidence, quality_score, started_at, ended_at FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='${TASK_ID}') ORDER BY started_at DESC LIMIT 5;"
   ```

### Expected
- `command_runs` row count is greater than zero.
- `phase/stdout_path/stderr_path/started_at` are populated.
- `validation_status` is populated and `output_json`/`artifacts_json` are persisted.

### Expected Data State
```sql
SELECT phase, validation_status, output_json, artifacts_json
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}');
-- Expected: >= 1 row with non-empty phase/validation_status and structured JSON payload fields
```

---

## Scenario 4: Task Logs Missing File Is Explicitly Observable

### Preconditions
- A task exists with at least one `command_runs` row.

### Goal
Verify missing log files are reported as explicit errors instead of silent empty output.

### Steps
1. Create/start a task to produce runs:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --name "log-obs" --goal "log observability" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/orchestrator.sh task start "${TASK_ID}" || true
   ```
2. Corrupt one run path:
   ```bash
   sqlite3 data/agent_orchestrator.db "UPDATE command_runs SET stdout_path='/tmp/nonexistent-log-file.out' WHERE id=(SELECT id FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='${TASK_ID}') ORDER BY started_at DESC LIMIT 1);"
   ```
3. Read logs:
   ```bash
   ./scripts/orchestrator.sh task logs "${TASK_ID}"
   ```

### Expected
- CLI returns non-zero or visible error text indicating log file read failure.
- Output includes path/context rather than silently printing an empty chunk.

### Expected Data State
```sql
SELECT id, stdout_path
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}')
ORDER BY started_at DESC
LIMIT 1;
-- Expected: stdout_path points to intentionally missing file for negative-path validation
```

---

## Scenario 5: Task Delete Cleans Persistent Task Graph

### Preconditions
- A task with task_items and command_runs exists.

### Goal
Verify task deletion still removes dependent records after repository extraction.

### Steps
1. Create/start a task:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --name "delete-clean" --goal "delete cleanup" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/orchestrator.sh task start "${TASK_ID}" || true
   ```
2. Delete task:
   ```bash
   ./scripts/orchestrator.sh task delete "${TASK_ID}" --force
   ```
3. Validate DB cleanup:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT COUNT(*) FROM tasks WHERE id='${TASK_ID}';"
   sqlite3 data/agent_orchestrator.db "SELECT COUNT(*) FROM task_items WHERE task_id='${TASK_ID}';"
   sqlite3 data/agent_orchestrator.db "SELECT COUNT(*) FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='${TASK_ID}');"
   sqlite3 data/agent_orchestrator.db "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}';"
   ```

### Expected
- All counts are `0`.
- No orphaned task graph records remain for deleted task.

### Expected Data State
```sql
SELECT
  (SELECT COUNT(*) FROM tasks WHERE id = '{task_id}') AS task_count,
  (SELECT COUNT(*) FROM task_items WHERE task_id = '{task_id}') AS item_count,
  (SELECT COUNT(*) FROM events WHERE task_id = '{task_id}') AS event_count;
-- Expected: task_count=0, item_count=0, event_count=0
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Task Summary Timestamp Mapping Correctness | ☐ | | | |
| 2 | Start Preparation Transaction Resets Failed-Unresolved Items | ☐ | | | |
| 3 | Command Run Persistence Remains Complete After Refactor | ☐ | | | |
| 4 | Task Logs Missing File Is Explicitly Observable | ☐ | | | |
| 5 | Task Delete Cleans Persistent Task Graph | ☐ | | | |
