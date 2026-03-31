---
self_referential_safe: true
---

# Orchestrator - Dynamic Items and Selection (WP03)

**Module**: orchestrator
**Scope**: GenerateItems post-action, dynamic task item creation, item_select builtin step, selection strategies
**Scenarios**: 5
**Priority**: High

---

## Background

WP03 adds two primitives for evolutionary workflows:

1. **GenerateItems post-action**: Extracts candidates from a JSON pipeline variable and creates dynamic task items at runtime. Items have `source='dynamic'`, optional `label`, and per-item `dynamic_vars_json`.

2. **item_select builtin step**: After all candidates are evaluated in parallel, selects a winner using configurable strategies (min, max, threshold, weighted). Eliminates losers and promotes the winner's pipeline vars to task level.

GenerateItems is **buffered** via `StepExecutionAccumulator.pending_generate_items` to prevent mutation during item iteration. The loop engine creates items after the segment completes.

---

## Scenario 1: Generate Dynamic Items from Pipeline Variable

### Goal
Verify that `GenerateItems` creates dynamic task items with correct metadata from JSON pipeline variable extraction.

### Steps

1. **Unit test** — verify dynamic item extraction from JSON:
   ```bash
   cargo test -p orchestrator-scheduler --lib test_extract_dynamic_items
   ```

2. **Unit test** — verify missing variable handling:
   ```bash
   cargo test -p orchestrator-scheduler --lib test_extract_dynamic_items_missing_var
   ```

3. **Unit test** — verify items with missing ID are skipped:
   ```bash
   cargo test -p orchestrator-scheduler --lib test_extract_dynamic_items_skips_missing_id
   ```

4. **Unit test** — verify config serde:
   ```bash
   cargo test -p orchestrator-config --lib test_generate_items_action_minimal
   cargo test -p orchestrator-config --lib test_generate_items_action_full
   ```

### Expected
- JSON extraction produces items with correct `qa_file_path`, `label`, `dynamic_vars_json`
- Missing variable returns error; missing item ID gracefully skips
- GenerateItemsAction config round-trips correctly

---

## Scenario 2: Generate Items with Replace Mode

### Goal
Verify pipeline variable content handling: truncation, stream-JSON extraction, and unquoted JSON repair.

### Steps

1. **Unit test** — verify pipeline variable content resolution:
   ```bash
   cargo test -p orchestrator-scheduler --lib test_resolve_pipeline_var_content_not_truncated
   cargo test -p orchestrator-scheduler --lib test_resolve_pipeline_var_content_truncated
   cargo test -p orchestrator-scheduler --lib test_resolve_pipeline_var_content_truncated_stream_json
   ```

2. **Unit test** — verify stream-JSON result extraction:
   ```bash
   cargo test -p orchestrator-scheduler --lib test_extract_stream_json_result
   cargo test -p orchestrator-scheduler --lib test_extract_stream_json_result_no_result
   cargo test -p orchestrator-scheduler --lib test_extract_stream_json_result_redacted
   ```

3. **Unit test** — verify unquoted JSON repair:
   ```bash
   cargo test -p orchestrator-scheduler --lib test_extract_dynamic_items_unquoted_json
   ```

### Expected
- Truncated content uses spill file fallback; stream-JSON extracts last result block
- Unquoted JSON is repaired before extraction
- Missing/redacted results handled gracefully

---

## Scenario 3: Item Selection with Min Strategy

### Goal
Verify that `item_select` with `min` strategy picks the item with the lowest metric value.

### Steps

1. **Unit test** — verify min selection:
   ```bash
   cargo test -p orchestrator-scheduler --lib test_select_min
   ```

2. **Unit test** — verify edge cases:
   ```bash
   cargo test -p orchestrator-scheduler --lib test_single_item
   cargo test -p orchestrator-scheduler --lib test_empty_items_fails
   ```

3. **Unit test** — verify config serde:
   ```bash
   cargo test -p orchestrator-config --lib test_item_select_config_min
   ```

### Expected
- Min strategy selects item with lowest metric value
- Single item returns that item; empty items fails with error

---

## Scenario 4: Item Selection with Weighted Strategy

### Goal
Verify weighted multi-metric scoring selects the item with highest composite score.

### Steps

1. **Unit test** — verify weighted selection:
   ```bash
   cargo test -p orchestrator-scheduler --lib test_select_weighted
   ```

2. **Unit test** — verify tie-break behavior:
   ```bash
   cargo test -p orchestrator-scheduler --lib test_tie_break_last
   ```

3. **Unit test** — verify config serde:
   ```bash
   cargo test -p orchestrator-config --lib test_item_select_config_weighted
   cargo test -p orchestrator-config --lib test_tie_break_default
   ```

### Expected
- Weighted score = sum(value × weight) for each metric
- Tie-break `last` selects the last item; `first` (default) selects the first

---

## Scenario 5: Item Selection with Threshold Strategy

### Goal
Verify that threshold strategy filters items below the threshold and selects from the remaining.

### Steps

1. **Unit test** — verify threshold selection:
   ```bash
   cargo test -p orchestrator-scheduler --lib test_select_threshold
   ```

2. **Unit test** — verify max strategy (complementary to min):
   ```bash
   cargo test -p orchestrator-scheduler --lib test_select_max
   ```

3. **Unit test** — verify loop engine integration for item_select:
   ```bash
   cargo test -p orchestrator-scheduler --lib build_segments_item_select_is_task_scoped
   cargo test -p orchestrator-scheduler --lib collect_item_eval_states_maps_pipeline_vars
   cargo test -p orchestrator-scheduler --lib promote_winner_vars_inserts_into_pipeline
   ```

### Expected
- Threshold filters items below the cutoff; first passing item wins with `tie_break: first`
- Max strategy selects item with highest metric
- Item selection is task-scoped; winner vars are promoted to task-level pipeline

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Generate dynamic items from pipeline variable | ✅ | 2026-03-31 | claude | All 5 unit tests pass: test_extract_dynamic_items, test_extract_dynamic_items_missing_var, test_extract_dynamic_items_skips_missing_id, test_generate_items_action_minimal, test_generate_items_action_full |
| 2 | Generate items with replace mode | ✅ | 2026-03-31 | claude | All 7 unit tests pass: pipeline var content (not truncated/truncated/truncated stream-json), stream-JSON result extraction (result/no result/redacted), unquoted JSON repair |
| 3 | Item selection with min strategy | ✅ | 2026-03-31 | claude | All 4 unit tests pass: test_select_min, test_single_item, test_empty_items_fails, test_item_select_config_min |
| 4 | Item selection with weighted strategy | ✅ | 2026-03-31 | claude | All 4 unit tests pass: test_select_weighted, test_tie_break_last, test_item_select_config_weighted, test_tie_break_default |
| 5 | Item selection with threshold strategy | ✅ | 2026-03-31 | claude | All 6 unit tests pass: test_select_threshold, test_select_max (+highest_score subcase), build_segments_item_select_is_task_scoped, collect_item_eval_states_maps_pipeline_vars, promote_winner_vars_inserts_into_pipeline |
