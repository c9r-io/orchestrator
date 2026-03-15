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
- C/S mode: daemon-embedded workers replace standalone worker lifecycle commands

Entry point: `orchestrator` (CLI client) or `orchestratord` (daemon)

**C/S mode note**: Scenarios 4 and 5 can also be validated through the C/S architecture where `orchestratord --workers N` embeds the worker loop directly in the daemon process. See `docs/qa/orchestrator/53-client-server-architecture.md` for dedicated C/S scenarios.

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
- **Project `qa-plain` must be freshly deleted and re-applied** (step 1 below) to clear any stale daemon health state from prior runs. Skipping the delete will cause "No healthy agent found" if the agent was diseased previously.
- A QA-capable agent/template exists that prints plain text (non-JSON) for `qa`.

### Goal
Verify strict-mode validation fails phase output when `qa` stdout is not JSON.

### Steps
1. **Reset** and apply the plain-text-agent fixture into project scope (delete is mandatory to clear stale health state):
   ```bash
   orchestrator delete project/qa-plain --force
   orchestrator apply -f fixtures/manifests/bundles/plain-text-agent.yaml --project qa-plain
   ```
2. Create and run a task that uses non-JSON `qa` output:
   ```bash
   orchestrator task create --project qa-plain --workflow plain_text_test
   ```
3. Check validation failure event:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT event_type, payload_json FROM events
      WHERE task_id='${TASK_ID}' AND event_type='output_validation_failed'
      ORDER BY id DESC LIMIT 5;"
   ```

### Expected
- At least one `output_validation_failed` event is present.
- Corresponding phase run has `validation_status='failed'` and `exit_code=-6`.

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| Agents from another project selected instead of `plain_text_agent` | Fixture not applied with `--project`, or task created under the wrong project | Use `apply -f ... --project qa-plain` and create the task with `--project qa-plain` |
| Task fails with "No healthy agent found" after first few items | Agent marked diseased after consecutive validation failures | Expected behavior — strict validation correctly fails non-JSON output, and health system diseases the agent after 2 consecutive errors |
| Task immediately fails with "No healthy agent found" before any runs | Agent exists in config but was diseased from a prior run, or daemon health state is stale | Delete and re-apply the project (`orchestrator delete project/qa-plain --force` then re-apply) to clear stale health/lifecycle state. Verify with `orchestrator get agents --project qa-plain` before creating the task |
| Suspected missing `metadata.cost` causes selection failure | Red herring — missing cost defaults to 50 internally (`cost.unwrap_or(50)`); cost only affects scoring, never filtering | No fix needed. If agent is not selected, check health state (diseased from prior run) rather than cost config |

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
   orchestrator task start {task_id} || true
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

## Scenario 4: Queue-Only Lifecycle Enqueues Tasks

### Preconditions
- Runtime initialized and config applied.

### Goal
Verify task lifecycle commands no longer execute inline and always enqueue work for daemon processing.

### Steps
1. Create a task:
   ```bash
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --name "queue-create" --goal "queue" | grep -oE '[0-9a-f-]{36}' | head -1)
   ```
2. Enqueue an existing task explicitly:
   ```bash
   orchestrator task start "${TASK_ID}"
   ```
3. Query queue and scheduling events:
   ```bash
   orchestrator task info "${TASK_ID}" -o json
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
1. Start the daemon with multiple embedded workers in terminal A:
   ```bash
   ./target/release/orchestratord --foreground --workers 3
   ```
2. In terminal B, monitor queue and task progress:
   ```bash
   orchestrator task list -o json
   orchestrator task watch "${TASK_ID}" --interval 1
   ```
3. Stop the daemon after the queue drains:
   ```bash
   kill "${DAEMON_PID}"
   ```
4. Wait for daemon process to fully exit:
   ```bash
   while kill -0 "${DAEMON_PID}" 2>/dev/null; do sleep 1; done
   ```

### Expected
- Worker consumes pending tasks and updates task status to terminal state.
- Pending queue claim is atomic under parallel consumers (no duplicate pending-task execution).
- Stopping the daemon terminates embedded workers gracefully.

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| `stop_signal: true` after worker exits | Worker exited with error before cleanup ran | Fixed: cleanup now runs before error propagation. If still seen, check for process crash. |

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
