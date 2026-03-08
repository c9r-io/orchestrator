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
- worker commands (`task worker start|stop|status`) — standalone mode
- daemon-embedded workers (`orchestratord --workers N`) — C/S mode
- task logs and retry behavior

Task creation target resolution now follows workflow scope:

- item-scoped workflows still default to scanning workspace `qa_targets` when `--target-file` is omitted
- task-scoped-only workflows use a synthetic `__UNASSIGNED__` anchor when `--target-file` is omitted
- any explicit `--target-file` values override the default source
- multiple explicit targets are only valid for workflows that include item-scoped steps

Runtime control commands also need to remain stable while a task is actively running:

- `task info` should return valid output repeatedly during execution instead of failing on transient reads
- `task logs` should return partial output when some log files are temporarily unavailable
- `task watch` should keep the last visible frame until a fresh snapshot is ready
- `task watch` should display the real step scope instead of inferring it from an anchor item binding

Entry point: `orchestrator task <command>` (standalone) or `./target/release/orchestrator task <command>` (C/S client)

**C/S mode note**: In C/S mode, the daemon (`orchestratord`) embeds background workers that automatically consume pending tasks. The standalone `task worker start|stop|status` commands are not needed — use `orchestratord --workers N` instead. See `docs/qa/orchestrator/53-client-server-architecture.md` for C/S-specific scenarios.

### Project Isolation Setup

Run once before scenarios:

```bash
QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
orchestrator apply -f fixtures/manifests/bundles/cli-probe-fixtures.yaml
orchestrator qa project reset "${QA_PROJECT}" --keep-config --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
orchestrator qa project create "${QA_PROJECT}" --from-workspace cli_probe_ws --workflow probe_task_scoped --force
```

### Target Resolution Supplemental Checks

**Automated regression**: Run the unified CLI probe regression runner to validate task-create target resolution in a single pass:

```bash
./scripts/regression/run-cli-probes.sh --group task-create
```

| Symptom | Likely Cause | Fix |
|---|---|---|
| `item-scoped` or `task-scoped` times out on first run but passes on retry | SQLite WAL contention between consecutive probe scenarios | Re-run the group; the inter-scenario cooldown (commit `85a5954`) prevents this in most cases |

For manual verification, verify `task create --project <project> ...` target resolution using the fixed probe fixtures:

1. For the task-scoped workflow on the populated workspace:
   - `orchestrator task create --project "${QA_PROJECT}" --workspace cli_probe_ws --workflow probe_task_scoped --name "task-default" --goal "task default" --no-start`
   - `orchestrator task create --project "${QA_PROJECT}" --workspace cli_probe_ws --workflow probe_task_scoped --name "task-single" --goal "task single" --target-file fixtures/qa-probe-targets/sample-a.md --no-start`
   - `orchestrator task create --project "${QA_PROJECT}" --workspace cli_probe_ws --workflow probe_task_scoped --name "task-multi" --goal "task multi" --target-file fixtures/qa-probe-targets/sample-a.md --target-file fixtures/qa-probe-targets/sample-b.md --no-start`
2. For the item-scoped workflow on the empty workspace:
   - `orchestrator task create --project "${QA_PROJECT}" --workspace cli_probe_empty_ws --workflow probe_item_scoped --name "item-empty" --goal "item empty" --no-start`
3. For the item-scoped workflow with explicit targets:
   - `orchestrator task create --project "${QA_PROJECT}" --workspace cli_probe_ws --workflow probe_item_scoped --name "item-explicit" --goal "item explicit" --target-file fixtures/qa-probe-targets/sample-a.md --target-file fixtures/qa-probe-targets/sample-b.md --no-start`

Expected:
- `probe_task_scoped` uses a synthetic anchor when `--target-file` is omitted.
- Explicit `--target-file` overrides the default source.
- Multiple explicit targets are rejected for `probe_task_scoped`.
- `probe_item_scoped` fails on `cli_probe_empty_ws` when `--target-file` is omitted.
- `probe_item_scoped` succeeds with one or more explicit targets.

### Runtime Control Supplemental Checks

**Automated regression**: Run the unified CLI probe regression runner to validate runtime control and low-output detection in a single pass:

```bash
./scripts/regression/run-cli-probes.sh --group runtime-control
./scripts/regression/run-cli-probes.sh --group low-output
```

For manual verification against a real in-flight task from the fixed probe fixtures:

1. Create a detached task that will run long enough to observe live state:
   ```bash
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workspace cli_probe_ws --workflow probe_runtime_control --name "runtime-control" --goal "runtime control validation" --detach | grep -oE '[0-9a-f-]{36}' | head -1)
   ```
2. Start a worker in another terminal and wait for the task to enter `running`.
3. While the task is still running:
   - run `orchestrator task info "${TASK_ID}" -o json` multiple times
   - run `orchestrator task logs "${TASK_ID}" --tail 20`
   - run `orchestrator task watch "${TASK_ID}" --interval 1`
4. Stop the worker after the task reaches a terminal state.

Expected:
- Repeated `task info` calls keep returning valid JSON and do not fail on transient reads.
- `task logs` succeeds even if some run logs are not yet readable, using per-run placeholders when needed.
- `task watch` renders a frame immediately and should not clear to a blank screen before data is available.
- `task watch` includes a `Scope` column and reports `task` vs `item` from explicit step metadata, not from whether an anchor item exists.
- For `probe_low_output`, `task watch` surfaces a `LOW_OUTPUT [INTERVENE]` indicator instead of only showing a live PID.
- For `probe_active_output`, `task watch` continues to show progress details without entering `LOW_OUTPUT`.

### Self-Referential Probe Safety Checks

These checks intentionally do not use `qa project create`, because that path
always creates non-self-referential workspaces.

Do not pair these probe checks with `db reset --include-config`; they must keep
the active runtime config intact and only apply the dedicated probe fixtures.

1. Apply the self-referential probe fixtures:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/self-referential-probe-fixtures.yaml
   ```
2. Submit a detached task directly against the global workspace `self_ref_probe_ws` using `self_ref_probe_runtime_control`.
3. Submit a detached task directly against the global workspace `self_ref_probe_ws` using `self_ref_probe_low_output`.
4. Submit a detached task directly against the global workspace `self_ref_probe_ws` using `self_ref_probe_active_output`.

Expected:
- The self-referential probe workflows create and run without requiring `self_test`.
- They do not need to borrow `build` or any strict-output phase.
- `self_ref_probe_low_output` surfaces `LOW_OUTPUT [INTERVENE]` during execution.
- `self_ref_probe_active_output` does not surface `LOW_OUTPUT`.

---

## Scenario 1: Foreground Task Start

### Preconditions
- Runtime initialized and config applied.
- Task created with `--no-start`.

### Steps
1. Create task (specify `--workflow` explicitly — `qa project create` copies the
   workspace/workflow into the project, but `task create` does not auto-resolve
   the project's workflow without the flag):
   ```bash
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workflow probe_task_scoped --name "fg-start" --goal "foreground" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ```
2. Start task in foreground:
   ```bash
   orchestrator task start "${TASK_ID}" || true
   ```
3. Inspect result:
   ```bash
   orchestrator task info "${TASK_ID}" -o json
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
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --name "detach-mode" --goal "queue" --detach | grep -oE '[0-9a-f-]{36}' | head -1)
   ```
2. Check task state:
   ```bash
   orchestrator task info "${TASK_ID}" -o json
   ```
3. Re-enqueue with start detach:
   ```bash
   orchestrator task start "${TASK_ID}" --detach
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
   orchestrator task worker start --poll-ms 500
   ```
   Optional (parallel consumers):
   ```bash
   orchestrator task worker start --poll-ms 500 --workers 3
   ```
2. In terminal B, check status:
   ```bash
   orchestrator task worker status
   ```
3. Stop worker:
   ```bash
   orchestrator task worker stop
   ```
4. Re-check status:
   ```bash
   orchestrator task worker status
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
   orchestrator task logs {task_id}
   ```
2. View last lines:
   ```bash
   orchestrator task logs {task_id} --tail 10
   ```
3. View with timestamps:
   ```bash
   orchestrator task logs {task_id} --timestamps
   ```

### Expected
- Logs show run output chunks grouped by phase/run id.
- Missing/corrupted log paths produce per-run placeholders instead of aborting the whole command.
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
   orchestrator task info {task_id} -o json
   ```
2. Attempt retry without `--force` (safety gate):
   ```bash
   orchestrator task retry {task_item_id} 2>&1; echo "exit=$?"
   ```
3. Retry in foreground with `--force`:
   ```bash
   orchestrator task retry {task_item_id} --force || true
   ```
4. Retry in detach mode with `--force`:
   ```bash
   orchestrator task retry {task_item_id} --detach --force
   ```

### Expected
- Without `--force`: prints warning to stderr and exits with code 1; no state change occurs.
- Foreground retry with `--force` runs immediately and returns terminal result.
- Detach retry with `--force` enqueues associated task and returns without inline execution.

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
