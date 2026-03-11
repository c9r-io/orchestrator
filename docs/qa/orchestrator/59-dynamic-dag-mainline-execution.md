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

### Preconditions
- Repository root is `/Volumes/Yotta/ai_native_sdlc`
- Latest code is compiled

### Goal
Verify static workflow steps can be converted into a runtime execution graph while excluding `init_once` and guard steps from the business DAG.

### Steps
1. Rebuild the core crate:
   ```bash
   cd core && cargo build --release
   ```
2. Run the focused graph materialization regression:
   ```bash
   cargo test -p agent-orchestrator build_static_execution_graph_skips_init_and_guard -- --nocapture
   ```
3. Inspect the runtime graph model definitions:
   ```bash
   rg -n "WorkflowExecutionMode|EffectiveExecutionGraph|build_static_execution_graph" core/src
   ```

### Expected
- The test passes
- Static step nodes are materialized in order
- `init_once` is excluded from the effective graph
- `loop_guard` stays outside the graph execution path
- The graph entry node is the first executable business step

---

## Scenario 2: Invalid Dynamic Step Triggers Fail CEL Validation

### Preconditions
- Latest code is compiled

### Goal
Verify `dynamic_steps.trigger` uses CEL validation instead of legacy string matching.

### Steps
1. Run the focused validation regression:
   ```bash
   cargo test -p agent-orchestrator validate_workflow_config_rejects_invalid_dynamic_step_trigger_cel -- --nocapture
   ```
2. Inspect trigger evaluation wiring:
   ```bash
   rg -n "evaluate_trigger_condition|validate_workflow_config_rejects_invalid_dynamic_step_trigger_cel|dynamic_steps" core/src
   ```

### Expected
- The test passes
- Invalid CEL in `dynamic_steps.trigger` is rejected during workflow validation
- Trigger evaluation is routed through the shared CEL prehook engine
- No legacy simple string matcher remains on the main trigger path

---

## Scenario 3: Task Trace Captures Graph Runs And Dynamic Events

### Preconditions
- Latest code is compiled

### Goal
Verify the trace builder emits graph-level execution information for dynamic DAG runs.

### Steps
1. Run the focused trace regression:
   ```bash
   cargo test -p agent-orchestrator build_trace_includes_dynamic_graph_events -- --nocapture
   ```
2. Inspect trace model fields:
   ```bash
   rg -n "graph_runs|GraphTrace|dynamic_plan_materialized|dynamic_edge_taken" core/src/scheduler/trace core/src/scheduler/loop_engine
   ```

### Expected
- The test passes
- `TaskTrace` includes `graph_runs`
- Graph trace captures `source`, `node_count`, `edge_count`, and `dynamic_*` event history
- Dynamic edge transitions are preserved in trace output

---

## Scenario 4: Task Info Exposes Persisted Graph Debug Bundles

### Preconditions
- Latest code is compiled

### Goal
Verify task detail queries can return persisted graph debug bundles without reconstructing everything from trace events.

### Steps
1. Run the focused repository regression:
   ```bash
   cargo test -p agent-orchestrator load_task_detail_rows_includes_graph_debug_bundles -- --nocapture
   ```
2. Run the service-level detail regression:
   ```bash
   cargo test -p agent-orchestrator get_task_details_impl_returns_items_and_empty_runs -- --nocapture
   ```
3. Inspect the query and proto wiring:
   ```bash
   rg -n "task_graph_runs|task_graph_snapshots|graph_debug|TaskGraphDebugBundle" core/src crates proto
   ```

### Expected
- The repository regression passes
- `task info` has a dedicated `graph_debug` payload
- Graph bundles come from task-level snapshot tables when available
- Legacy event-only tasks still retain best-effort graph visibility via query fallback

---

## Scenario 5: DAG Debug View And Workspace Compile Validation

### Preconditions
- Repository has no unresolved local compile breakage unrelated to this feature

### Goal
Verify the implementation integrates cleanly across config, runtime, scheduler, trace, task query, and CLI debug surfaces.

### Steps
1. Run a package-wide compile-only validation:
   ```bash
   cargo test -p agent-orchestrator --no-run
   ```
2. Run the DAG debug regression:
   ```bash
   cargo test -p agent-orchestrator debug_info_covers_known_and_unknown_components -- --nocapture
   ```
3. Inspect the loop-engine dispatch split and debug surface:
   ```bash
   rg -n "StaticSegment|DynamicDag|execute_cycle_graph|FallbackToStaticSegment|debug_dag_info" core/src/scheduler/loop_engine core/src/config core/src/service
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
| 1 | Static Workflow Steps Materialize Into A Mainline Graph | ☐ | | | |
| 2 | Invalid Dynamic Step Triggers Fail CEL Validation | ☐ | | | |
| 3 | Task Trace Captures Graph Runs And Dynamic Events | ☐ | | | |
| 4 | Task Info Exposes Persisted Graph Debug Bundles | ☐ | | | |
| 5 | DAG Debug View And Workspace Compile Validation | ☐ | | | |
