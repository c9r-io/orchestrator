# Orchestrator - Degenerate Cycle Detection and Circuit Breaker

**Module**: orchestrator
**Status**: Approved
**Related FR**: FR-035
**Related QA**: `docs/qa/orchestrator/23-degenerate-cycle-loop-guard.md`
**Created**: 2026-03-13
**Last Updated**: 2026-03-13

## Background And Goals

### Background

During `follow-logs-callback-execution.md` test plan execution, a task entered a degenerate loop where `implement` steps failed 13+ times in rapid succession (~12s intervals), wasting API tokens with no chance of recovery. The existing `loop_guard` only fires at pipeline end, so it never triggers when a mid-pipeline step fails repeatedly.

### Root Cause

1. No per-item per-step consecutive failure tracking — the scheduler blindly retried the same failing step
2. No cycle interval detection — cycles completing in <15s indicated immediate step failure but went undetected
3. The `loop_guard` step sits at the end of the pipeline and cannot intercept mid-pipeline degenerate patterns

### Goals

- Stop wasting API tokens on structurally unrecoverable failures
- Provide layered defense: per-item circuit breaker (L1) + task-level rapid cycle detection (L2)
- Preserve normal self-bootstrap flows (cycle intervals >5min) without interference
- Allow manual recovery via `task resume --reset-blocked`

### Non-goals

- Automatic root-cause diagnosis of why a step fails
- Replacing the existing `loop_guard` mechanism

## Design

### Defense-in-Depth Layers

**L1: Per-item per-step circuit breaker** (`segment.rs`)

Tracks consecutive failures per `(item_id, step_id)` on `TaskRuntimeContext`. When failures reach `max_item_step_failures` (default 3):
- Item status set to `blocked` in DB
- `item_blocked_consecutive_failures` event emitted
- Item skipped in subsequent cycles

Exponential backoff before blocking:
- 1st failure: 30s retry delay
- 2nd failure: 120s retry delay
- 3rd failure: blocked

**L2: Rapid cycle detection** (`loop_engine/mod.rs`)

After `cycle_started` event (cycle >= 4), queries last 4 `cycle_started` timestamps from the events table. If all 3 intervals are below `min_cycle_interval_secs` (default 60):
- `degenerate_cycle_detected` event emitted
- Task auto-paused

This is DB-query based, so it survives daemon restarts.

### Key Design Decisions

1. **`TaskRuntimeContext` for failure tracking** — lives across cycles (unlike `StepExecutionAccumulator` which is per-cycle). Counters are in-memory; item `blocked` status is persisted in DB.

2. **No `tokio::sleep` for backoff** — would block worker slots. Instead, `item_retry_after: HashMap<String, Instant>` marks when an item can next be dispatched. Items are skipped (not slept) if their retry time hasn't arrived.

3. **Cycle timestamps from DB** — `query_recent_cycle_timestamps` reads from the events table rather than keeping an in-memory Vec, ensuring rapid cycle detection works after daemon restart.

4. **`blocked` status is implicitly resolved** — `count_unresolved_items` uses `status IN ('unresolved', 'qa_failed')`, so blocked items don't trigger new cycles. No finalize logic changes needed.

5. **Separate config field** — `max_item_step_failures` is distinct from existing `max_consecutive_failures` (which controls task-level auto-rollback).

### Configuration

```yaml
safety:
  max_item_step_failures: 3       # per-item per-step circuit breaker threshold
  min_cycle_interval_secs: 60     # rapid cycle detection threshold
```

### Anomaly Integration

`DegenerateLoop` anomaly rule added to the trace system. `detect_degenerate_loop()` in `trace/anomaly.rs` groups `command_runs` by `(task_item_id, phase)` and detects 3+ consecutive non-zero exit codes from the tail.

### Recovery

`task resume --reset-blocked` resets all `blocked` items back to `unresolved`, allowing the task loop to retry them. The `reset_blocked` field is added to `TaskResumeRequest` in the proto.

## Files Changed

| File | Change |
|------|--------|
| `core/src/config/safety.rs` | `max_item_step_failures`, `min_cycle_interval_secs` fields |
| `core/src/config/execution.rs` | `item_step_failures`, `item_retry_after` on TaskRuntimeContext |
| `core/src/scheduler/runtime.rs` | Initialize new fields |
| `core/src/scheduler/task_state.rs` | `set_item_blocked`, `reset_blocked_items`, `query_recent_cycle_timestamps` |
| `core/src/scheduler/loop_engine/segment.rs` | L1 circuit breaker logic |
| `core/src/scheduler/loop_engine/mod.rs` | L2 rapid cycle detection |
| `core/src/anomaly.rs` | `DegenerateLoop` variant |
| `core/src/scheduler/trace/anomaly.rs` | `detect_degenerate_loop()` |
| `core/src/scheduler/trace/builder.rs` | Wire detector |
| `proto/orchestrator.proto` | `reset_blocked` in `TaskResumeRequest` |
| `crates/cli/src/cli.rs` | `--reset-blocked` flag |
| `crates/cli/src/commands/task.rs` | Pass flag in gRPC call |
| `crates/daemon/src/server/task.rs` | Handle `reset_blocked` on resume |
| `crates/cli/src/output/task_detail.rs` | `[BLOCKED]` tag in task info |
| `core/src/cli_types.rs` | SafetySpec matching fields |
| `core/src/resource/workflow/workflow_convert.rs` | Config↔Spec conversion |
