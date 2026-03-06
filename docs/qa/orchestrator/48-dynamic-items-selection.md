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

## Database Schema Reference

### Table: task_items (M8 additions)

| Column | Type | Notes |
|--------|------|-------|
| dynamic_vars_json | TEXT | Per-item variables as JSON (nullable) |
| label | TEXT | Human-readable label (nullable) |
| source | TEXT NOT NULL DEFAULT 'static' | `"static"` or `"dynamic"` |

---

## Scenario 1: Generate Dynamic Items from Pipeline Variable

### Preconditions
- A workflow step has `post_actions: [{type: "generate_items", from_var: "candidates", json_path: "$.items", mapping: {item_id: "$.id", label: "$.name"}}]`
- Pipeline variable `candidates` contains a JSON object with an `items` array

### Goal
Verify that `GenerateItems` creates dynamic task items with correct metadata.

### Steps
1. Set pipeline variable:
   ```json
   {"items": [
     {"id": "approach_a", "name": "Approach A", "config": "/a.yaml"},
     {"id": "approach_b", "name": "Approach B", "config": "/b.yaml"}
   ]}
   ```
2. Configure GenerateItems with variable mapping:
   ```yaml
   post_actions:
     - type: generate_items
       from_var: candidates
       json_path: "$.items"
       mapping:
         item_id: "$.id"
         label: "$.name"
         vars:
           config_path: "$.config"
   ```
3. Execute the step and let the loop engine process the buffered action

### Expected
- 2 new rows in `task_items` with `source = 'dynamic'`
- `qa_file_path` matches the extracted `item_id` values (`approach_a`, `approach_b`)
- `label` is populated (`Approach A`, `Approach B`)
- `dynamic_vars_json` contains `{"config_path": "/a.yaml"}` and `{"config_path": "/b.yaml"}`
- Items have sequential `order_no` values following any existing items

### Expected Data State
```sql
SELECT qa_file_path, label, source, dynamic_vars_json
FROM task_items WHERE task_id = '{task_id}' AND source = 'dynamic'
ORDER BY order_no;
-- Expected: 2 rows
-- Row 1: qa_file_path='approach_a', label='Approach A', source='dynamic', dynamic_vars_json='{"config_path":"/a.yaml"}'
-- Row 2: qa_file_path='approach_b', label='Approach B', source='dynamic', dynamic_vars_json='{"config_path":"/b.yaml"}'
```

---

## Scenario 2: Generate Items with Replace Mode

### Preconditions
- Task already has 2 dynamic items from a previous generation
- A new GenerateItems action has `replace: true`

### Goal
Verify that `replace: true` removes existing dynamic items before creating new ones.

### Steps
1. Execute a GenerateItems action that creates 2 dynamic items (from Scenario 1)
2. Execute a second GenerateItems action with `replace: true` and a different candidate set:
   ```json
   {"items": [{"id": "candidate_x", "name": "Candidate X"}]}
   ```
3. Query task_items

### Expected
- Only 1 dynamic item exists (`candidate_x`)
- The 2 previous dynamic items (`approach_a`, `approach_b`) are deleted
- Static items (if any) are NOT affected by replace

### Expected Data State
```sql
SELECT COUNT(*) FROM task_items WHERE task_id = '{task_id}' AND source = 'dynamic';
-- Expected: 1

SELECT qa_file_path FROM task_items WHERE task_id = '{task_id}' AND source = 'dynamic';
-- Expected: 'candidate_x'
```

---

## Scenario 3: Item Selection with Min Strategy

### Preconditions
- 3 dynamic items have been evaluated with pipeline variable `error_count`
- item_select config: `strategy: min, metric_var: error_count`

### Goal
Verify that `item_select` with `min` strategy picks the item with the lowest metric value.

### Steps
1. Set up 3 items with evaluation results:
   - Item A: `error_count = 5`
   - Item B: `error_count = 2`
   - Item C: `error_count = 8`
2. Run `item_select` with config:
   ```yaml
   item_select:
     strategy: min
     metric_var: error_count
   ```
3. Check selection result

### Expected
- Winner: Item B (lowest `error_count = 2`)
- Eliminated: Item A, Item C
- Winner's pipeline vars are promoted to task-level variables
- `SelectionResult.winner_id = "b"`, `eliminated_ids = ["a", "c"]`

---

## Scenario 4: Item Selection with Weighted Strategy

### Preconditions
- 2 items evaluated with `quality` and `speed` pipeline variables
- item_select config: `strategy: weighted, weights: {quality: 0.7, speed: 0.3}`

### Goal
Verify weighted multi-metric scoring selects the item with highest composite score.

### Steps
1. Set up 2 items:
   - Item A: `quality = 8.0`, `speed = 2.0` → score: 8×0.7 + 2×0.3 = 6.2
   - Item B: `quality = 5.0`, `speed = 9.0` → score: 5×0.7 + 9×0.3 = 6.2
2. Run `item_select` with weighted config and `tie_break: last`
3. Check selection result

### Expected
- Scores are tied at 6.2
- With `tie_break: last`, Item B wins
- With `tie_break: first` (default), Item A would win

---

## Scenario 5: Item Selection with Threshold Strategy

### Preconditions
- 3 items evaluated with `quality_score` pipeline variable
- item_select config: `strategy: threshold, metric_var: quality_score, threshold: 5.0`

### Goal
Verify that threshold strategy filters items below the threshold and selects from the remaining.

### Steps
1. Set up 3 items:
   - Item A: `quality_score = 3.0` (below threshold)
   - Item B: `quality_score = 7.0` (above threshold)
   - Item C: `quality_score = 9.0` (above threshold)
2. Run `item_select` with threshold config and `tie_break: first`
3. Check selection result

### Expected
- Item A is eliminated (below threshold 5.0)
- Item B wins (first item that passes threshold, with `tie_break: first`)
- Item C is eliminated (passed threshold but lost tie-break)
- If **no** items pass the threshold, the selection fails with an error

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Generate dynamic items from pipeline variable | ☐ | | | |
| 2 | Generate items with replace mode | ☐ | | | |
| 3 | Item selection with min strategy | ☐ | | | |
| 4 | Item selection with weighted strategy | ☐ | | | |
| 5 | Item selection with threshold strategy | ☐ | | | |
