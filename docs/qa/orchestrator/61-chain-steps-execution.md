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

## Scenario 1: Execution Plan Preserves Chain Shape

### Preconditions

- Orchestrator crate compiles

### Steps

1. Run the execution-plan regression:
   ```bash
   cargo test -p agent-orchestrator smoke_chain_execution_plan_preserves_chain_steps_at_runtime -- --nocapture
   ```

### Expected

- The test passes
- The serialized `execution_plan_json` contains `chain_steps`
- The loaded runtime context resolves the parent step to `ExecutionMode::Chain`
- The runtime plan still contains the expected child-step count

---

## Scenario 2: Chain Children Are Valid Self-Contained Steps

### Preconditions

- Orchestrator crate compiles

### Steps

1. Run the validation regression:
   ```bash
   cargo test -p agent-orchestrator validate_workflow_accepts_chain_steps_without_agent -- --nocapture
   ```

2. Run the build-plan regression:
   ```bash
   cargo test -p agent-orchestrator build_execution_plan_includes_chain_steps -- --nocapture
   ```

### Expected

- Both tests pass
- A parent chain step is accepted without its own direct agent requirement
- Child command steps are preserved in the runtime execution plan

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

- Orchestrator crate compiles

### Steps

1. Run the trace event compatibility regression:
   ```bash
   cargo test -p agent-orchestrator chain_and_dynamic_step_events_handled -- --nocapture
   ```

### Expected

- The test passes
- Trace builder still accepts `chain_step_started` / `chain_step_finished`
- Chain events remain visible in cycle step reconstruction
