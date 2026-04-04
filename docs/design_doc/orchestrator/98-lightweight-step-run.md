# Design Doc: FR-090 Lightweight Step Run

## Problem

The orchestrator required all execution to go through the full workflow -> task -> step loop chain. Users could not "point-shoot" a single step from an existing workflow or execute a step template directly. The kernel capability (`process_item_filtered()` with `step_filter`) already existed internally for scope segment partitioning, but was not exposed to CLI or gRPC layers.

## Solution

Three-phase implementation that progressively expands the execution surface:

### Phase 1: Workflow Step Filtering

Added `--step` and `--set` flags to `task create`:

- **Proto**: `TaskCreateRequest` extended with `repeated string step_filter` and `map<string,string> initial_vars`
- **DB**: Migration m0023 adds `step_filter_json` and `initial_vars_json` columns to tasks table
- **Validation**: Step IDs validated against execution plan at creation time; unknown IDs return clear error
- **Runtime**: `TaskRuntimeContext` gains `step_filter: Option<HashSet<String>>`; loaded from DB on task start
- **Segment filtering**: `build_scope_segments()` skips steps not in the filter, so existing segment execution, parallelism, and finalization logic works unchanged
- **Initial vars**: Merged into `pipeline_vars.vars` via `entry().or_insert()` during runtime context loading, available to prompt rendering and prehook CEL

### Phase 2: `orchestrator run` Command

New top-level `run` subcommand for synchronous execution:

- Creates task via `TaskCreate` RPC (with step_filter + initial_vars)
- Follows task logs via `TaskFollow` streaming RPC
- On completion, fetches final status and exits with code 0 (completed) or 1 (failed)
- `--detach` flag falls back to async task creation behavior

### Phase 3: Direct Assembly Mode

New `RunStep` gRPC endpoint for workflow-less execution:

- `--template` + `--agent-capability` flags construct a single-step `TaskExecutionPlan`
- Template and capability validated against project resources
- Optional `--profile` for execution profile override
- Reuses all existing task infrastructure (items, events, RunResult recording)

## Key Design Decisions

1. **Filter at segment level, not item level**: The step_filter is applied in `build_scope_segments()` rather than in `process_item_filtered()`. This ensures filtered-out steps don't appear in any segment, keeping all existing parallelism and finalization logic intact.

2. **initial_vars use `entry().or_insert()`**: This ensures initial vars don't overwrite values already in pipeline_vars (e.g., from a previous cycle restore), while still providing them as defaults.

3. **Ephemeral workflow ID for Phase 3**: Direct assembly tasks use `_ephemeral:<template_name>` as workflow_id, distinguishing them from regular workflow tasks without adding a new task type.

4. **No new execution engine**: All three phases reuse 100% of existing step execution logic (phase_runner, agent spawn, capture, record).

## Files Modified

| Layer | Files |
|-------|-------|
| Proto | `crates/proto/orchestrator.proto` |
| DTO | `core/src/dto.rs` |
| Migration | `core/src/persistence/migration_steps.rs`, `migration.rs` |
| Task creation | `core/src/task_ops.rs` |
| Runtime | `crates/orchestrator-config/src/config/execution.rs`, `crates/orchestrator-scheduler/src/scheduler/runtime.rs` |
| Segment | `crates/orchestrator-scheduler/src/scheduler/loop_engine/segment.rs` |
| Repository | `core/src/task_repository/types.rs`, `queries.rs` |
| gRPC | `crates/daemon/src/server/task.rs`, `mod.rs` |
| CLI | `crates/cli/src/cli.rs`, `commands/task.rs`, `commands/run.rs` (new), `commands/mod.rs` |
| GUI | `crates/gui/src/commands/task.rs`, `system.rs` |
| Integration | `crates/integration-tests/src/lib.rs` |
