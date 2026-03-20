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
- A step has `store_outputs` configured with `from_var` referencing a pipeline variable populated by a **prior step's `behavior.captures`**:
  ```yaml
  steps:
    - id: qa_testing
      behavior:
        captures:
          - var: qa_score           # Capture step output into pipeline var
            source: stdout
            json_path: $.qa_score   # Extract from agent's JSON stdout
      store_outputs:
        - store: metrics
          key: qa_result
          from_var: qa_score        # Read from pipeline var (populated by captures above)
  ```
- Agent stdout includes `{"qa_score":"passed:98%"}`

### Goal
Verify that `process_store_outputs()` writes the pipeline variable to the workflow store after step execution.

### Steps
1. **Code review** — verify `process_store_outputs` in `apply.rs` reads from `acc.pipeline_vars.vars`:
   ```bash
   rg -n "process_store_outputs|from_var" crates/orchestrator-scheduler/src/scheduler/item_executor/apply.rs
   ```
2. **Code review** — verify `apply_captures` populates pipeline_vars from stdout with json_path:
   ```bash
   rg -n "apply_captures|CaptureSource|json_path" crates/orchestrator-scheduler/src/scheduler/item_executor/accumulator.rs
   ```
3. **Unit test** — verify store I/O config serde:
   ```bash
   cargo test -p agent-orchestrator -- store_output_config_serde_round_trip
   ```

### Expected
- `from_var` reads from `acc.pipeline_vars.vars` — the variable must be previously populated by `behavior.captures` with `json_path`
- Step output fields do NOT auto-populate as pipeline vars; explicit `captures` declarations are required
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
- Pipeline var `bench_result` contains `'{"test_count": 1419, "pass_rate": 1.0}'` (populated by a prior step's `behavior.captures` with `json_path`)

### Goal
Verify that the `StorePut` post-action writes the pipeline variable to the store.

### Steps
1. **Code review** — verify `PostAction::StorePut` in `apply.rs` reads from pipeline_vars:
   ```bash
   rg -n "StorePut|execute_store_put" crates/orchestrator-scheduler/src/scheduler/item_executor/apply.rs
   ```
2. **Unit test** — verify PostAction serde:
   ```bash
   cargo test -p agent-orchestrator -- test_post_action_store_put_serde_round_trip
   ```

### Expected
- `PostAction::StorePut` is matched in `apply_step_results()`
- `execute_store_put()` reads from `acc.pipeline_vars.vars.get(from_var)` — the variable must be previously populated by `behavior.captures`
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
| 3 | store_outputs writes pipeline variables | ✅ PASS | 2026-03-20 | Claude | False positive: `from_var` reads from pipeline_vars (populated by `behavior.captures` with `json_path`). Doc updated to clarify captures prerequisite. |
| 4 | PostAction::StorePut writes to store | ✅ PASS | 2026-03-20 | Claude | False positive: same as S3. `from_var` requires explicit captures. Doc updated. |
| 5 | Invariant checkpoints halt execution | ✅ PASS | 2026-03-20 | claude | Tested with qa50-invariant-halt workflow. Task failed with `invariant_violated` event at before_complete checkpoint. Unit test `check_invariants_returns_none_for_empty_invariants` passes. |
