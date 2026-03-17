---
self_referential_safe: false
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
   cargo test -p orchestrator-config test_validate_step_type_known_ids
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
   cd core && cargo test --lib workflow_spec_to_config_self_test_sets_builtin_execution
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
   cd core && cargo test --lib normalize_workflow_sets_builtin_for_self_test
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
   cd core && cargo test --lib validate_self_referential_safety
   ```

### Expected

- `validate_self_referential_safety_errors_missing_self_test` passes
- `validate_self_referential_safety_passes_with_self_test` passes

---

## Scenario 5: Smoke Chain Execution (Survival Test)

### Preconditions

- Project initialized:
  ```bash
  QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
  orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
  rm -rf "workspace/${QA_PROJECT}"
  orchestrator apply -f fixtures/manifests/bundles/self-bootstrap-test.yaml --project "${QA_PROJECT}"
  ```

### Goal

Validate self_test step executes and code compiles (survival smoke test)

### Steps

1. Execute self_test via scheduler using a workflow with self_test step:
   ```bash
   orchestrator task create --project "${QA_PROJECT}" --workflow sdlc_full_pipeline --goal "smoke chain survival test"
   ```
2. Wait for task completion and query self_test events:
   ```bash
   TASK_ID=$(orchestrator task list --project "${QA_PROJECT}" -o json | jq -r '.[0].id')
   orchestrator task watch "${TASK_ID}" --interval 2 --timeout 120
   sqlite3 data/agent_orchestrator.db "SELECT event_type, json_extract(payload_json, '$.step'), json_extract(payload_json, '$.phase') FROM events WHERE task_id = '${TASK_ID}' AND json_extract(payload_json, '$.step') = 'self_test' ORDER BY created_at;"
   ```

### Expected

- self_test step starts and finishes (step_started + step_finished events)
- **Mock fixture limitation**: Mock agents do not create real git changes, so `empty_change_check` detects no diff and self_test fails early with exit_code=1. This is expected behavior (FR-044).
- `self_test_phase` events emitted with phase = "empty_change_check" (passed = false)
- `cargo_check` phase is **not reached** because `empty_change_check` returns early
- The task may still complete (depending on workflow `on_failure` policy) since self_test failure is non-fatal in the pipeline

> **Note**: To verify the full self_test chain (cargo_check + cargo_test_lib), use a workflow where the implement step makes real code changes and commits them. This scenario only validates the smoke chain wiring.

---

## General Scenario: Pipeline Variable Propagation

### Goal

Verify self_test_exit_code and self_test_passed propagate to pipeline variables

### Steps

1. Execute workflow with self_test step
2. Check pipeline variables:
   ```bash
   # Query via CLI or check scheduler.rs logic
   # Variables should be set in PipelineVars
   ```

### Expected

- `self_test_exit_code` = "0" on success
- `self_test_passed` = "true" on success

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Self-Test Step Parsing | ☐ | | | |
| 2 | Self-Test YAML Parsing | ☐ | | | |
| 3 | Self-Test Builtin Normalization | ☐ | | | |
| 4 | Self-Referential Safety Validation | ☐ | | | |
| 5 | Smoke Chain Execution | ☐ | | | |
| G | Pipeline Variable Propagation | ☐ | | | |
