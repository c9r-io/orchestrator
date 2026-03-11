# Dynamic DAG Mainline Execution

**Module**: orchestrator  
**Related QA**: `docs/qa/orchestrator/59-dynamic-dag-mainline-execution.md`, `docs/qa/orchestrator/32-task-trace.md`

## Background

FR-004 promoted DAG orchestration from a side capability into an explicit runtime mode. The first implementation wave already introduced:

- `workflow.execution.mode: dynamic_dag`
- graph materialization from static workflow steps
- adaptive planner output conversion into runtime graph form
- graph-aware trace events

The remaining gap was not scheduling correctness but debugability. Runtime graph state still lived mostly inside `dynamic_*` events, which made `task info`, replay, and post-mortem analysis harder than necessary.

## Goals

- Make runtime graph state queryable without reconstructing everything from events
- Expose effective execution graph directly through `task info`
- Persist planner/debug snapshots in a decoupled task-level model
- Keep `task trace` event-centric while enriching graph correlation

## Non-goals

- Parallel graph execution
- Cross-task DAG scheduling
- Graph UI visualization
- CEL explain-tree style condition debugging

## Scope

### In

- `task_graph_runs` and `task_graph_snapshots` persistence
- `TaskInfoResponse.graph_debug`
- graph run identifiers on runtime events
- persisted effective graph, normalized plan, planner raw output, execution replay
- `debug --component dag`

### Out

- historical backfill migration for old event-only tasks
- changing the deterministic sequential v1 graph executor

## Interfaces And Data Changes

- New DB tables:
  - `task_graph_runs`
  - `task_graph_snapshots`
- `task info` now returns graph debug bundles with:
  - `graph_run_id`
  - graph source/status
  - effective graph JSON
  - optional planner/raw/replay snapshots
- `dynamic_*` events now carry:
  - `cycle`
  - `graph_run_id`
  - `source`
  - `mode`

## Key Design Decisions

- Separate graph persistence from `events` rather than overloading `payload_json`
  - Events remain the timeline source
  - Graph snapshots become the query/debug source

- Always persist `effective_graph`
  - `persist_graph_snapshots` only gates extended debug materials
  - This keeps `task info` reliable even in low-debug mode

- Use stable reason codes for edge evaluation
  - `unconditional`
  - `cel_true`
  - `cel_false`
  - This avoids promising opaque or brittle CEL internals

- Keep old tasks readable through query-layer fallback
  - no risky data backfill
  - legacy `dynamic_plan_materialized` events still produce a best-effort graph bundle

## Risks And Mitigations

- More runtime writes per DAG cycle
  - snapshots are compact JSON blobs and are bounded by cycle/run granularity

- Query/API drift between old and new tasks
  - fallback path preserves visibility for event-only historical runs

- Debug payload sprawl
  - snapshot kinds are fixed and normalized instead of free-form

## Observability And Operations

- `task trace` keeps graph timeline visibility through `dynamic_*` events
- `task info` becomes the primary point-in-time graph inspection command
- `debug --component dag` shows effective mode/fallback/snapshot behavior per workflow
- Default recommendation: enable `persist_graph_snapshots` in environments where DAG tuning or planner troubleshooting matters

## Testing And Acceptance

- Repository/service tests for graph bundle persistence and query
- Trace regression ensuring `graph_runs` still render correctly
- CLI output regression for `task info` JSON payloads
- Compile-only workspace verification after proto and DB changes

Acceptance is satisfied when:

- `task info` shows runtime graph debug bundles
- graph snapshots are stored outside the events table
- trace remains intact for dynamic DAG runs
- DAG debug config is inspectable from CLI
