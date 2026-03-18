---
self_referential_safe: false
self_referential_safe_scenarios: [S2, S3]
---

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
- self-referential probe traces must support official probe workflows without requiring `self_test`
- dynamic DAG traces should preserve `graph_run_id`-correlated event grouping even when point-in-time graph snapshots are persisted separately
- graph edge evaluation payloads should preserve stable reason codes such as `unconditional`, `cel_true`, and `cel_false`

### Automated Regression

The unified CLI probe regression runner covers trace scenarios automatically:

```bash
./scripts/regression/run-cli-probes.sh --group trace
```

### Common Preconditions

Every scenario starts from a clean project with at least one completed task:

```bash
QA_PROJECT="qa-trace-$(date +%s)"
orchestrator apply -f fixtures/manifests/bundles/cli-probe-fixtures.yaml
orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
orchestrator apply -f fixtures/manifests/bundles/cli-probe-fixtures.yaml --project "${QA_PROJECT}"
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
   orchestrator task create --goal "trace test" --project "${QA_PROJECT}" --workspace cli_probe_ws --workflow probe_task_scoped
   ```
2. Note the `{task_id}` from output
3. Run trace:
   ```bash
   orchestrator task trace {task_id}
   ```

### Expected

- Output begins with `TRACE TIMELINE (N events)` header followed by a separator line
- Each event line shows: timestamp, event_type, step={id} (if applicable), item={truncated_id} (if applicable)
- Events include `cycle_started`, `step_started`, `step_finished`, etc.
- If anomalies are detected, an `ANOMALIES (N detected)` section appears with `[SEVERITY] rule: message` lines
- Exit code 0

---

## Scenario 2: JSON Output

### Preconditions

- One completed task from `probe_task_scoped`
- One completed task from `probe_item_scoped`

### Steps

1. Run trace with `--json`:
   ```bash
   orchestrator task trace {task_id} --json
   ```
2. Pipe through `jq` to validate:
   ```bash
   orchestrator task trace {task_id} --json | jq .
   ```

### Expected

- Output is valid JSON
- Top-level keys: `task_id`, `status`, `cycles`, `graph_runs`, `anomalies`, `summary`
- `cycles` is an array; each element has `cycle`, `started_at`, `ended_at`, `steps`
- `summary` contains `total_cycles`, `total_steps`, `total_commands`, `failed_commands`, `anomaly_counts`, `wall_time_secs`
- `graph_runs` is present; it may be empty for non-DAG tasks
- For dynamic DAG tasks, graph events remain grouped under the correct run/cycle after graph snapshots move into task-level persistence
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
   orchestrator task trace {task_scoped_task_id} --verbose
   ```
2. Run trace with `--verbose` for the item-scoped fixture:
   ```bash
   orchestrator task trace {item_scoped_task_id} --verbose
   ```

### Expected

- Verbose mode includes additional events not shown in non-verbose output (e.g. intermediate heartbeats)
- Text output uses the same `timestamp event_type step={id} item={id}` format as non-verbose mode
- Scope and binding information is available in JSON output (`--json`), where each step entry contains `scope`, `anchor_item_id`, etc.
- When a task emits `dynamic_*` graph events, JSON trace output preserves them under `graph_runs`
- When a task emits `dynamic_edge_evaluated`, the corresponding payload retains a stable reason code rather than a free-form explanation blob
- `probe_item_scoped` steps in JSON output show `scope: "item"` with `item_id`
- `probe_task_scoped` steps in JSON output show `scope: "task"` with `anchor_item_id` when available
- Legacy tasks without explicit scope metadata show `scope: "legacy"` (not `scope: "unknown"`) in JSON output

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
   orchestrator task trace {task_id}
   orchestrator task trace {task_id} --json | jq '.anomalies[] | select(.rule == "nonzero_exit")'
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
- A trace built from low-output heartbeats reports `low_output` exactly once per affected step
- Each anomaly in JSON output includes `rule`, `severity`, `escalation`, `message`, and `at` fields

---

## Scenario 5: Trace Works When Active Config Is Not Runnable

### Preconditions

- Orchestrator binary built and available
- At least one historical task exists in the local database
- Current active config is intentionally invalid (for example, a workflow step defines both `builtin` and `required_capability`)

### Steps

1. Confirm the current config is invalid with a command that requires a runnable config:
   ```bash
   orchestrator check
   ```
2. Run trace for a historical task:
   ```bash
   orchestrator task trace {task_id} --json | jq '.summary'
   ```

### Expected

- `check` reports the active config is not runnable
- `task trace` still returns trace output instead of failing during startup
- JSON output remains valid and includes `summary.total_cycles`
- Exit code 0

### Self-Referential Probe Trace Checks

These checks use the official self-referential probe fixtures directly, not
`apply --project`.

Do not use `delete project/<name> --force` here; these probe checks rely on direct
runtime fixtures, not control-plane reinitialization.

1. Apply the self-referential probe fixtures:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/self-referential-probe-fixtures.yaml
   ```
2. Run a low-output self-referential probe task to completion.
3. Run an active-output self-referential probe task to completion.
4. Inspect both traces:
   ```bash
   orchestrator task trace {low_output_task_id} --json | jq '.anomalies'
   orchestrator task trace {active_output_task_id} --json | jq '.anomalies'
   ```

Expected:
- `self_ref_probe_low_output` emits `low_output` anomaly with `escalation: "intervene"`
- `self_ref_probe_active_output` does not emit `low_output`
- Both probe workflows include an enabled `self_test` builtin step (required by the self-referential safety policy for all workflows targeting self-referential workspaces)

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Basic Trace Output | ☐ | | | |
| 2 | JSON Output | PASS | 2026-03-19 | Claude | task-scoped: 4 steps, scope=task, anchor_item_id=158eb22e; item-scoped: 2 steps, scope=item, item_id set; wall_time_secs=0.085/0.042; no anomalies; exit 0; no overlapping_cycles |
| 3 | Verbose Mode Shows Scope And Binding | PASS | 2026-03-19 | Claude | verbose=25 events vs non-verbose=9 events; step_id= and item= fields present; scope binding correct in JSON |
| 4 | Anomaly Detection - Real Failure vs False Overlap | ☐ | | | |
| 5 | Trace Works When Active Config Is Not Runnable | ☐ | | | |
