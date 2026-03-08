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
3. **manifest validate**: Optional - validates workflow YAML if run-cli.sh exists

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
   cd core && cargo test --lib self_test_step_type_validates_correctly
   ```

### Expected

- Test passes
- `validate_step_type("self_test")` returns Ok("self_test")

---

## Scenario 2: Self-Test YAML Parsing

### Preconditions

- None (unit test only)

### Goal

Verify YAML with self_test step parses correctly

### Steps

1. Run unit test:
   ```bash
   cd core && cargo test --lib parse_workflow_yaml_with_self_test_step
   ```

### Expected

- Test passes
- Workflow contains self_test step with id = "self_test"

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

Verify validation warns on missing self_test in self-referential workflows

### Steps

1. Run unit tests:
   ```bash
   cd core && cargo test --lib validate_self_referential_safety
   ```

### Expected

- `validate_self_referential_safety_warns_missing_self_test` passes
- `validate_self_referential_safety_passes_with_self_test` passes

---

## Scenario 5: Smoke Chain Execution (Survival Test)

### Preconditions

- Project initialized:
  ```bash
  QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
  orchestrator qa project reset "${QA_PROJECT}" --keep-config --force 2>/dev/null || true
  rm -rf "workspace/${QA_PROJECT}"
  orchestrator qa project create "${QA_PROJECT}" --force
  ```

### Goal

Validate self_test step executes and code compiles (survival smoke test)

### Steps

1. Run the survival smoke test directly:
   ```bash
   cd core && cargo test --lib self_test_survives_smoke_test
   ```

2. Alternatively, execute self_test via scheduler (requires workflow with self_test step):
   ```bash
   # Create workflow with self_test step if needed
   orchestrator task create --project "${QA_PROJECT}" --workflow <workflow-with-self_test> --goal "test self_test"
   ```

### Expected

- cargo check passes (exit code 0)
- Test completes without assertion failure
- Event "self_test_phase" emitted with phase = "cargo_check"

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
