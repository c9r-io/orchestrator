---
self_referential_safe: true
---

# Orchestrator - Chain Steps Execution

**Module**: orchestrator
**Scope**: `chain_steps` runtime contract, pipeline variable inheritance, parent/child failure handling, and trace event linkage
**Scenarios**: 4
**Priority**: High

---

## Background

`chain_steps` is a first-class execution mode in the unified step engine. A parent step with `chain_steps` acts as a serial container for child steps. Child steps inherit the current `pipeline_vars`, execute with the same prehook/capture/store/post-action semantics as normal steps, and emit `chain_step_started` / `chain_step_finished` events linked back to the parent step.

This doc validates the contract introduced by FR-008 governance:

- child steps run serially
- pipeline variables flow child-to-child and back to the parent context
- child `on_failure` runs before parent `on_failure`
- `parent_step` is recorded in chain events for trace reconstruction

---

## Scenario 1: Runtime Execution Preserves Chain Shape

### Preconditions

- Repository root is the current working directory

### Steps

1. Code review confirms unit tests exist:
   - `build_execution_plan_includes_chain_steps` in `core/src/config_load/build.rs`
   - `chain_steps_checked` in `crates/orchestrator-scheduler/src/scheduler/check/mod.rs`
2. Run `cargo test --lib -p agent-orchestrator -- build_execution_plan_includes_chain_steps` (safe)
3. Run `cargo test --lib -p orchestrator-scheduler -- chain_steps_checked` (safe)

### Expected

- Both tests pass
- Chain child steps are preserved in the runtime execution plan
- The loaded runtime context resolves the parent step to `ExecutionMode::Chain`
- The runtime plan still contains the expected child-step count

> **Note:** Runtime chain event emission (`chain_step_started`/`chain_step_finished`) is validated
> separately in Scenario 4 via the trace-compatibility test.

---

## Scenario 2: Chain Children Are Valid Self-Contained Steps

### Preconditions

- Repository root is the current working directory

### Steps

1. Code review confirms unit tests exist:
   - `validate_workflow_accepts_chain_steps_without_agent` in `core/src/config_load/validate/tests.rs`
   - `workflow_chain_steps_round_trip_through_spec_conversion` in `core/src/resource/workflow/workflow_convert.rs`
2. Run `cargo test --lib -p agent-orchestrator -- validate_workflow_accepts_chain_steps_without_agent` (safe)
3. Run `cargo test --lib -p agent-orchestrator -- workflow_chain_steps_round_trip_through_spec_conversion` (safe)

### Expected

- All tests pass
- A parent chain step is accepted without its own direct agent requirement
- Child command steps are preserved in the runtime execution plan
- `chain_steps` survive resource spec/config conversion without being flattened away

---

## Scenario 3: Guide Contract Matches Runtime Semantics

### Preconditions

- Documentation updated in this repo

### Steps

1. Inspect the chain mode section in:
   ```bash
   sed -n '45,85p' docs/guide/03-workflow-configuration.md
   ```

2. Inspect the Chinese guide section:
   ```bash
   sed -n '45,90p' docs/guide/zh/03-workflow-configuration.md
   ```

### Expected

- Both guides describe `chain_steps` as a parent-step container
- Both guides state that child steps inherit `pipeline_vars`
- Both guides state that child failure is handled before parent `on_failure`

---

## Scenario 4: Chain Event Names Remain Trace-Compatible

### Preconditions

- Repository root is the current working directory

### Steps

1. Code review confirms unit test exists:
   - `chain_and_dynamic_step_events_handled` in `crates/orchestrator-scheduler/src/scheduler/trace/tests.rs`
2. Run `cargo test --lib -p orchestrator-scheduler -- chain_and_dynamic_step_events_handled` (safe)

### Expected

- The test passes
- Trace builder still accepts `chain_step_started` / `chain_step_finished`
- Chain events remain visible in cycle step reconstruction

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Runtime Execution Preserves Chain Shape | ✅ PASS | 2026-03-21 | Claude | cargo test --lib passed for both tests |
| 2 | Chain Children Are Valid Self-Contained Steps | ✅ PASS | 2026-03-21 | Claude | cargo test --lib passed for both tests |
| 3 | Guide Contract Matches Runtime Semantics | ✅ PASS | 2026-03-21 | Claude | EN+ZH guides: container, pipeline_vars, on_failure order |
| 4 | Chain Event Names Remain Trace-Compatible | ✅ PASS | 2026-03-21 | Claude | cargo test --lib passed |
