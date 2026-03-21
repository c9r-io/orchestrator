---
self_referential_safe: true
---

# Orchestrator - Self-Test Step (SMOKE CHAIN)

**Module**: orchestrator
**Scope**: Validate self_test builtin step execution, pipeline variable propagation, and self-referential workflow safety
**Scenarios**: 5
**Priority**: High

---

## Background

The self_test step is a builtin step type that validates code compiles and tests pass. It's used in self-referential (SMOKE CHAIN) workflows to catch breaking changes early.

### Entry Points

- CLI: `orchestrator task create --project <project> --workflow <workflow-with-self_test>`

### Self-Test Execution Phases

1. **cargo check**: Validates code compiles
2. **cargo test --lib**: Runs unit tests (skips `self_test_survives_smoke_test` to avoid recursion)
3. **manifest validate**: Optional - validates workflow YAML if the orchestrator binary exists

### Pipeline Variables Set

| Variable | Type | Description |
|----------|------|-------------|
| `self_test_exit_code` | String | Exit code of self_test (0 = success) |
| `self_test_passed` | String | "true" if passed, "false" otherwise |

---

## Scenario 1: Self-Test Step Parsing

### Preconditions

- None (unit test only)

### Goal

Verify self_test step type validates correctly

### Steps

1. Run unit test:
   ```bash
   cargo test -p orchestrator-config --lib -- test_validate_step_type_known_ids 2>&1 | tail -5
   ```

### Expected

- Test passes
- `validate_step_type("self_test")` returns Ok — self_test is included in the known valid step type list

---

## Scenario 2: Self-Test YAML Parsing and Conversion

### Preconditions

- None (unit test only)

### Goal

Verify WorkflowSpec with self_test step converts correctly to WorkflowConfig with builtin execution

### Steps

1. Run unit test:
   ```bash
   cargo test -p agent-orchestrator --lib -- workflow_spec_to_config_self_test_sets_builtin_execution 2>&1 | tail -5
   ```

### Expected

- Test passes
- self_test step has `builtin = Some("self_test")`
- self_test step has `behavior.execution = ExecutionMode::Builtin { name: "self_test" }`
- `required_capability` is None (builtin steps don't use agent dispatch)

---

## Scenario 3: Self-Test Builtin Normalization

### Preconditions

- None (unit test only)

### Goal

Verify normalization sets builtin = "self_test" for self_test step id

### Steps

1. Run unit test:
   ```bash
   cargo test -p agent-orchestrator --lib -- normalize_workflow_sets_builtin_for_self_test 2>&1 | tail -5
   ```

### Expected

- Test passes
- self_test step has builtin = Some("self_test")

---

## Scenario 4: Self-Referential Safety Validation

### Preconditions

- None (unit test only)

### Goal

Verify self-referential validation rejects workflows that omit builtin `self_test`

### Steps

1. Run unit tests:
   ```bash
   cargo test -p agent-orchestrator --lib -- validate_self_referential_safety 2>&1 | tail -10
   ```

### Expected

- `validate_self_referential_safety_errors_missing_self_test` passes
- `validate_self_referential_safety_passes_with_self_test` passes

---

## Scenario 5: Self-Test Step Execution Logic (Unit Test + Code Review)

### Preconditions

- Rust toolchain available

### Goal

Verify `execute_self_test_step` three-phase execution logic (empty_change_check → cargo_check → cargo_test_lib) via unit test coverage and code review.

### Steps

1. Run self_test execution unit tests in the scheduler safety module:
   ```bash
   cargo test -p orchestrator-scheduler --lib -- execute_self_test 2>&1 | tail -10
   ```
2. Code review — verify the three-phase execution sequence:
   ```bash
   rg -n "empty_change_check|cargo_check|cargo_test_lib" crates/orchestrator-scheduler/src/scheduler/safety.rs | head -15
   ```
3. Code review — verify self_test emits phase events with `self_test_phase` event type:
   ```bash
   rg -n "self_test_phase|self_test_exit_code|self_test_passed" crates/orchestrator-scheduler/src/scheduler/safety.rs | head -10
   ```

### Expected

- `execute_self_test_step` unit tests pass (empty_change_check → cargo_check → cargo_test_lib phases)
- Phase events are emitted with `self_test_phase` event type and appropriate `passed`/`exit_code` fields
- `empty_change_check` returns early (exit_code=1) when no git changes are detected, skipping subsequent phases

---

## General Scenario: Pipeline Variable Propagation

### Goal

Verify self_test_exit_code and self_test_passed propagate to pipeline variables via prehook CEL context.

### Steps

1. Run prehook CEL unit tests that verify self_test pipeline variables:
   ```bash
   cargo test -p agent-orchestrator --lib -- test_prehook_cel_context_self_test_exit_code_variable test_evaluate_step_prehook_expression_self_test_passed_not_in_cel 2>&1 | tail -10
   ```
2. Code review — verify pipeline variable binding in prehook context:
   ```bash
   rg -n "self_test_exit_code|self_test_passed" core/src/prehook/context.rs | head -5
   ```

### Expected

- `test_prehook_cel_context_self_test_exit_code_variable` passes: `self_test_exit_code` is available in CEL context
- `test_evaluate_step_prehook_expression_self_test_passed_not_in_cel` passes: `self_test_passed` evaluated correctly
- Both variables are bound via `add_variable` in prehook context builder

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Self-Test Step Parsing | ✅ PASS | 2026-03-21 | Claude | |
| 2 | Self-Test YAML Parsing | ✅ PASS | 2026-03-21 | Claude | |
| 3 | Self-Test Builtin Normalization | ✅ PASS | 2026-03-21 | Claude | |
| 4 | Self-Referential Safety Validation | ✅ PASS | 2026-03-21 | Claude | 8 tests passed |
| 5 | Smoke Chain Execution | ✅ PASS | 2026-03-21 | Claude | 5 tests + code review verified three-phase execution |
| G | Pipeline Variable Propagation | ✅ PASS | 2026-03-21 | Claude | 2 tests + code review verified add_variable bindings |
