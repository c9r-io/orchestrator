# Orchestrator - Engine Wiring: Dynamic Items & Item Selection (WP03-WP04 Integration)

**Module**: orchestrator
**Scope**: pending_generate_items consumption in loop_engine, item_select orchestration after item-scoped segments
**Scenarios**: 2
**Priority**: High

---

## Background

Split from `50-engine-wiring-store-invariant-itemselect.md` to comply with the 5-scenario limit. These scenarios cover the engine-level wiring for dynamic item generation and item selection — two of the WP01-WP04 primitives that integrate at the loop_engine layer.

---

## Scenario 1: pending_generate_items Consumption Creates Dynamic Items

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

## Scenario 2: item_select Orchestration After Item-Scoped Segment

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
| `build_segments_item_select_is_task_scoped` | `scheduler/loop_engine.rs` | item_select groups as Task scope |
| `collect_item_eval_states_maps_pipeline_vars` | `scheduler/loop_engine.rs` | Pipeline var collection from item state |
| `promote_winner_vars_inserts_into_pipeline` | `scheduler/loop_engine.rs` | Winner var promotion |
| `test_extract_dynamic_items` | `scheduler/item_generate.rs` | Dynamic item extraction |
| `test_extract_dynamic_items_missing_var` | `scheduler/item_generate.rs` | Error on missing variable |

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | pending_generate_items consumption | ✅ | 2026-03-07 | claude | Code path verified: loop_engine.rs:435-477. take() → extract → create_async → refresh items/paths |
| 2 | item_select orchestration after item-scoped segment | ✅ | 2026-03-07 | claude | Code path verified: loop_engine.rs:598-655. has_item_select_step → execute → eliminate → promote → retain |
