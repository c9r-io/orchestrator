# Orchestrator - generate_items Extraction from Non-Pure-JSON Agent Output

**Module**: orchestrator
**Scope**: GenerateItems post-action robustness when agent output contains mixed text (natural language + JSON), malformed JSON, or fenced code blocks
**Scenarios**: 5
**Priority**: High

---

## Background

`generate_items` post-action extracts dynamic items from a pipeline variable via `extract_json_array`. The pipeline calls `resolve_pipeline_var_content` → `extract_json_array(json_str, json_path)`. The first line of `extract_json_array` is `serde_json::from_str(json_str)`, which requires the entire string to be valid JSON.

In practice, LLM agents produce mixed-text output: natural language explanation followed by a JSON block. When the agent's result text is not pure JSON, `serde_json::from_str` fails, `generate_items` silently falls back, and all 100 static items remain — defeating the narrowing purpose entirely.

**Root cause ticket**: `docs/ticket/20260313-generate-items-json-extraction-fails-on-mixed-text.md`

### Key Code Paths

| File | Function | Role |
|------|----------|------|
| `core/src/json_extract.rs:8-15` | `extract_json_array` | Parses JSON string, resolves path to array |
| `core/src/scheduler/item_generate.rs:16-57` | `resolve_pipeline_var_content` | Resolves pipeline var (inline / spill / stream-json) |
| `core/src/scheduler/item_generate.rs:60-108` | `extract_dynamic_items` | Calls `extract_json_array`, maps to `NewDynamicItem` |
| `core/src/scheduler/loop_engine/segment.rs:155-208` | segment transition | Consumes `pending_generate_items`, calls `extract_dynamic_items` |

### Test Fixture

All scenarios use the `wp05-items-select.yaml` fixture pattern with a mock agent whose `command` is a `printf` or `echo` that produces controlled output. The workflow must have a `generate_items` post_action on a task-scoped step.

---

## Scenario 1: Mixed Text — Natural Language Preamble + JSON Object

### Preconditions
- Mock agent command outputs mixed text: explanation text followed by a JSON object containing the target array
- Workflow step has `generate_items` post_action with `from_var` pointing to step output, `json_path: "$.regression_targets"`

### Goal
Verify that `extract_json_array` can extract JSON from text that starts with natural language.

### Steps
1. Create a mock workflow where the agent command outputs:
   ```
   printf '%s' 'Based on my analysis, I identified these targets:

   {"regression_targets": [{"id": "target-a", "name": "Target A"}, {"id": "target-b", "name": "Target B"}]}'
   ```
2. Configure `generate_items` post_action:
   ```yaml
   post_actions:
     - type: generate_items
       from_var: plan_output
       json_path: "$.regression_targets"
       mapping:
         item_id: "$.id"
         label: "$.name"
       replace: true
   ```
3. Run the task and check whether dynamic items are created

### Expected (after fix)
- `extract_json_array` scans for the first `{` in the text, attempts parse from there
- 2 dynamic items created: `target-a`, `target-b`
- `items_generated` event emitted with `count: 2, replace: true`
- Subsequent item-scoped steps process only these 2 items

### Current Behavior (before fix)
- `serde_json::from_str` fails on the full mixed text → `Err("invalid JSON")`
- `generate_items` falls back silently; static items remain

### Verification
```bash
# After task completes (or segment transition occurs):
sqlite3 data/agent_orchestrator.db "
  SELECT event_type, payload_json FROM events
  WHERE task_id = '<task_id>' AND event_type = 'items_generated';
"
# Expected after fix: 1 row with count=2
# Current: 0 rows

sqlite3 data/agent_orchestrator.db "
  SELECT COUNT(*) FROM task_items
  WHERE task_id = '<task_id>' AND source = 'dynamic';
"
# Expected after fix: 2
# Current: 0
```

---

## Scenario 2: Fenced Code Block — JSON Inside Markdown Triple Backticks

### Preconditions
- Agent output wraps JSON in a markdown fenced code block (` ```json ... ``` `)
- This is the most common LLM output pattern

### Goal
Verify extraction works when JSON is inside a fenced code block.

### Steps
1. Mock agent outputs:
   ````
   printf '%s' 'Here are the regression targets:

   ```json
   {
     "regression_targets": [
       {"id": "doc-a", "name": "Doc A"},
       {"id": "doc-b", "name": "Doc B"},
       {"id": "doc-c", "name": "Doc C"}
     ]
   }
   ```'
   ````
2. Same `generate_items` config as Scenario 1
3. Run the task

### Expected (after fix)
- Extraction strips fencing and parses inner JSON
- 3 dynamic items created

### Current Behavior (before fix)
- `serde_json::from_str` fails → silent fallback

---

## Scenario 3: Pure JSON — Baseline (No Regression)

### Preconditions
- Agent output is pure valid JSON with no surrounding text

### Goal
Verify that existing pure-JSON extraction still works correctly (regression guard).

### Steps
1. Mock agent outputs:
   ```
   printf '%s' '{"regression_targets": [{"id": "clean-a", "name": "Clean A"}]}'
   ```
2. Same `generate_items` config
3. Run the task

### Expected
- 1 dynamic item created: `clean-a`
- This must continue to work identically before and after any fix

### Verification
```bash
sqlite3 data/agent_orchestrator.db "
  SELECT qa_file_path, label, source FROM task_items
  WHERE task_id = '<task_id>' AND source = 'dynamic';
"
# Expected: clean-a | Clean A | dynamic
```

---

## Scenario 4: Malformed JSON — Graceful Error

### Preconditions
- Agent output contains text that looks like JSON but is syntactically invalid

### Goal
Verify graceful error handling when no valid JSON can be extracted.

### Steps
1. Mock agent outputs:
   ```
   printf '%s' 'I found these targets: {regression_targets: [target-a, target-b]}'
   ```
   (Missing quotes around keys and string values — not valid JSON)
2. Same `generate_items` config
3. Run the task

### Expected
- Extraction fails with a clear error
- WARN log emitted with content preview
- Static items remain (fallback behavior)
- No `items_generated` event
- Task continues execution (non-fatal)

### Verification
```bash
# Check daemon log for warning
grep "extract_json_array failed" /tmp/orchestratord.log
# Expected: 1 line with "invalid JSON" and content preview

sqlite3 data/agent_orchestrator.db "
  SELECT COUNT(*) FROM task_items
  WHERE task_id = '<task_id>' AND source = 'dynamic';
"
# Expected: 0
```

---

## Scenario 5: Multiple JSON Objects in Text — First Valid Object Wins

### Preconditions
- Agent output contains multiple JSON-like blocks, only one of which has the target path

### Goal
Verify extraction selects the correct JSON block containing the target path.

### Steps
1. Mock agent outputs:
   ```
   printf '%s' 'Analysis summary: {"status": "complete", "count": 6}

   Detailed results:
   {"regression_targets": [{"id": "rt-1", "name": "RT 1"}, {"id": "rt-2", "name": "RT 2"}]}

   Metadata: {"timestamp": "2026-03-13"}'
   ```
2. `generate_items` with `json_path: "$.regression_targets"`
3. Run the task

### Expected (after fix)
- Extraction finds the second JSON object (the one containing `regression_targets`)
- 2 dynamic items created: `rt-1`, `rt-2`

### Current Behavior (before fix)
- `serde_json::from_str` on full text fails → silent fallback

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Mixed text — natural language + JSON | | | | Blocked on fix: `extract_json_array` requires pure JSON |
| 2 | Fenced code block (` ```json ``` `) | | | | Blocked on fix |
| 3 | Pure JSON baseline (regression guard) | ✅ | 2026-03-13 | claude | Covered by existing unit test `test_extract_dynamic_items` in `item_generate.rs` |
| 4 | Malformed JSON — graceful error | ✅ | 2026-03-13 | claude | Current behavior is correct: WARN log + fallback. Unit test `extract_array_not_array_fails` covers parse error path |
| 5 | Multiple JSON objects in text | | | | Blocked on fix |
