---
self_referential_safe: true
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

- Repository root is the current working directory.
- Rust toolchain is available.

### Goal

Verify trace timeline reconstruction produces correct cycle/step structure and closed cycle boundaries, via unit tests.

### Steps

1. Run the core trace reconstruction unit tests:
   ```bash
   cargo test -p orchestrator-scheduler --lib -- trace::tests::single_cycle_with_steps --nocapture
   cargo test -p orchestrator-scheduler --lib -- trace::tests::multi_cycle_trace --nocapture
   ```

2. Verify trace output structure via code review:
   ```bash
   rg -n "TRACE TIMELINE\|ANOMALIES.*detected\|cycle_started\|step_started" crates/orchestrator-scheduler/src/scheduler/trace/
   ```

### Expected

- `single_cycle_with_steps` passes — verifies cycle/step structure for single-cycle tasks
- `multi_cycle_trace` passes — verifies multi-cycle timeline with closed boundaries
- Code review confirms output format: `TRACE TIMELINE (N events)` header, `timestamp event_type step={id}` lines

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

- Repository root is the current working directory.
- Rust toolchain is available.

### Goal

Verify anomaly detection correctly identifies nonzero_exit, low_output, and overlapping_cycles via unit tests.

### Steps

1. Run the full trace anomaly regression suite:
   ```bash
   cargo test -p orchestrator-scheduler --lib -- trace --nocapture
   ```

2. Run focused anomaly detection tests:
   ```bash
   cargo test -p orchestrator-scheduler --lib -- anomaly --nocapture
   ```

### Expected

- `detect_nonzero_exit_anomaly` passes
- `two_cycle_completed_task_closes_first_cycle_without_overlap` passes
- `completed_task_wall_time_uses_task_meta_when_events_are_sparse` passes
- `completed_task_backfills_last_cycle_end_from_completed_at` passes
- `detect_low_output_step_anomaly` passes
- `quiet_heartbeat_does_not_create_low_output_anomaly` passes
- `multiple_low_output_heartbeats_for_same_step_deduplicate` passes
- All 63 trace + anomaly unit tests pass (58 in trace/tests.rs + 5 in trace/anomaly.rs)
- Normal completed multi-cycle traces do not report `overlapping_cycles`
- Each anomaly includes `rule`, `severity`, `escalation`, `message`, and `at` fields

---

## Scenario 5: Trace Availability and Config Independence

### Preconditions

- Repository root is the current working directory.
- Rust toolchain is available.

### Goal

Verify trace reconstruction is independent of the active config validity — the trace module reconstructs from persisted events without requiring a runnable config.

### Steps

1. Verify trace module builds and reconstructs from raw events (no config dependency):
   ```bash
   cargo test -p orchestrator-scheduler --lib -- trace::tests --nocapture
   ```

2. Code review: confirm trace reconstruction reads from events table only, not from active config:
   ```bash
   rg -n "fn build_trace\|fn reconstruct\|events.*task_id" crates/orchestrator-scheduler/src/scheduler/trace/
   ```

3. Verify low-output anomaly detection distinguishes probe types via unit test:
   ```bash
   cargo test -p orchestrator-scheduler --lib -- detect_low_output_step_anomaly --nocapture
   cargo test -p orchestrator-scheduler --lib -- quiet_heartbeat_does_not_create_low_output_anomaly --nocapture
   ```

### Expected

- All trace unit tests pass — reconstruction works from events alone
- Code review confirms trace module reads events/command_runs tables, not active config
- `detect_low_output_step_anomaly` passes — low-output correctly identified
- `quiet_heartbeat_does_not_create_low_output_anomaly` passes — no false positives

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Basic Trace Output | PASS | 2026-03-31 | Claude | single_cycle_with_steps and multi_cycle_trace unit tests pass; code review confirms TRACE TIMELINE output format with cycle_started/step_started events |
| 2 | JSON Output | PASS | 2026-03-31 | Claude | Verified with probe_task_scoped (0ddddf99): 4 steps, scope=task with anchor_item_id, wall_time_secs=0.157, no anomalies; probe_item_scoped (235d91d3): 159 steps, scope=item with item_id, wall_time_secs=5.55, no anomalies; top-level keys verified: task_id, status, cycles, graph_runs, anomalies, summary; no overlapping_cycles; all cycles have non-null ended_at |
| 3 | Verbose Mode Shows Scope And Binding | PASS | 2026-03-31 | Claude | task-scoped: verbose=21 events vs non-verbose=9 events; item-scoped: verbose=1117 events vs non-verbose=319 events; text format uses same timestamp/event_type/step=/item= format; JSON scope bindings verified: task-scoped shows scope=task with anchor_item_id, item-scoped shows scope=item with item_id |
| 4 | Anomaly Detection - Real Failure vs False Overlap | PASS | 2026-03-31 | Claude | 71 trace + anomaly unit tests all pass; detect_nonzero_exit_anomaly, detect_low_output_step_anomaly, quiet_heartbeat_does_not_create_low_output_anomaly, two_cycle_completed_task_closes_first_cycle_without_overlap, detect_overlapping_cycles_anomaly, multiple_low_output_heartbeats_for_same_step_deduplicate, completed_task_wall_time_uses_task_meta_when_events_are_sparse, completed_task_backfills_last_cycle_end_from_completed_at all verified |
| 5 | Trace Availability and Config Independence | PASS | 2026-03-31 | Claude | 61 trace unit tests pass; code review confirms build_trace reads events/command_runs DTOs directly with no active config dependency; detect_low_output_step_anomaly and quiet_heartbeat_does_not_create_low_output_anomaly pass |
