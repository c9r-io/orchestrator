---
self_referential_safe: true
---

# Orchestrator - Dynamic Orchestration & Adaptive Workflow

**Module**: orchestrator
**Scope**: Validate explicit `dynamic_dag` execution mode, dynamic step pool, and adaptive runtime planning
**Scenarios**: 4
**Priority**: High

---

## Background

This document tests the dynamic orchestration feature set:
- DynamicStepPool for runtime step selection
- explicit `dynamic_dag` mainline execution mode
- Conditional edge evaluation
- AdaptivePlanner runtime execution, validation, and fallback behavior
- task-level graph persistence and `task info` / `debug dag` observability for dynamic DAG runs

For FR-004 mainline execution and graph-aware trace coverage, also execute:
- `docs/qa/orchestrator/59-dynamic-dag-mainline-execution.md`

Entry point: `orchestrator` CLI, config file modifications, Rust unit tests

---

## Scenario 1: DynamicStepPool Matching

### Preconditions

- Unit tests available
- DynamicStepConfig structure defined

### Steps

1. Run dynamic step pool test:
   ```bash
   cd core && cargo test test_dynamic_step_pool
   ```

2. Verify pool matches based on trigger conditions:
   ```
   test dynamic_orchestration::tests::test_dynamic_step_pool ... ok
   ```

3. Review DynamicStepConfig structure in code:
   ```bash
   rg -n "struct DynamicStepConfig" core/src/dynamic_orchestration
   rg -n "dynamic_steps" core/src/config.rs
   ```

### Expected

- DynamicStepPool.find_matching_steps() returns steps matching context
- Trigger conditions are evaluated through CEL (for example, `active_ticket_count > 0`)
- Priority sorting works (higher priority first)

### DB Checks

N/A - Unit test verification

---

## Scenario 2: DAG Topological Sort

### Preconditions

- DAG structures available in code

### Steps

1. Run topological sort test:
   ```bash
   cd core && cargo test test_dag_topological_sort
   ```

2. Verify sorted order is correct:
   ```
   test dynamic_orchestration::tests::test_dag_topological_sort ... ok
   ```

3. Check DynamicExecutionPlan structure:
   ```bash
   rg -n "struct DynamicExecutionPlan" core/src/dynamic_orchestration
   ```

### Expected

- `topological_sort()` returns nodes in valid execution order
- No cycles in graph produces sorted list
- Entry nodes (no incoming edges) appear before dependent nodes

### DB Checks

N/A - Unit test verification

---

## Scenario 3: Cycle Detection

### Preconditions

- DAG with cycles testable

### Steps

1. Run cycle detection test:
   ```bash
   cd core && cargo test test_dag_cycle_detection
   ```

2. Verify cycle detection:
   ```
   test dynamic_orchestration::tests::test_dag_cycle_detection ... ok
   ```

3. Create graph with cycle manually:
   ```rust
   // a -> b -> a forms a cycle
   ```

### Expected

- `has_cycles()` returns true for cyclic graphs
- `has_cycles()` returns false for acyclic graphs
- topological_sort() fails with error for cyclic graphs

### DB Checks

N/A - Unit test verification

---

## Scenario 4: Adaptive Planner Runtime, Validation, And Fallback

### Preconditions

- Adaptive planner implementation present in `core/src/dynamic_orchestration/adaptive.rs`
- Workflow config supports `adaptive:` block
- Unit tests available: `cargo test adaptive_planner`, `cargo test workflow_convert`, `cargo test validate_workflow`

### Steps

1. Run targeted adaptive and validation tests:
   ```bash
   cd core && cargo test adaptive_planner --lib
   cd core && cargo test workflow_convert --lib
   cd core && cargo test validate_workflow --lib
   ```

2. Verify planner success, fallback, and validation coverage:
   ```
   test dynamic_orchestration::adaptive::tests::test_adaptive_planner_generate_plan_enabled ... ok
   test dynamic_orchestration::adaptive::tests::test_adaptive_planner_soft_fallback_on_invalid_json ... ok
   test dynamic_orchestration::adaptive::tests::test_adaptive_planner_fail_closed_on_invalid_json ... ok
   test dynamic_orchestration::adaptive::tests::test_adaptive_planner_rejects_missing_planner_agent ... ok
   ```

3. Check runtime event names for adaptive orchestration:
   ```bash
   rg -n "adaptive_plan_requested|adaptive_plan_succeeded|adaptive_plan_failed|adaptive_plan_fallback_used" crates/orchestrator-scheduler/src/scheduler/item_executor/dispatch.rs
   ```

4. Check validation logic for planner agent capability:
   ```bash
   rg -n "adaptive_plan" core/src/config_load/validate/adaptive_workflow.rs
   ```

### Expected

- Unit tests confirm adaptive planner generate plan, soft fallback on invalid JSON, fail-closed on invalid JSON, and rejection of missing planner agent
- Adaptive planner uses an agent-backed executor instead of a vendor-specific client
- Valid JSON DAG output is accepted as a runtime execution plan
- Invalid JSON or invalid DAG can trigger deterministic fallback when `fallback_mode=soft_fallback`
- Missing `planner_agent` or planner agents without capability `adaptive_plan` are rejected by workflow validation
- Runtime event names (`adaptive_plan_requested`, `adaptive_plan_succeeded`, etc.) are present in dispatch code
- Validation logic confirms `adaptive_plan` capability requirement

---

## Checklist

| Scenario | Status | Notes |
|----------|--------|-------|
| 1. DynamicStepPool Matching | PASS | 8 step pool tests passed; `DynamicStepConfig` and `dynamic_steps` verified |
| 2. DAG Topological Sort | PASS | 5 DAG sort tests passed including diamond, empty, cycle error cases |
| 3. Cycle Detection | PASS | `test_dag_cycle_detection` passed; cycle detection verified |
| 4. Adaptive Planner Runtime, Validation, And Fallback | PASS | 7 adaptive + 27 workflow_convert + 36 validate tests passed; event names and validation logic verified |
