# Orchestrator - Dynamic Orchestration & Adaptive Workflow

**Module**: orchestrator
**Scope**: Validate dynamic prehook decisions, DAG execution, dynamic step pool, and adaptive runtime planning
**Scenarios**: 5
**Priority**: High

---

## Background

This document tests the new dynamic orchestration features:
- PrehookDecision with extended decision types (Run/Skip/Branch/DynamicAdd/Transform)
- DynamicStepPool for runtime step selection
- DAG execution engine with topological sort and cycle detection
- Conditional edge evaluation
- AdaptivePlanner runtime execution, validation, and fallback behavior

Entry point: `orchestrator` CLI, config file modifications, Rust unit tests

---

## Scenario 1: PrehookDecision Extended Types

### Preconditions

- Orchestrator binary built successfully
- Unit tests available: `cargo test dynamic_orchestration`

### Steps

1. Run unit tests for PrehookDecision:
   ```bash
   cd core && cargo test test_prehook_decision_from_bool
   ```

2. Verify test output shows Run/Skip behavior:
   ```
   test dynamic_orchestration::tests::test_prehook_decision_from_bool ... ok
   ```

3. Check PrehookDecision enum in code:
   ```bash
   rg -n "enum PrehookDecision" core/src/dynamic_orchestration
   ```

### Expected

- `PrehookDecision::from(true)` returns `Run` variant
- `PrehookDecision::from(false)` returns `Skip` variant with reason
- `should_run()` method returns true for Run/DynamicAdd/Transform variants

### DB Checks

N/A - Unit test verification

---

## Scenario 2: DynamicStepPool Matching

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

3. Check DynamicStepConfig in config:
   ```bash
   ./scripts/orchestrator.sh manifest export -f /tmp/exported-config.yaml
   grep -A 10 "dynamic_steps:" /tmp/exported-config.yaml
   ```

### Expected

- DynamicStepPool.find_matching_steps() returns steps matching context
- Trigger conditions evaluated correctly (e.g., `active_ticket_count > 0`)
- Priority sorting works (higher priority first)

### DB Checks

N/A - Unit test verification

---

## Scenario 3: DAG Topological Sort

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

## Scenario 4: Cycle Detection

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

## Scenario 5: Adaptive Planner Runtime, Validation, And Fallback

### Preconditions

- Adaptive planner implementation present in `core/src/dynamic_orchestration/adaptive.rs`
- Workflow config supports `adaptive:` block
- Adaptive planner agent uses capability `adaptive_plan`

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

3. Check workflow export contains adaptive configuration:
   ```bash
   ./scripts/orchestrator.sh manifest export -f /tmp/exported-config.yaml
   grep -A 8 "adaptive:" /tmp/exported-config.yaml || true
   ```

4. Check runtime event names for adaptive orchestration:
   ```bash
   rg -n "adaptive_plan_requested|adaptive_plan_succeeded|adaptive_plan_failed|adaptive_plan_fallback_used" core/src/scheduler/item_executor/dispatch.rs
   ```

5. Check validation logic for planner agent capability:
   ```bash
   rg -n "adaptive_plan" core/src/config_load/validate.rs
   ```

### Expected

- `workflow.adaptive` is part of manifest/config roundtrip and can carry `planner_agent` and `fallback_mode`
- Adaptive planner uses an agent-backed executor instead of a vendor-specific client
- Valid JSON DAG output is accepted as a runtime execution plan
- Invalid JSON or invalid DAG can trigger deterministic fallback when `fallback_mode=soft_fallback`
- Missing `planner_agent` or planner agents without capability `adaptive_plan` are rejected by workflow validation
- Runtime emits `adaptive_plan_requested`, `adaptive_plan_succeeded`, `adaptive_plan_failed`, and `adaptive_plan_fallback_used` events
- Strict JSON validation applies to phase `adaptive_plan`

### DB Checks

1. Create a task that uses an adaptive-enabled workflow and inspect the events table:
   ```bash
   sqlite3 data/agent_orchestrator.db "
   SELECT event_type, payload_json
   FROM events
   WHERE event_type LIKE 'adaptive_plan_%'
   ORDER BY id DESC
   LIMIT 20;
   "
   ```

2. Verify at least one of the following event flows appears:
   - `adaptive_plan_requested` -> `adaptive_plan_succeeded`
   - `adaptive_plan_requested` -> `adaptive_plan_fallback_used`
   - `adaptive_plan_requested` -> `adaptive_plan_failed`

3. When fallback is exercised, verify payload includes planner error metadata:
   - `error_class`
   - `fallback_mode`
   - `node_count`
   - `edge_count`

---

## Cleanup

All tests are unit tests, no cleanup required.

---

## Checklist

| Scenario | Status | Notes |
|----------|--------|-------|
| 1. PrehookDecision Extended Types | | |
| 2. DynamicStepPool Matching | | |
| 3. DAG Topological Sort | | |
| 4. Cycle Detection | | |
| 5. Adaptive Planner Runtime, Validation, And Fallback | | |
