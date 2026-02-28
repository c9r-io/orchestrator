# Orchestrator - Unified Step Execution Model

**Module**: orchestrator
**Scope**: Validate string-based step identification, StepBehavior data structures, StepExecutionAccumulator, and unified execution loop correctness after WorkflowStepType removal
**Scenarios**: 5
**Priority**: High

---

## Background

The orchestrator step execution model was refactored to remove the `WorkflowStepType` enum. Steps are now identified by their string `id` field, and behavior is declared via `StepBehavior` data structures rather than hardcoded match arms.

**Design doc**: `docs/design_doc/orchestrator/13-unified-step-execution-model.md`

### Key Changes

| Before | After |
|--------|-------|
| `WorkflowStepType::Plan` enum variant | `step.id == "plan"` string comparison |
| `step_type: Some(WorkflowStepType::Qa)` field | Field removed; step identified by `id` |
| `execution_plan.step(WorkflowStepType::X)` | `execution_plan.step_by_id("x")` |
| `WorkflowStepType::from_str("self_test")` | `validate_step_type("self_test")` |
| `step_type.default_scope()` method | `default_scope_for_step_id(&step.id)` free function |
| ~900-line `process_item_filtered()` with hardcoded steps | Unified loop with `StepExecutionAccumulator` |

### Key Files

| File | Role |
|------|------|
| `core/src/config.rs` | `validate_step_type()`, `default_scope_for_step_id()`, `has_structured_output()`, `StepBehavior` types |
| `core/src/scheduler/item_executor.rs` | `StepExecutionAccumulator`, unified `process_item_filtered()` loop |
| `core/src/config_load.rs` | `normalize_workflow_config()` with string-based identification |

---

## Scenario 1: Step Type Validation (Known and Unknown IDs)

### Preconditions

- None (unit test only)

### Goal

Verify that `validate_step_type()` accepts all 20 known step IDs and rejects unknown IDs.

### Steps

1. Run validation unit tests:
   ```bash
   cd core && cargo test --lib test_validate_step_type -- --nocapture
   ```

### Expected

- `test_validate_step_type_known_ids` passes — all 20 known IDs (init_once, plan, qa, ticket_scan, fix, retest, loop_guard, build, test, lint, implement, review, git_ops, qa_doc_gen, qa_testing, ticket_fix, doc_governance, align_tests, self_test, smoke_chain) are accepted
- `test_validate_step_type_unknown_id` passes — `"my_custom_step"` is rejected with "unknown workflow step type" error

### Expected Data State

```bash
cd core && cargo test --lib test_validate_step_type 2>&1 | grep "test result"
# Expected: test result: ok. 2 passed; 0 failed
```

---

## Scenario 2: Default Scope Classification (Task vs Item)

### Preconditions

- None (unit test only)

### Goal

Verify that `default_scope_for_step_id()` correctly classifies all known step IDs into Task-scoped or Item-scoped.

### Steps

1. Run scope classification unit tests:
   ```bash
   cd core && cargo test --lib test_default_scope -- --nocapture
   ```

### Expected

- **Task-scoped** (run once per cycle): plan, qa_doc_gen, implement, self_test, align_tests, doc_governance, review, build, test, lint, git_ops, smoke_chain, loop_guard, init_once
- **Item-scoped** (fan out per QA file): qa, qa_testing, ticket_fix, ticket_scan, fix, retest
- Unknown step IDs default to **Task** scope
- `resolved_scope()` returns explicit override when `scope: Some(StepScope::Task)` is set on an item-scoped step

### Expected Data State

```bash
cd core && cargo test --lib test_default_scope test_resolved_scope 2>&1 | grep "test result"
# Expected: test result: ok. 5 passed; 0 failed
```

---

## Scenario 3: StepExecutionAccumulator and Unified Loop

### Preconditions

- None (unit test only)

### Goal

Verify the unified execution loop in `process_item_filtered()` correctly handles agent execution, builtin steps (self_test, ticket_scan), prehook evaluation, and pipeline variable propagation via `StepExecutionAccumulator`.

### Steps

1. Run all item_executor unit tests:
   ```bash
   cd core && cargo test --test '*' item_executor -- --nocapture 2>&1 || true
   cd core && cargo test --lib -- item_executor --nocapture 2>&1 | tail -5
   ```

2. Run the full lib test suite to confirm no regressions:
   ```bash
   cd core && cargo test --lib 2>&1 | grep "test result"
   ```

### Expected

- All 80 item_executor tests pass
- All 670 lib tests pass
- No test references `WorkflowStepType` or `step_type` field

### Expected Data State

```bash
cd core && cargo test --lib 2>&1 | grep "test result"
# Expected: test result: ok. 670 passed; 0 failed
```

---

## Scenario 4: WorkflowStepType Fully Removed From Codebase

### Preconditions

- None (source code audit)

### Goal

Verify that no references to `WorkflowStepType` enum or `step_type` field on `WorkflowStepConfig`/`TaskExecutionStep` remain in production or test code.

### Steps

1. Search for any remaining `WorkflowStepType` references:
   ```bash
   cd core && grep -r "WorkflowStepType" src/ tests/ --include="*.rs"
   ```

2. Search for `step_type` on config/execution step structs (excluding unrelated uses in cli_types, dynamic_orchestration, etc.):
   ```bash
   cd core && grep -rn "\.step_type" src/config.rs src/config_load.rs src/scheduler/item_executor.rs src/scheduler/loop_engine.rs
   ```

3. Verify no `step()` method calls remain on `TaskExecutionPlan`:
   ```bash
   cd core && grep -rn "execution_plan\.step(" src/ --include="*.rs"
   ```

### Expected

- Step 1: **Zero matches** — `WorkflowStepType` fully deleted
- Step 2: **Zero matches** — `step_type` field removed from config structs
- Step 3: **Zero matches** — all callers use `step_by_id()` instead

---

## Scenario 5: Self-Bootstrap Pipeline Backward Compatibility

### Preconditions

- Self-bootstrap test fixture available: `fixtures/manifests/bundles/self-bootstrap-test.yaml`

### Goal

Verify that self-bootstrap workflows continue to work after the refactoring — steps are identified by `id`, normalization sets correct defaults, and the full SDLC pipeline executes.

### Steps

1. Run integration tests that exercise self-bootstrap fixture parsing:
   ```bash
   cd core && cargo test --test integration_test -- --nocapture
   ```

2. Run the normalize workflow tests:
   ```bash
   cd core && cargo test --lib normalize_workflow -- --nocapture
   ```

3. Verify the self-bootstrap fixture applies without errors:
   ```bash
   QA_PROJECT="qa-unified-${USER}-$(date +%Y%m%d%H%M%S)"
   ./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
   ./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/self-bootstrap-test.yaml --dry-run
   ```

### Expected

- 24 integration tests pass
- All normalization tests pass (scope defaults, builtin flags, is_guard flags set by string matching)
- Self-bootstrap fixture dry-run succeeds without validation errors
- Steps correctly classified: plan → Task, qa_testing → Item, etc. (via `default_scope_for_step_id`)

### Expected Data State

```bash
cd core && cargo test --test integration_test 2>&1 | grep "test result"
# Expected: test result: ok. 24 passed; 0 failed
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Step Type Validation (Known and Unknown IDs) | ☐ | | | |
| 2 | Default Scope Classification (Task vs Item) | ☐ | | | |
| 3 | StepExecutionAccumulator and Unified Loop | ☐ | | | |
| 4 | WorkflowStepType Fully Removed From Codebase | ☐ | | | |
| 5 | Self-Bootstrap Pipeline Backward Compatibility | ☐ | | | |
