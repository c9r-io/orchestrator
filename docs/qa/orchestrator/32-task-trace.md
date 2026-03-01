# Orchestrator - Task Trace Post-Mortem Diagnostics

**Module**: orchestrator
**Scope**: `task trace` command — execution timeline reconstruction, cycle boundary repair, and anomaly detection
**Scenarios**: 5
**Priority**: High

---

## Background

The `task trace` command reconstructs a task's execution history from the events table and command_runs, producing a human-readable timeline with automatic anomaly detection. This is the primary post-mortem debugging tool for diagnosing issues like duplicate execution, overlapping cycles, unexpanded template variables, and orphan commands.

The latest regression fix also requires:
- completed multi-cycle tasks must populate `ended_at` for every cycle
- normal two-cycle tasks must not emit false `overlapping_cycles`
- `summary.wall_time_secs` must be populated for completed tasks, including RFC3339 timestamps with timezone offsets

### Common Preconditions

Every scenario starts from a clean project with at least one completed task:

```bash
QA_PROJECT="qa-trace-$(date +%s)"
./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/echo-workflow.yaml
```

---

## Scenario 1: Basic Trace Output

### Preconditions

- Orchestrator binary built and available
- At least one task completed or failed

### Goal

Verify `task trace` renders a readable timeline with cycle/step structure and closed cycle boundaries for completed tasks

### Steps

1. Create and run a task to completion:
   ```bash
   ./scripts/orchestrator.sh task create --goal "trace test" --project "${QA_PROJECT}" --from fixtures/manifests/bundles/echo-workflow.yaml
   ```
2. Note the `{task_id}` from output
3. Run trace:
   ```bash
   ./scripts/orchestrator.sh task trace {task_id}
   ```

### Expected

- Header line shows task ID (truncated to 8 chars) and status
- Wall time, cycle count, step count, and command count are displayed
- At least one `Cycle N` section is rendered
- Each step line shows: timestamp, status icon (✓/✗/⊘), step ID, duration, agent
- For completed tasks, the final cycle is closed and wall time is not shown as `?`
- Exit code 0

---

## Scenario 2: JSON Output

### Preconditions

- Same task as Scenario 1

### Steps

1. Run trace with `--json`:
   ```bash
   ./scripts/orchestrator.sh task trace {task_id} --json
   ```
2. Pipe through `jq` to validate:
   ```bash
   ./scripts/orchestrator.sh task trace {task_id} --json | jq .
   ```

### Expected

- Output is valid JSON
- Top-level keys: `task_id`, `status`, `cycles`, `anomalies`, `summary`
- `cycles` is an array; each element has `cycle`, `started_at`, `ended_at`, `steps`
- `summary` contains `total_cycles`, `total_steps`, `total_commands`, `failed_commands`, `anomaly_counts`, `wall_time_secs`
- `anomalies` is an array (may be empty for clean runs)
- For a normal completed multi-cycle task, every cycle has non-null `ended_at`
- For a normal completed multi-cycle task, `.anomalies[] | select(.rule == "overlapping_cycles")` returns no rows
- For a completed task, `summary.wall_time_secs` is non-null

---

## Scenario 3: Verbose Mode Shows Item IDs

### Preconditions

- Same task as Scenario 1

### Steps

1. Run trace with `--verbose`:
   ```bash
   ./scripts/orchestrator.sh task trace {task_id} --verbose
   ```

### Expected

- Each item-scoped step additionally shows `item={item_id}` on an indented line
- Task-scoped steps (scope="task") do not show item IDs

---

## Scenario 4: Anomaly Detection - Real Failure vs False Overlap

### Preconditions

- Orchestrator binary built and available
- A workflow fixture that produces a failing step
- A normal completed task with two cycles (or the unit-level regression suite as fallback)

### Steps

1. Run the unit-level trace regression suite:
   ```bash
   cd core && cargo test --lib scheduler::trace -- --nocapture
   ```
2. Create a task with a workflow that has a failing agent:
   ```bash
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/echo-workflow-fixed.yaml
   ```
   (Or use any fixture where a step exits non-zero)
3. Run the task and wait for completion
4. Run trace:
   ```bash
   ./scripts/orchestrator.sh task trace {task_id}
   ```
5. Run trace as JSON for structured validation:
   ```bash
   ./scripts/orchestrator.sh task trace {task_id} --json | jq '.anomalies[] | select(.rule == "nonzero_exit")'
   ```

### Expected

- `cargo test --lib scheduler::trace` passes
- `two_cycle_completed_task_closes_first_cycle_without_overlap` passes
- `completed_task_wall_time_uses_task_meta_when_events_are_sparse` passes
- `completed_task_backfills_last_cycle_end_from_completed_at` passes
- Anomaly section shows `WARN nonzero_exit` with the phase and exit code
- JSON output: anomaly object has `rule: "nonzero_exit"`, `severity: "warning"`, `message` containing exit code
- `summary.anomaly_counts` includes `"warning": N` (N >= 1)
- Normal completed multi-cycle traces do not report `overlapping_cycles`
- Completed traces expose non-null `summary.wall_time_secs`

---

## Scenario 5: Trace on Nonexistent Task

### Preconditions

- Orchestrator binary built and available

### Steps

1. Run trace with an invalid task ID:
   ```bash
   ./scripts/orchestrator.sh task trace nonexistent-task-id-000
   ```

### Expected

- Error message indicating task not found
- Exit code is non-zero

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Basic Trace Output | ☐ | | | |
| 2 | JSON Output | ☐ | | | |
| 3 | Verbose Mode Shows Item IDs | ☐ | | | |
| 4 | Anomaly Detection - Real Failure vs False Overlap | ☐ | | | |
| 5 | Trace on Nonexistent Task | ☐ | | | |
