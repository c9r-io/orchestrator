# Orchestrator - Structured Output Mainline and Worker Scheduler

**Module**: orchestrator
**Scope**: Validate strict structured output enforcement, command_runs structured persistence, and detach/worker scheduling flow
**Scenarios**: 5
**Priority**: High

---

## Background

This document validates the refactor that moved `collab` capabilities into the scheduler main path:

- strict JSON output validation for `qa`/`fix`/`retest`/`guard`
- structured output persistence in `command_runs`
- phase execution result publication to MessageBus with observable events
- dual CLI model: foreground run and detach queue + worker loop

Entry point: `./scripts/orchestrator.sh`

---

## Database Schema Reference

### Table: command_runs
| Column | Type | Notes |
|--------|------|-------|
| output_json | TEXT | Serialized `AgentOutput` |
| artifacts_json | TEXT | Serialized artifact list |
| confidence | REAL | Parsed confidence value |
| quality_score | REAL | Parsed quality score value |
| validation_status | TEXT | `passed` / `failed` / `unknown` |

### Table: events
| Column | Type | Notes |
|--------|------|-------|
| event_type | TEXT | Includes `output_validation_failed`, `phase_output_published`, `scheduler_enqueued` |
| payload_json | TEXT | Event payload details |

---

## Scenario 1: Strict Validation Rejects Non-JSON QA Output

### Preconditions
- Runtime initialized.
- Fixture bundle applied.
- A QA-capable agent/template exists that prints plain text (non-JSON) for `qa`.

### Goal
Verify strict-mode validation fails phase output when `qa` stdout is not JSON.

### Steps
1. Prepare an isolated project and apply config:
   ```bash
   QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
   ./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
   ./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/echo-workflow.yaml
   ```
2. Create and run a task that uses non-JSON `qa` output:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --name "strict-json-fail" --goal "strict validation" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/orchestrator.sh task start "${TASK_ID}" || true
   ```
3. Check validation failure event:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT event_type, payload_json FROM events WHERE task_id='${TASK_ID}' AND event_type='output_validation_failed' ORDER BY id DESC LIMIT 5;"
   ```

### Expected
- At least one `output_validation_failed` event is present.
- Corresponding phase run has `validation_status='failed'`.

### Expected Data State
```sql
SELECT phase, validation_status
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}')
ORDER BY started_at DESC;
-- Expected: at least one row with phase='qa' and validation_status='failed'
```

---

## Scenario 2: Structured Output Persists Into command_runs

### Preconditions
- Runtime initialized.
- A QA-capable agent/template returns JSON with `confidence`, `quality_score`, and `artifacts`.

### Goal
Verify structured fields are persisted in `command_runs`.

### Steps
1. Execute one task using structured JSON output.
2. Query structured columns:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT validation_status, confidence, quality_score, substr(output_json,1,120), substr(artifacts_json,1,120) FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='{task_id}') ORDER BY started_at DESC LIMIT 5;"
   ```

### Expected
- `validation_status='passed'` for structured-output runs.
- `output_json` and `artifacts_json` are non-empty JSON payloads.
- `confidence` and `quality_score` are populated when provided by output.

### Expected Data State
```sql
SELECT COUNT(*)
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}')
  AND validation_status = 'passed'
  AND output_json <> '{}'
  AND artifacts_json <> '[]';
-- Expected: count >= 1
```

---

## Scenario 3: Scheduler Publishes Phase Output Events

### Preconditions
- A task run exists with at least one phase execution.

### Goal
Verify phase outputs are published and observable in persisted events.

### Steps
1. Run a task to completion (or failure):
   ```bash
   ./scripts/orchestrator.sh task start {task_id} || true
   ```
2. Query phase publication events:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT event_type, payload_json FROM events WHERE task_id='{task_id}' AND event_type='phase_output_published' ORDER BY id DESC LIMIT 10;"
   ```

### Expected
- `phase_output_published` events are present with `phase` and `run_id` in payload.
- For validation-failed runs, `output_validation_failed` and `phase_output_published` can both be observed for traceability.

### Expected Data State
```sql
SELECT COUNT(*)
FROM events
WHERE task_id = '{task_id}'
  AND event_type = 'phase_output_published';
-- Expected: count >= 1
```

---

## Scenario 4: Detach Mode Enqueues Tasks

### Preconditions
- Runtime initialized and config applied.

### Goal
Verify `--detach` no longer executes task inline and enqueues it for worker processing.

### Steps
1. Create a task in detach mode:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --name "detach-create" --goal "queue" --detach | grep -oE '[0-9a-f-]{36}' | head -1)
   ```
2. Enqueue an existing task explicitly:
   ```bash
   ./scripts/orchestrator.sh task start "${TASK_ID}" --detach
   ```
3. Query queue and scheduling events:
   ```bash
   ./scripts/orchestrator.sh task worker status
   sqlite3 data/agent_orchestrator.db "SELECT event_type FROM events WHERE task_id='${TASK_ID}' AND event_type='scheduler_enqueued' ORDER BY id DESC LIMIT 5;"
   ```

### Expected
- Task status remains `pending` until a worker consumes it.
- `scheduler_enqueued` event exists.

### Expected Data State
```sql
SELECT status
FROM tasks
WHERE id = '{task_id}';
-- Expected: 'pending' before worker consumption
```

---

## Scenario 5: Worker Start/Stop and Queue Consumption

### Preconditions
- At least one pending task exists.

### Goal
Verify worker loop consumes pending tasks and honors stop signal.

### Steps
1. Start worker in terminal A:
   ```bash
   ./scripts/orchestrator.sh task worker start --poll-ms 500
   ```
2. In terminal B, monitor queue:
   ```bash
   ./scripts/orchestrator.sh task worker status
   ./scripts/orchestrator.sh task list -o json
   ```
3. Stop worker:
   ```bash
   ./scripts/orchestrator.sh task worker stop
   ```
4. Confirm stop signal cleared after worker exits:
   ```bash
   ./scripts/orchestrator.sh task worker status
   ```

### Expected
- Worker consumes pending tasks and updates task status to terminal state.
- `task worker stop` triggers graceful loop termination.
- `stop_signal` returns `false` after worker exits and clears marker file.

### Expected Data State
```sql
SELECT id, status
FROM tasks
WHERE id = '{task_id}';
-- Expected: status transitions from pending -> running -> completed/failed
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Strict Validation Rejects Non-JSON QA Output | ☐ | | | |
| 2 | Structured Output Persists Into command_runs | ☐ | | | |
| 3 | Scheduler Publishes Phase Output Events | ☐ | | | |
| 4 | Detach Mode Enqueues Tasks | ☐ | | | |
| 5 | Worker Start/Stop and Queue Consumption | ☐ | | | |
