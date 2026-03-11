# Orchestrator - Dynamic DAG Mainline Execution

**Module**: orchestrator
**Scope**: Explicit `dynamic_dag` workflow mode, CEL-based dynamic trigger validation, graph materialization, runtime fallback, and graph-aware trace output
**Scenarios**: 4
**Priority**: High

---

## Background

FR-004 promotes DAG orchestration from a side capability into an explicit execution mode. The implementation introduces:

- `workflow.execution.mode: dynamic_dag`
- deterministic graph materialization from static workflow steps
- adaptive planner graph conversion into the same runtime graph model
- CEL-based validation for `dynamic_steps.trigger` and conditional edges
- graph-aware task trace output via `graph_runs` and `dynamic_*` events

This QA document focuses on deterministic, reproducible verification using unit tests and CLI-safe commands only.

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

## Scenario 4: Full Crate Compiles With Dynamic DAG Mainline Wiring

### Preconditions
- Repository has no unresolved local compile breakage unrelated to this feature

### Goal
Verify the implementation integrates cleanly across config, runtime, scheduler, trace, and tests.

### Steps
1. Run a package-wide compile-only validation:
   ```bash
   cargo test -p agent-orchestrator --no-run
   ```
2. Inspect the loop-engine dispatch split:
   ```bash
   rg -n "StaticSegment|DynamicDag|execute_cycle_graph|FallbackToStaticSegment" core/src/scheduler/loop_engine core/src/config
   ```

### Expected
- `cargo test -p agent-orchestrator --no-run` succeeds
- The scheduler contains an explicit execution-mode branch
- `dynamic_dag` supports graph execution with fallback
- Config/runtime structs compile cleanly with the new execution mode field

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Static Workflow Steps Materialize Into A Mainline Graph | ☐ | | | |
| 2 | Invalid Dynamic Step Triggers Fail CEL Validation | ☐ | | | |
| 3 | Task Trace Captures Graph Runs And Dynamic Events | ☐ | | | |
| 4 | Full Crate Compiles With Dynamic DAG Mainline Wiring | ☐ | | | |
