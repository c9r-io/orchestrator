---
self_referential_safe: true
---

# Orchestrator - Dynamic DAG Mainline Execution

**Module**: orchestrator
**Scope**: Explicit `dynamic_dag` workflow mode, CEL-based dynamic trigger validation, graph materialization, runtime fallback, and graph-aware trace output
**Scenarios**: 5
**Priority**: High

---

## Background

FR-004 promotes DAG orchestration from a side capability into an explicit execution mode. The implementation introduces:

- `workflow.execution.mode: dynamic_dag`
- deterministic graph materialization from static workflow steps
- adaptive planner graph conversion into the same runtime graph model
- CEL-based validation for `dynamic_steps.trigger` and conditional edges
- graph-aware task trace output via `graph_runs` and `dynamic_*` events

This QA document focuses on deterministic, reproducible verification using unit tests and CLI-safe commands only. In addition to scheduler correctness, it verifies the closure work for task-level graph persistence and DAG debug surfaces.

---

## Scenario 1: Static Workflow Steps Materialize Into A Mainline Graph

### Goal
Verify static workflow steps can be converted into a runtime execution graph while excluding `init_once` and guard steps from the business DAG.

### Steps
1. Run the focused graph materialization regression:
   ```bash
   cargo test -p agent-orchestrator --lib build_static_execution_graph_skips_init_and_guard
   ```
2. Inspect the runtime graph model definitions:
   ```bash
   rg -n "WorkflowExecutionMode|EffectiveExecutionGraph|build_static_execution_graph" core/src/dynamic_orchestration/graph.rs
   ```

### Expected
- The test passes
- Static step nodes are materialized in order
- `init_once` is excluded from the effective graph
- `loop_guard` stays outside the graph execution path
- The graph entry node is the first executable business step

---

## Scenario 2: Invalid Dynamic Step Triggers Fail CEL Validation

### Goal
Verify `dynamic_steps.trigger` uses CEL validation instead of legacy string matching.

### Steps
1. Run the focused validation regression:
   ```bash
   cargo test -p agent-orchestrator --lib validate_workflow_config_rejects_invalid_dynamic_step_trigger_cel
   ```
2. Inspect trigger evaluation wiring:
   ```bash
   rg -n "evaluate_trigger_condition|dynamic_steps" core/src/config_load/validate/tests.rs
   ```

### Expected
- The test passes
- Invalid CEL in `dynamic_steps.trigger` is rejected during workflow validation
- Trigger evaluation is routed through the shared CEL prehook engine
- No legacy simple string matcher remains on the main trigger path

---

## Scenario 3: Task Trace Captures Graph Runs And Dynamic Events

### Goal
Verify the trace builder emits graph-level execution information for dynamic DAG runs.

### Steps
1. Run the focused trace regression:
   ```bash
   cargo test -p orchestrator-scheduler --lib build_trace_includes_dynamic_graph_events
   ```
2. Inspect trace model fields:
   ```bash
   rg -n "graph_runs|GraphTrace|dynamic_plan_materialized|dynamic_edge_taken" crates/orchestrator-scheduler/src/scheduler/trace/tests.rs
   ```

### Expected
- The test passes
- `TaskTrace` includes `graph_runs`
- Graph trace captures `source`, `node_count`, `edge_count`, and `dynamic_*` event history
- Dynamic edge transitions are preserved in trace output

---

## Scenario 4: Task Info Exposes Persisted Graph Debug Bundles

### Goal
Verify task detail queries can return persisted graph debug bundles without reconstructing everything from trace events.

### Steps
1. Run the focused repository regression:
   ```bash
   cargo test -p agent-orchestrator --lib load_task_detail_rows_includes_graph_debug_bundles
   ```
2. Run the service-level detail regression:
   ```bash
   cargo test -p orchestrator-scheduler --lib get_task_details_impl_returns_items_and_empty_runs
   ```
3. Inspect the query and proto wiring:
   ```bash
   rg -n "task_graph_runs|task_graph_snapshots|graph_debug|TaskGraphDebugBundle" core/src/task_repository/tests/queries_tests.rs
   ```

### Expected
- The repository regression passes
- `task info` has a dedicated `graph_debug` payload
- Graph bundles come from task-level snapshot tables when available
- Legacy event-only tasks still retain best-effort graph visibility via query fallback

---

## Scenario 5: DAG Debug View And Workspace Compile Validation

### Goal
Verify the implementation integrates cleanly across config, runtime, scheduler, trace, task query, and CLI debug surfaces.

### Steps
1. Run a workspace-wide compile-only validation:
   ```bash
   cargo test --workspace --lib --no-run
   ```
2. Run the DAG debug regression:
   ```bash
   cargo test -p agent-orchestrator --lib debug_info_covers_known_and_unknown_components
   ```
3. Inspect the loop-engine dispatch split and debug surface:
   ```bash
   rg -n "StaticSegment|DynamicDag|execute_cycle_graph|FallbackToStaticSegment" crates/orchestrator-scheduler/src/scheduler/loop_engine/tests.rs
   ```

### Expected
- `cargo test -p agent-orchestrator --no-run` succeeds
- `debug_info_covers_known_and_unknown_components` passes
- The scheduler contains an explicit execution-mode branch
- `dynamic_dag` supports graph execution with fallback
- Config/runtime structs compile cleanly with the new execution mode field
- `debug --component dag` is backed by a workflow-aware diagnostic view instead of a raw config dump

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Static Workflow Steps Materialize Into A Mainline Graph | ✅ | 2026-03-28 | Claude | |
| 2 | Invalid Dynamic Step Triggers Fail CEL Validation | ✅ | 2026-03-28 | Claude | |
| 3 | Task Trace Captures Graph Runs And Dynamic Events | ✅ | 2026-03-28 | Claude | |
| 4 | Task Info Exposes Persisted Graph Debug Bundles | ✅ | 2026-03-28 | Claude | |
| 5 | DAG Debug View And Workspace Compile Validation | ✅ | 2026-03-28 | Claude | |
