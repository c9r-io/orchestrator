# Design Doc 67: Parallel Spawn Stagger Delay (FR-055)

## Problem

When `max_parallel > 1`, the scheduler dispatches all available items into a `JoinSet` in a tight loop with no inter-spawn delay. Each agent subprocess loads MCP servers, initializes API connections, and performs filesystem operations during startup. Simultaneous launches cause MCP port/lock contention, API rate limiting, and I/O saturation. In a full-QA regression test with 4 parallel agents, all hung during initialization (0 bytes stdout for 900s), triggering `step_stall_killed` (exit=-7) for every item.

## Solution

Add an optional `stagger_delay_ms` field at both the workflow level (global default) and step level (per-step override). When set, the scheduler inserts a `tokio::time::sleep` between successive `join_set.spawn()` calls in the parallel dispatch loop.

### Resolution order

```
step.stagger_delay_ms  ??  workflow.stagger_delay_ms  ??  0
```

- Only applies when `max_parallel > 1` (parallel path)
- Value of `0` means no delay (backward-compatible default)
- The delay is inserted after each spawn, except after the last item

### YAML surface

```yaml
spec:
  max_parallel: 4
  stagger_delay_ms: 3000        # workflow-level default

  steps:
    - id: qa_testing
      scope: item
      stagger_delay_ms: 5000    # per-step override
```

## Modified Files

| File | Change |
|------|--------|
| `crates/orchestrator-config/src/cli_types.rs` | `stagger_delay_ms: Option<u64>` on `WorkflowYamlSpec` and `WorkflowStepSpec` |
| `crates/orchestrator-config/src/config/workflow.rs` | `stagger_delay_ms` on `WorkflowStepConfig` and `WorkflowConfig` |
| `crates/orchestrator-config/src/config/execution.rs` | `stagger_delay_ms` on `TaskExecutionStep` and `TaskExecutionPlan` |
| `core/src/resource/workflow/workflow_convert.rs` | Bidirectional mapping in spec-to-config and config-to-spec |
| `crates/orchestrator-scheduler/src/scheduler/loop_engine/segment.rs` | `stagger_delay_ms` on `ScopeSegment`, resolution in `build_scope_segments()`, sleep in dispatch loop |

## Verification

- `cargo build` passes
- `cargo test -p orchestrator-config -p orchestrator-scheduler -p agent-orchestrator` passes
- Manual: set `stagger_delay_ms: 3000` in a workflow YAML, run with `max_parallel: 4`, verify agent spawns are ~3s apart via `step_spawned` event timestamps
