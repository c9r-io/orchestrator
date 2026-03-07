# Orchestrator - Engine Wiring: Store I/O, Invariants, Item Select (WP01-WP04 Integration)

**Module**: orchestrator
**Scope**: store_inputs/store_outputs pipeline wiring, PostAction::StorePut, invariant checkpoints in loop_engine, item_select orchestration, pending_generate_items consumption
**Scenarios**: 7
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

## Scenario 6: pending_generate_items Consumption Creates Dynamic Items

### Preconditions
- A step has `GenerateItems` post-action that buffers into `task_acc.pending_generate_items`
- The step is in a task-scoped segment
- The GenerateItems action specifies `json_path`, `id_field`, and field mappings

### Goal
Verify that after a task-scoped segment completes, `pending_generate_items` is consumed and new items are created.

### Steps
1. Configure a step with `GenerateItems` post-action:
   ```yaml
   post_actions:
     - type: generate_items
       json_path: "$.candidates"
       id_field: "name"
       qa_file_path_field: "file"
       replace: false
   ```
2. The step produces JSON output with candidates array
3. The task-scoped segment completes

### Expected
- `task_acc.pending_generate_items.take()` returns the buffered action
- `extract_dynamic_items()` parses the candidates from pipeline vars
- `create_dynamic_task_items_async()` inserts new items into the database
- Event emitted: `items_generated` with `count` and `replace` flag
- The `items` and `task_item_paths` vectors are refreshed for subsequent segments
- If `replace: true`, existing items are replaced; if `false`, items are appended

---

## Scenario 7: item_select Orchestration After Item-Scoped Segment

### Preconditions
- An execution plan has:
  - Item-scoped steps: `qa_testing`, `ticket_fix`
  - Task-scoped step: `evaluate` (builtin: `item_select`)
  - `ItemSelectConfig` with `strategy: min`, `metric_var: error_count`
- Two items exist, each with different `error_count` pipeline values after QA

### Goal
Verify that after the item-scoped segment completes, the loop engine runs item selection and eliminates losers.

### Steps
1. Configure execution plan with item_select as described
2. Run the task with two items
3. After item-scoped segment (qa_testing, ticket_fix), the engine detects the next segment has item_select
4. Selection runs

### Expected
- `has_item_select_step()` detects the item_select builtin in the next segment
- `find_item_select_config()` retrieves the ItemSelectConfig
- `collect_item_eval_states()` gathers pipeline vars from each item's accumulator
- `execute_item_select()` picks the winner (item with lowest error_count for `min` strategy)
- Winner's pipeline vars are promoted to task-level via `promote_winner_vars()`
- `item_select_winner` is set in task pipeline vars
- Eliminated items are updated via `update_task_item_status(id, "eliminated")`
- Event emitted: `item_selected` with `winner` and `eliminated` arrays
- `items.retain()` removes eliminated items from subsequent segments
- The `item_select` builtin dispatch returns `Handled` (no-op at step level)
- If `store_result` is configured, result is persisted via `persist_selection_to_store()`

---

## Unit Test Coverage

| Test | File | Verified |
|------|------|----------|
| `store_input_config_serde_round_trip` | `config/store_io.rs` | StoreInputConfig serialization |
| `store_input_config_required_defaults_false` | `config/store_io.rs` | Required field defaults |
| `store_output_config_serde_round_trip` | `config/store_io.rs` | StoreOutputConfig serialization |
| `test_post_action_store_put_serde_round_trip` | `config/step.rs` | PostAction::StorePut serde |
| `build_segments_item_select_is_task_scoped` | `scheduler/loop_engine.rs` | item_select groups as Task scope |
| `collect_item_eval_states_maps_pipeline_vars` | `scheduler/loop_engine.rs` | Pipeline var collection from item state |
| `promote_winner_vars_inserts_into_pipeline` | `scheduler/loop_engine.rs` | Winner var promotion |
| `check_invariants_returns_none_for_empty_invariants` | `scheduler/loop_engine.rs` | Empty invariants early return |
| `test_extract_dynamic_items` | `scheduler/item_generate.rs` | Dynamic item extraction |
| `test_extract_dynamic_items_missing_var` | `scheduler/item_generate.rs` | Error on missing variable |

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | store_inputs injects pipeline variables | ✅ | 2026-03-07 | claude | Code path verified: dispatch.rs:839-912. StoreOp::Get → pipeline_vars injection. Optional keys silently skipped |
| 2 | store_inputs required key missing — step fails | ✅ | 2026-03-07 | claude | Code path verified: dispatch.rs:877-882. bail! with store name + key in message |
| 3 | store_outputs writes pipeline variables | ✅ | 2026-03-07 | claude | Code path verified: apply.rs:315-342. Non-critical: warns on failure, doesn't propagate |
| 4 | PostAction::StorePut writes to store | ✅ | 2026-03-07 | claude | Code path verified: apply.rs:172-186, apply.rs:283-312. Tests: test_post_action_store_put_serde_round_trip |
| 5 | Invariant checkpoints halt execution | ✅ | 2026-03-07 | claude | Code path verified: loop_engine.rs:340 (before_cycle), 412 (after_implement), 158 (before_complete), dispatch.rs:350 (before_restart). Tests: check_invariants_returns_none_for_empty_invariants |
| 6 | pending_generate_items consumption | ✅ | 2026-03-07 | claude | Code path verified: loop_engine.rs:435-477. take() → extract → create_async → refresh items/paths |
| 7 | item_select orchestration after item-scoped segment | ✅ | 2026-03-07 | claude | Code path verified: loop_engine.rs:598-655. has_item_select_step → execute → eliminate → promote → retain. Tests: build_segments_item_select_is_task_scoped, collect_item_eval_states, promote_winner_vars |
