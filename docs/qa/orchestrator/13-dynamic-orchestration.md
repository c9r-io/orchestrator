# Orchestrator - Dynamic Orchestration & Adaptive Workflow

**Module**: orchestrator
**Scope**: Validate dynamic prehook decisions, DAG execution, dynamic step pool, adaptive planning interfaces
**Scenarios**: 5
**Priority**: High

---

## Background

This document tests the new dynamic orchestration features:
- PrehookDecision with extended decision types (Run/Skip/Branch/DynamicAdd/Transform)
- DynamicStepPool for runtime step selection
- DAG execution engine with topological sort and cycle detection
- Conditional edge evaluation
- AdaptivePlanner interfaces (Phase 4)

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
   grep -n "enum PrehookDecision" core/src/dynamic_orchestration.rs
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
   ./scripts/orchestrator.sh config export -f /tmp/exported-config.yaml
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
   grep -n "struct DynamicExecutionPlan" core/src/dynamic_orchestration.rs
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

## Scenario 5: Adaptive Planner Disabled Mode

### Preconditions

- AdaptivePlanner defined but disabled by default

### Steps

1. Run adaptive planner test:
   ```bash
   cd core && cargo test test_adaptive_planner_disabled
   ```

2. Verify disabled behavior:
   ```
   test dynamic_orchestration::tests::test_adaptive_planner_disabled ... ok
   ```

3. Check AdaptivePlannerConfig defaults:
   ```bash
   grep -A 5 "impl Default for AdaptivePlannerConfig" core/src/dynamic_orchestration.rs
   ```

### Expected

- AdaptivePlannerConfig.enabled defaults to false
- generate_plan() returns error when disabled
- LlmClient trait defined but not implemented (Phase 4 placeholder)

### DB Checks

N/A - Unit test verification

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
| 5. Adaptive Planner Disabled | | |
