# Trace/Scheduler Observation Ticket

Status: FAILED
Task ID: baf041bd-20cc-443d-b0e3-a358fd1dc136
Created At: 2026-03-01 13:00:00
Related QA Doc: docs/qa/orchestrator/32-task-trace.md

## Test Content

Monitor the `self-bootstrap` workflow while executing the `docs/plan/resource-rs-refactor-execution.md` two-cycle refactor task, then inspect `task trace` output and scheduler state for correctness.

## Expected Result

- The task completes cleanly after 2 cycles.
- `task trace` reports accurate cycle boundaries.
- Each cycle has a correct `ended_at`.
- `summary.wall_time_secs` is populated for a completed task.
- Trace step scope reflects the actual execution model (`plan`/`implement`/`self_test`/`align_tests`/`doc_governance` should not be mislabeled).
- Trace anomaly detection should not report cycle overlap when the previous cycle has logically ended.

## Reproduction Steps

1. Apply and run the `self-bootstrap` workflow for a two-cycle refactor task.
2. Wait for task `baf041bd-20cc-443d-b0e3-a358fd1dc136` to reach `completed`.
3. Run `./scripts/orchestrator.sh task trace baf041bd-20cc-443d-b0e3-a358fd1dc136`.
4. Run `./scripts/orchestrator.sh task trace --json baf041bd-20cc-443d-b0e3-a358fd1dc136`.
5. Compare trace output with raw event/log sequence under `data/logs/baf041bd-20cc-443d-b0e3-a358fd1dc136/` and the task completion event.

## Actual Result

- The task itself completed successfully with `0` failed commands and `2` completed cycles.
- `task trace` reported an `ERROR` anomaly: `overlapping_cycles`.
- The corresponding JSON trace shows:
  - `cycles[0].ended_at = null`
  - `cycles[1].ended_at` is populated
  - `summary.wall_time_secs = null`
- Cycle 1 contains skipped `align_tests` and `doc_governance` steps at `2026-03-01T04:07:03.632xxx+00:00`, and Cycle 2 starts at `2026-03-01T04:07:03.635397+00:00`, which strongly suggests the overlap anomaly is caused by missing cycle-finalization metadata rather than real concurrent execution.
- Trace JSON labels all captured steps with `scope: "item"`, including phases that are treated as task-scoped by the documented execution model.

### Key Trace Snippets

- Reported anomaly:
  - `Cycle 2 started at 2026-03-01T04:07:03.635397+00:00 while Cycle 1 (started 2026-03-01T03:52:54.918652+00:00) still running`
- Final events:
  - `step_finished` for `doc_governance`
  - `loop_guard_decision` with `continue=false` and `reason=fixed_cycles_complete`
  - `task_completed`

## Suspected Cause

- `core/src/scheduler/trace.rs` likely does not close Cycle 1 when the cycle ends via skipped tail steps followed immediately by a new cycle.
- Trace summarization appears to miss wall-clock derivation for completed tasks.
- Trace step scope mapping may be using task-item context as the displayed scope instead of the configured phase scope.

## Impact

- Operators can receive a false `ERROR` anomaly for healthy tasks.
- Monitoring output overstates scheduler risk and obscures real anomalies.
- Completed-task trace summaries are missing a basic timing signal.
- Scope mislabeling makes it harder to validate whether workflow execution matches architecture.

## Suggested Fix

1. Fix cycle finalization in trace reconstruction so every completed cycle receives `ended_at`.
2. Recompute `overlapping_cycles` only after final cycle boundary reconciliation.
3. Populate `summary.wall_time_secs` whenever the task has start and completion timestamps.
4. Map displayed step scope from workflow semantics rather than item attachment alone.
