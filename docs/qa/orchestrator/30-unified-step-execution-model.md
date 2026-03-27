---
self_referential_safe: true
---

# Orchestrator - Unified Step Execution Model

**Module**: orchestrator
**Scope**: Validate string-based step identification, unified step semantic resolution, StepBehavior execution alignment, and unified execution loop correctness after WorkflowStepType removal
**Scenarios**: 5
**Priority**: High

---

## Background

The orchestrator step execution model was refactored to remove the `WorkflowStepType` enum. Steps are now identified by their string `id` field, and behavior is declared via `StepBehavior` data structures rather than hardcoded match arms. The latest regression update also requires `builtin`, `required_capability`, `command`, `chain_steps`, and `behavior.execution` to resolve through one shared semantic rule set.

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
| Implicit default `ExecutionMode::Agent` drift | Explicit semantic normalization and execution-mode alignment |

### Key Files

| File | Role |
|------|------|
| `core/src/config.rs` | `validate_step_type()`, `default_scope_for_step_id()`, `resolve_step_semantic_kind()`, `normalize_step_execution_mode()` |
| `core/src/scheduler/item_executor.rs` | `StepExecutionAccumulator`, unified `process_item_filtered()` loop |
| `core/src/config_load.rs` | `normalize_workflow_config()`, `validate_workflow_config()`, `build_execution_plan()` |
| `core/src/resource/workflow/workflow_convert.rs` | `workflow_spec_to_config()` semantic normalization |
| `core/src/scheduler/check/` | Static semantic consistency checks (split into workspace, capability, execution, workflow, safety sub-modules) |

---

## Scenario 1: Step Type Validation (Known and Unknown IDs)

### Preconditions

- None (unit test only)

### Goal

Verify that `validate_step_type()` accepts all 20 known step IDs and rejects unknown IDs.

### Steps

1. Run validation unit tests:
   ```bash
   cargo test -p orchestrator-config test_validate_step_type -- --nocapture
   ```

### Expected

- `test_validate_step_type_known_ids` passes — all 20 known IDs (init_once, plan, qa, ticket_scan, fix, retest, loop_guard, build, test, lint, implement, review, git_ops, qa_doc_gen, qa_testing, ticket_fix, doc_governance, align_tests, self_test, smoke_chain) are accepted
- `test_validate_step_type_unknown_id` passes — `"my_custom_step"` is rejected with "unknown workflow step type" error

### Expected Data State

```bash
cargo test -p orchestrator-config test_validate_step_type 2>&1 | grep "test result"
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
   cargo test -p orchestrator-config test_default_scope -- --nocapture
   ```

### Expected

- **Task-scoped** (run once per cycle): plan, qa_doc_gen, implement, self_test, align_tests, doc_governance, review, build, test, lint, git_ops, smoke_chain, loop_guard, init_once
- **Item-scoped** (fan out per QA file): qa, qa_testing, ticket_fix, ticket_scan, fix, retest
- Unknown step IDs default to **Task** scope
- `resolved_scope()` returns explicit override when `scope: Some(StepScope::Task)` is set on an item-scoped step

### Expected Data State

```bash
cargo test -p orchestrator-config test_default_scope test_resolved_scope 2>&1 | grep "test result"
# Expected: test result: ok. 5 passed; 0 failed
```

---

## Scenario 3: Semantic Normalization And Execution Rehydration

### Preconditions

- None (unit test only)

### Goal

Verify normalization and execution-plan building keep declarative step semantics and runtime `ExecutionMode` in sync.

### Steps

1. Run normalization-focused tests:
   ```bash
   cargo test -p agent-orchestrator --lib normalize_workflow_ -- --nocapture
   ```

2. Run execution-plan-focused tests:
   ```bash
   cargo test -p agent-orchestrator --lib build_execution_plan_ -- --nocapture
   ```

3. Run the full lib test suite to confirm no regressions:
   ```bash
   cargo test --workspace --lib 2>&1 | grep "test result"
   ```

### Expected

- `normalize_workflow_sets_builtin_for_self_test` passes
- `normalize_workflow_sets_builtin_execution_for_loop_guard` passes
- `normalize_workflow_sets_agent_execution_for_plan` passes
- `build_execution_plan_rehydrates_builtin_execution_from_builtin_field` passes
- Chain parent steps keep `ExecutionMode::Chain`, while command children are emitted as self-contained builtin-style execution
- Full library suite passes without regression

### Expected Data State

```bash
cargo test --workspace --lib 2>&1 | grep "test result"
# Expected: all workspace lib tests pass
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
   grep -r "WorkflowStepType" src/ tests/ --include="*.rs"
   ```

2. Search for `step_type` on config/execution step structs (excluding dynamic orchestration `DynamicStepConfig.step_type` which is a legitimate string field):
   ```bash
   grep -rn "\.step_type" src/config.rs src/config_load.rs src/scheduler/loop_engine.rs
   ```
   > **Note**: `src/scheduler/item_executor.rs` uses `ds.step_type` on `DynamicStepConfig` instances from the dynamic orchestration step pool — this is intentional and unrelated to the removed `WorkflowStepType` enum.

3. Verify no `step()` method calls remain on `TaskExecutionPlan`:
   ```bash
   grep -rn "execution_plan\.step(" src/ --include="*.rs"
   ```

### Expected

- Step 1: **Zero matches** — `WorkflowStepType` fully deleted
- Step 2: **Zero matches** — `step_type` field removed from config structs
- Step 3: **Zero matches** — all callers use `step_by_id()` instead

---

## Scenario 5: Static Checks And Resource Conversion Stay In Sync

### Preconditions

- None (unit test only)

### Goal

Verify resource conversion and static checks share the same step semantic rules as runtime normalization.

### Steps

1. Run workflow resource conversion tests:
   ```bash
   cargo test -p agent-orchestrator --lib workflow_spec_to_config_ -- --nocapture
   ```

2. Run targeted static-check tests:
   ```bash
   cargo test -p orchestrator-scheduler --lib step_semantic_conflict -- --nocapture
   cargo test -p orchestrator-scheduler --lib execution_mode_mismatch -- --nocapture
   cargo test -p orchestrator-scheduler --lib command_steps_skip_capability_requirement -- --nocapture
   cargo test -p orchestrator-scheduler --lib clean_config_no_errors -- --nocapture
   ```

3. Run the full lib test suite:
   ```bash
   cargo test --workspace --lib
   ```

### Expected

- `workflow_spec_to_config_self_test_sets_builtin_execution` passes
- `workflow_spec_to_config_init_once_sets_builtin` passes
- `step_semantic_conflict` passes and catches steps that set both `builtin` and `required_capability`
- `execution_mode_mismatch` passes and catches stale `behavior.execution`
- `command_steps_skip_capability_requirement` passes and confirms command steps remain self-contained
- `clean_config_no_errors` passes with the scheduler-check fixture aligned to builtin `loop_guard`
- Full library suite passes without regression

### Expected Data State

```bash
cargo test --workspace --lib 2>&1 | grep "test result"
# Expected: all workspace lib tests pass
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Step Type Validation (Known and Unknown IDs) | ✅ PASS | 2026-03-28 | QA | 2 tests passed |
| 2 | Default Scope Classification (Task vs Item) | ✅ PASS | 2026-03-28 | QA | 2 tests passed |
| 3 | Semantic Normalization And Execution Rehydration | ✅ PASS | 2026-03-28 | QA | 11 tests passed (4 normalize + 7 build), 425 workspace lib tests pass |
| 4 | WorkflowStepType Fully Removed From Codebase | ✅ PASS | 2026-03-28 | QA | Grep audit — zero matches for WorkflowStepType and execution_plan.step() |
| 5 | Static Checks And Resource Conversion Stay In Sync | ✅ PASS | 2026-03-28 | QA | 12 tests passed (8 workflow_spec_to_config + 4 scheduler check), 425 workspace lib tests pass |
