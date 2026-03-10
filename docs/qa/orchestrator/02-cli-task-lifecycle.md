# Orchestrator - CLI Task Lifecycle

**Module**: orchestrator
**Scope**: Validate queue-only task execution, daemon worker consumption, logs, and retry
**Scenarios**: 5
**Priority**: High

---

## Background

This document validates task lifecycle behavior after the C/S queue-only refactor:

- queue-only execution path for `task create/start/resume/retry`
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

**C/S mode note**: In C/S mode, the daemon (`orchestratord`) embeds background workers that automatically consume pending tasks. Use `orchestratord --workers N` plus `task info/watch/logs` for observation. See `docs/qa/orchestrator/53-client-server-architecture.md` for C/S-specific scenarios.

### Project Isolation Setup

Run once before scenarios:

```bash
QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
orchestrator apply -f fixtures/manifests/bundles/cli-probe-fixtures.yaml
orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
orchestrator apply -f fixtures/manifests/bundles/cli-probe-fixtures.yaml --project "${QA_PROJECT}"
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

1. Create a task that will run long enough to observe live state:
   ```bash
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workspace cli_probe_ws --workflow probe_runtime_control --name "runtime-control" --goal "runtime control validation" | grep -oE '[0-9a-f-]{36}' | head -1)
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

These checks intentionally do not use `apply --project`, because that path
always creates non-self-referential workspaces.

Do not pair these probe checks with `delete project/<name> --force`; they must keep
the active runtime config intact and only apply the dedicated probe fixtures.

1. Apply the self-referential probe fixtures:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/self-referential-probe-fixtures.yaml
   ```
2. Submit a task directly against the global workspace `self_ref_probe_ws` using `self_ref_probe_runtime_control`.
3. Submit a task directly against the global workspace `self_ref_probe_ws` using `self_ref_probe_low_output`.
4. Submit a task directly against the global workspace `self_ref_probe_ws` using `self_ref_probe_active_output`.

Expected:
- The self-referential probe workflows create and run without requiring `self_test`.
- They do not need to borrow `build` or any strict-output phase.
- `self_ref_probe_low_output` surfaces `LOW_OUTPUT [INTERVENE]` during execution.
- `self_ref_probe_active_output` does not surface `LOW_OUTPUT`.

---

## Scenario 1: Queue-Only Task Start

### Preconditions
- Runtime initialized and config applied.
- Task created with `--no-start`.

### Steps
1. Create task (specify `--workflow` explicitly — `apply --project` copies the
   workspace/workflow into the project, but `task create` does not auto-resolve
   the project's workflow without the flag):
   ```bash
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workflow probe_task_scoped --name "fg-start" --goal "foreground" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ```
2. Start task:
   ```bash
   orchestrator task start "${TASK_ID}"
   ```
3. Inspect result:
   ```bash
   orchestrator task info "${TASK_ID}" -o json
   ```

### Expected
- Command returns promptly with an enqueue message.
- Task transitions through `pending` to `running`, then to `completed` or `failed` after daemon worker consumption.

### Expected Data State
```sql
SELECT status
FROM tasks
WHERE id = '{task_id}';
-- Expected: terminal status (completed/failed)
```

---

## Scenario 2: Create/Start Enqueue Mode

### Preconditions
- Runtime initialized.

### Goal
Verify `task create` and `task start` always enqueue work and never execute inline.

### Steps
1. Create a task:
   ```bash
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --name "queue-mode" --goal "queue" | grep -oE '[0-9a-f-]{36}' | head -1)
   ```
2. Check task state:
   ```bash
   orchestrator task info "${TASK_ID}" -o json
   ```
3. Re-enqueue explicitly:
   ```bash
   orchestrator task start "${TASK_ID}"
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

## Scenario 3: Daemon Worker Consumption

### Preconditions
- At least one pending task exists.

### Steps
1. Start the daemon with embedded workers:
   ```bash
   ./target/release/orchestratord --foreground --workers 3
   ```
2. In another terminal, watch the task until it leaves `pending`:
   ```bash
   orchestrator task watch "${TASK_ID}" --interval 1
   ```
3. Inspect final status:
   ```bash
   orchestrator task info "${TASK_ID}" -o json
   ```

### Expected
- Embedded daemon workers consume pending tasks while running.
- With `--workers N`, pending tasks can be consumed concurrently by N workers.
- `task watch` and `task info` reflect the status change from `pending` to an executing/terminal state.

### Expected Data State
```sql
SELECT event_type
FROM events
WHERE task_id = '{task_id}'
  AND event_type = 'scheduler_enqueued'
ORDER BY id DESC
LIMIT 5;
-- Expected: enqueue events exist for queued submissions
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

## Scenario 5: Task Retry (Queue-Only)

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
3. Retry with `--force`:
   ```bash
   orchestrator task retry {task_item_id} --force
   ```

### Expected
- Without `--force`: prints warning to stderr and exits with code 1; no state change occurs.
- Retry with `--force` enqueues associated task and returns without inline execution.

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
| 1 | Queue-Only Task Start | ☐ | | | |
| 2 | Create/Start Enqueue Mode | ☐ | | | |
| 3 | Daemon Worker Consumption | ☐ | | | |
| 4 | Task Logs | ☐ | | | |
| 5 | Task Retry (Queue-Only) | ☐ | | | |
