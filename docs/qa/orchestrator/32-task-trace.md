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
- `task trace` must remain available even when the current active config is invalid for execution
- low-output heartbeats must be distinguishable from ordinary long-running steps in anomaly output

### Common Preconditions

Every scenario starts from a clean project with at least one completed task:

```bash
QA_PROJECT="qa-trace-$(date +%s)"
./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/cli-probe-fixtures.yaml
./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --from-workspace cli_probe_ws --workflow probe_task_scoped --force
./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
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
   ./scripts/orchestrator.sh task create --goal "trace test" --project "${QA_PROJECT}" --workspace cli_probe_ws --workflow probe_task_scoped
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

- One completed task from `probe_task_scoped`
- One completed task from `probe_item_scoped`

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

## Scenario 3: Verbose Mode Shows Scope And Binding

### Preconditions

- Same task as Scenario 1

### Steps

1. Run trace with `--verbose` for the task-scoped fixture:
   ```bash
   ./scripts/orchestrator.sh task trace {task_scoped_task_id} --verbose
   ```
2. Run trace with `--verbose` for the item-scoped fixture:
   ```bash
   ./scripts/orchestrator.sh task trace {item_scoped_task_id} --verbose
   ```

### Expected

- Every verbose step prints an indented scope line
- `probe_item_scoped` steps show `scope=item item={item_id}`
- `probe_task_scoped` steps show `scope=task`, and if an execution anchor exists it is rendered as `anchor_item={item_id}`
- Legacy tasks without explicit scope metadata may show `scope=unknown`; they must not silently relabel the anchor as a true `item=...`

---

## Scenario 4: Anomaly Detection - Real Failure vs False Overlap

### Preconditions

- Orchestrator binary built and available
- A workflow fixture that produces a failing step
- A normal completed task with two cycles (or the unit-level regression suite as fallback)
- Prefer completed tasks from `probe_low_output` and `probe_active_output` when validating low-output anomaly presence vs absence

### Steps

1. Run the unit-level trace regression suite:
   ```bash
   cd core && cargo test --lib scheduler::trace -- --nocapture
   ```
2. Run the focused nonzero-exit regression:
   ```bash
   cd core && cargo test --lib scheduler::trace::tests::detect_nonzero_exit_anomaly -- --nocapture
   ```
3. If the current config is runnable and you have a known failing task with a non-zero step exit, run:
   ```bash
   ./scripts/orchestrator.sh task trace {task_id}
   ./scripts/orchestrator.sh task trace {task_id} --json | jq '.anomalies[] | select(.rule == "nonzero_exit")'
   ```

### Expected

- `cargo test --lib scheduler::trace` passes
- `detect_nonzero_exit_anomaly` passes
- `two_cycle_completed_task_closes_first_cycle_without_overlap` passes
- `completed_task_wall_time_uses_task_meta_when_events_are_sparse` passes
- `completed_task_backfills_last_cycle_end_from_completed_at` passes
- `detect_low_output_step_anomaly` passes
- `quiet_heartbeat_does_not_create_low_output_anomaly` passes
- `multiple_low_output_heartbeats_for_same_step_deduplicate` passes
- When a real failing task with non-zero exit is available, anomaly section shows `WARN nonzero_exit` with the phase and exit code
- When a real failing task with non-zero exit is available, JSON output includes `rule: "nonzero_exit"`, `severity: "warning"`, and a message containing the exit code
- Normal completed multi-cycle traces do not report `overlapping_cycles`
- Completed traces expose non-null `summary.wall_time_secs`
- A trace built from low-output heartbeats reports `low_output_step` exactly once per affected step

---

## Scenario 5: Trace Works When Active Config Is Not Runnable

### Preconditions

- Orchestrator binary built and available
- At least one historical task exists in the local database
- Current active config is intentionally invalid (for example, a workflow step defines both `builtin` and `required_capability`)

### Steps

1. Confirm the current config is invalid with a command that requires a runnable config:
   ```bash
   ./scripts/orchestrator.sh check
   ```
2. Run trace for a historical task:
   ```bash
   ./scripts/orchestrator.sh task trace {task_id} --json | jq '.summary'
   ```

### Expected

- `check` reports the active config is not runnable
- `task trace` still returns trace output instead of failing during startup
- JSON output remains valid and includes `summary.total_cycles`
- Exit code 0

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Basic Trace Output | ☐ | | | |
| 2 | JSON Output | ☐ | | | |
| 3 | Verbose Mode Shows Scope And Binding | ☐ | | | |
| 4 | Anomaly Detection - Real Failure vs False Overlap | ☐ | | | |
| 5 | Trace Works When Active Config Is Not Runnable | ☐ | | | |
