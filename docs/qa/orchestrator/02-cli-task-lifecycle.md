# Orchestrator - CLI Task Lifecycle

**Module**: orchestrator
**Scope**: Validate foreground task execution, detach queue mode, worker lifecycle control, logs, and retry
**Scenarios**: 5
**Priority**: High

---

## Background

This document validates task lifecycle behavior after scheduler refactor:

- foreground execution path (`task start/resume/retry`)
- detached queue execution (`--detach`)
- worker commands (`task worker start|stop|status`)
- task logs and retry behavior

Task creation target resolution now follows workflow scope:

- item-scoped workflows still default to scanning workspace `qa_targets` when `--target-file` is omitted
- task-scoped-only workflows use a synthetic `__UNASSIGNED__` anchor when `--target-file` is omitted
- any explicit `--target-file` values override the default source
- multiple explicit targets are only valid for workflows that include item-scoped steps

Entry point: `./scripts/orchestrator.sh task <command>`

### Project Isolation Setup

Run once before scenarios:

```bash
QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/output-formats.yaml
```

### Target Resolution Supplemental Checks

Before Scenario 1, also verify `task create --project <project> ...` target resolution in an isolated app root with a minimal config:

1. Prepare isolated runtime and apply a task-scoped-only workflow plus an item-scoped workflow.
2. For the task-scoped-only workflow:
   - omit `--target-file`, confirm `task create --project <project> --no-start` succeeds
   - pass one `--target-file`, confirm success
   - pass two `--target-file`, confirm the command fails
3. For the item-scoped workflow:
   - omit `--target-file`, confirm the command fails when `qa_targets` is empty
   - pass one or more `--target-file`, confirm the command succeeds

Expected:
- Task-scoped-only workflows use a synthetic anchor when `--target-file` is omitted.
- Explicit `--target-file` overrides the default source.
- Multiple explicit targets are rejected only for task-scoped-only workflows.

---

## Scenario 1: Foreground Task Start

### Preconditions
- Runtime initialized and config applied.
- Task created with `--no-start`.

### Steps
1. Create task:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --name "fg-start" --goal "foreground" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ```
2. Start task in foreground:
   ```bash
   ./scripts/orchestrator.sh task start "${TASK_ID}" || true
   ```
3. Inspect result:
   ```bash
   ./scripts/orchestrator.sh task info "${TASK_ID}" -o json
   ```

### Expected
- Command blocks until run loop reaches terminal status.
- Task transitions through `running` to `completed` or `failed`.

### Expected Data State
```sql
SELECT status
FROM tasks
WHERE id = '{task_id}';
-- Expected: terminal status (completed/failed)
```

---

## Scenario 2: Detach Enqueue Mode

### Preconditions
- Runtime initialized.

### Goal
Verify `--detach` does not execute inline and enqueues task.

### Steps
1. Create in detach mode:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --name "detach-mode" --goal "queue" --detach | grep -oE '[0-9a-f-]{36}' | head -1)
   ```
2. Check task state:
   ```bash
   ./scripts/orchestrator.sh task info "${TASK_ID}" -o json
   ```
3. Re-enqueue with start detach:
   ```bash
   ./scripts/orchestrator.sh task start "${TASK_ID}" --detach
   ```

### Expected
- Task remains `pending` before worker consumption.
- `scheduler_enqueued` event is recorded.

### Expected Data State
```sql
SELECT status
FROM tasks
WHERE id = '{task_id}';
-- Expected: pending (before worker consumes)
```

---

## Scenario 3: Worker Start/Status/Stop

### Preconditions
- At least one pending task exists.

### Steps
1. Start worker in terminal A:
   ```bash
   ./scripts/orchestrator.sh task worker start --poll-ms 500
   ```
   Optional (parallel consumers):
   ```bash
   ./scripts/orchestrator.sh task worker start --poll-ms 500 --workers 3
   ```
2. In terminal B, check status:
   ```bash
   ./scripts/orchestrator.sh task worker status
   ```
3. Stop worker:
   ```bash
   ./scripts/orchestrator.sh task worker stop
   ```
4. Re-check status:
   ```bash
   ./scripts/orchestrator.sh task worker status
   ```

### Expected
- Worker consumes pending tasks while running.
- With `--workers N`, pending tasks can be consumed concurrently by N consumers.
- Stop signal terminates worker loop gracefully.
- `task worker status` reflects pending count and stop-signal state.

### Expected Data State
```sql
SELECT event_type
FROM events
WHERE task_id = '{task_id}'
  AND event_type = 'scheduler_enqueued'
ORDER BY id DESC
LIMIT 5;
-- Expected: enqueue events exist for detached submissions
```

---

## Scenario 4: Task Logs

### Preconditions
- A task has executed at least one phase (`command_runs` exists).

### Steps
1. View logs:
   ```bash
   ./scripts/orchestrator.sh task logs {task_id}
   ```
2. View last lines:
   ```bash
   ./scripts/orchestrator.sh task logs {task_id} --tail 10
   ```
3. View with timestamps:
   ```bash
   ./scripts/orchestrator.sh task logs {task_id} --timestamps
   ```

### Expected
- Logs show run output chunks grouped by phase/run id.
- Missing/corrupted log paths produce explicit read errors.
- Tail and timestamp flags behave as documented.

### Expected Data State
```sql
SELECT phase, stdout_path, stderr_path
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}')
ORDER BY started_at DESC;
-- Expected: non-empty rows for executed task
```

---

## Scenario 5: Task Retry (Foreground and Detach)

### Preconditions
- A task has at least one failed or unresolved item.

### Steps
1. Find retry target item:
   ```bash
   ./scripts/orchestrator.sh task info {task_id} -o json
   ```
2. Retry in foreground:
   ```bash
   ./scripts/orchestrator.sh task retry {task_item_id} || true
   ```
3. Retry in detach mode:
   ```bash
   ./scripts/orchestrator.sh task retry {task_item_id} --detach
   ```

### Expected
- Foreground retry runs immediately and returns terminal result.
- Detach retry enqueues associated task and returns without inline execution.

### Expected Data State
```sql
SELECT status, updated_at
FROM task_items
WHERE id = '{task_item_id}';
-- Expected: status/updated_at changed after retry execution
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Foreground Task Start | ☐ | | | |
| 2 | Detach Enqueue Mode | ☐ | | | |
| 3 | Worker Start/Status/Stop | ☐ | | | |
| 4 | Task Logs | ☐ | | | |
| 5 | Task Retry (Foreground and Detach) | ☐ | | | |
