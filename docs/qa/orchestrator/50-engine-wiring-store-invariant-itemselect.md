# Orchestrator - Engine Wiring: Store I/O, Invariants, Item Select (WP01-WP04 Integration)

**Module**: orchestrator
**Scope**: store_inputs/store_outputs pipeline wiring, PostAction::StorePut, invariant checkpoints in loop_engine, item_select orchestration, pending_generate_items consumption
**Scenarios**: 5
**Priority**: High

---

## Background

WP01-WP04 implemented standalone primitives: persistent store, task spawning, dynamic items/selection, and invariant constraints. This document covers the **engine wiring** that connects these primitives into the execution loop (`loop_engine.rs`, `dispatch.rs`, `apply.rs`).

Key integration points:
- **store_inputs**: Before step execution, pipeline variables are injected from workflow stores via `resolve_store_inputs()` in `dispatch.rs`
- **store_outputs / PostAction::StorePut**: After step execution, pipeline variables are written back to stores via `process_store_outputs()` and `PostAction::StorePut` in `apply.rs`
- **Invariant checkpoints**: `check_invariants()` in `loop_engine.rs` evaluates pinned invariants at 4 points: `before_cycle`, `after_implement`, `before_restart`, `before_complete`
- **item_select orchestration**: After item-scoped segments, the loop engine runs selection logic to pick a winner and eliminate losers
- **pending_generate_items**: Task-scoped segment consumes buffered `GenerateItems` actions to create dynamic items mid-execution

---

## Scenario 1: store_inputs Injects Pipeline Variables Before Step Execution

### Preconditions
- A workflow step has `store_inputs` configured:
  ```yaml
  steps:
    - id: implement
      store_inputs:
        - store: metrics
          key: baseline_score
          as_var: prev_score
          required: false
  ```
- The `metrics` store contains key `baseline_score` with value `"0.95"`
- The step does not already have `prev_score` in its pipeline variables

### Goal
Verify that `resolve_store_inputs()` reads from the store and injects the value into pipeline variables before the step runs.

### Steps
1. Configure the step with the store_inputs declaration above
2. Ensure the store has the key via `store put metrics baseline_score '"0.95"'`
3. Run the task; the step should see `prev_score = "0.95"` in its pipeline vars

### Expected
- `StoreOp::Get` is called with `store_name="metrics"`, `key="baseline_score"`
- Pipeline var `prev_score` is set to the JSON string from the store
- The step receives the injected variable and can use it in its prompt/command
- If the key is missing and `required: false`, the var is simply not set (no error)

---

## Scenario 2: store_inputs Required Key Missing — Step Fails

### Preconditions
- A step has `store_inputs` with `required: true`:
  ```yaml
  store_inputs:
    - store: results
      key: mandatory_config
      as_var: config_json
      required: true
  ```
- The `results` store does NOT contain key `mandatory_config`

### Goal
Verify that a required missing key causes the step to fail before execution.

### Steps
1. Configure the step with `required: true`
2. Ensure the store key does not exist
3. Run the task

### Expected
- `resolve_store_inputs()` calls `anyhow::bail!` with message containing "required store input"
- The step does NOT execute (error occurs before dispatch)
- Task fails with a clear error message identifying the missing key and store

---

## Scenario 3: store_outputs Writes Pipeline Variables After Step Execution

### Preconditions
- A step has `store_outputs` configured:
  ```yaml
  steps:
    - id: qa_testing
      store_outputs:
        - store: metrics
          key: qa_result
          from_var: qa_score
  ```
- After the step runs, pipeline var `qa_score` contains `"passed:98%"`

### Goal
Verify that `process_store_outputs()` writes the pipeline variable to the workflow store after step execution.

### Steps
1. Configure store_outputs on the step
2. Run the step; agent produces `qa_score` in its output
3. Check the store for the written value

### Expected
- `StoreOp::Put` is called with `store_name="metrics"`, `key="qa_result"`, `value="passed:98%"`
- The store entry is created/updated
- If the `from_var` is missing from pipeline vars, a warning is logged but the step does NOT fail (non-critical)
- If the store write fails, a warning is logged but the step is still considered successful

---

## Scenario 4: PostAction::StorePut Writes to Store

### Preconditions
- A step has a post_action of type `store_put`:
  ```yaml
  post_actions:
    - type: store_put
      store: benchmarks
      key: latest_run
      from_var: bench_result
  ```
- Pipeline var `bench_result` contains `'{"test_count": 1419, "pass_rate": 1.0}'`

### Goal
Verify that the `StorePut` post-action writes the pipeline variable to the store.

### Steps
1. Configure the post_action on the step
2. Run the step; agent sets `bench_result`
3. Check the store value

### Expected
- `PostAction::StorePut` is matched in `apply_step_results()`
- `execute_store_put()` reads custom_resources from `active_config`
- `StoreOp::Put` is executed with the correct store/key/value
- If the write fails, a warning is logged (non-critical, does not fail the step)
- The post_action serializes as `{"type":"store_put","store":"...","key":"...","from_var":"..."}`

---

## Scenario 5: Invariant Checkpoints Halt Execution

### Preconditions
- A workflow has invariants configured with `on_violation: halt` at `before_cycle` and `after_implement`
- The invariant command fails (e.g., `cargo check` returns non-zero)

### Goal
Verify that `check_invariants()` in `loop_engine.rs` correctly halts execution at each checkpoint.

### Steps
1. Configure invariants with halting violation at multiple checkpoints
2. Run a task where the invariant command fails

### Expected

| Checkpoint | Location | On Halt |
|---|---|---|
| `before_cycle` | `execute_cycle_segments()` after checkpoint creation | `set_task_status("failed")` + `bail!` |
| `after_implement` | Task-scoped segment, after pipeline_vars persist | `set_task_status("failed")` + `bail!` |
| `before_restart` | `dispatch.rs` self_restart arm, before `process::exit(75)` | Abort restart, `EarlyReturn` |
| `before_complete` | `run_task_loop_core` after cycle loop exits | `set_task_status("failed")` + return |

- Events emitted: `invariant_violated` with invariant name and message
- `after_implement` detection: checks if segment step_ids contains "implement" or a step with `required_capability == Some("implement")`
- When `pinned_invariants` is empty, `check_invariants()` returns `Ok(None)` immediately (no-op)

---

## Unit Test Coverage

| Test | File | Verified |
|------|------|----------|
| `store_input_config_serde_round_trip` | `config/store_io.rs` | StoreInputConfig serialization |
| `store_input_config_required_defaults_false` | `config/store_io.rs` | Required field defaults |
| `store_output_config_serde_round_trip` | `config/store_io.rs` | StoreOutputConfig serialization |
| `test_post_action_store_put_serde_round_trip` | `config/step.rs` | PostAction::StorePut serde |
| `check_invariants_returns_none_for_empty_invariants` | `scheduler/loop_engine.rs` | Empty invariants early return |

> **Note**: Scenarios 6-7 (pending_generate_items, item_select orchestration) moved to `52-engine-wiring-dynamic-items-selection.md`.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | store_inputs injects pipeline variables | ✅ PASS | 2026-03-20 | claude | Code exists in dispatch.rs:1519. Tested with qa50-store-inputs-optional workflow. Optional key correctly injected. |
| 2 | store_inputs required key missing — step fails | ✅ PASS | 2026-03-20 | claude | Tested with qa50-store-inputs-required workflow. Task failed with 0 runs (step never dispatched). |
| 3 | store_outputs writes pipeline variables | ❌ FAIL | 2026-03-20 | claude | Feature gap: `from_var` cannot be populated from step output JSON. No capture mechanism exists. Ticket: docs/ticket/qa50_s3_store_outputs_fromvar_20260320_085730.md |
| 4 | PostAction::StorePut writes to store | ❌ FAIL | 2026-03-20 | claude | Same issue as S3 - `from_var` must exist in pipeline_vars but step output fields are not extracted. Ticket: docs/ticket/qa50_s3_store_outputs_fromvar_20260320_085730.md |
| 5 | Invariant checkpoints halt execution | ✅ PASS | 2026-03-20 | claude | Tested with qa50-invariant-halt workflow. Task failed with `invariant_violated` event at before_complete checkpoint. Unit test `check_invariants_returns_none_for_empty_invariants` passes. |
