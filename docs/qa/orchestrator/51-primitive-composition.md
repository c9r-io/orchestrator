# Orchestrator - Primitive Composition (WP05)

**Module**: orchestrator
**Scope**: WP01-WP04 pairwise and triple composition — Store, Spawning, Dynamic Items/Selection, Invariants
**Scenarios**: 5
**Priority**: High

---

## Background

WP01-WP04 each implement a standalone workflow primitive. This document verifies that primitives **compose correctly** when combined in a single workflow execution:

- **WP01 Persistent Store**: `store_put` post-action, `store_inputs` / `store_outputs`
- **WP02 Task Spawning**: `spawn_task` post-action with parent linkage and depth tracking
- **WP03 Dynamic Items + Selection**: `generate_items` post-action, `item_select` builtin step
- **WP04 Invariant Constraints**: `safety.invariants` with `check_at` / `on_violation` at `before_complete`

### Isolation Strategy

Each scenario runs inside its own **project namespace** (`--project wp05-<id>`). This ensures:

- Resources (workspace, agent, workflow) are scoped to the project via `apply --project`
- Tasks are created with `--project`, setting `tasks.project_id`
- Store entries are keyed by `(store_name, project_id, key)` — no cross-scenario leaks
- No database reset required; scenarios are idempotent and repeatable

### Test Fixtures

All scenario manifests live under `fixtures/manifests/bundles/wp05-*.yaml`. Each bundle declares a self-contained set of Workspace + Agent + Workflow resources.

### Automated Test Script

```bash
scripts/qa/test-wp05-integration.sh [--layer N] [--scenario ID] [--verbose]
```

---

## Scenario 1: Store + Spawning (WP01 x WP02) — L1-A

### Preconditions
- Orchestrator binary built (`cargo build --release`)
- Database initialized (`orchestrator init`)
- Fixture: `fixtures/manifests/bundles/wp05-store-spawn.yaml`

### Goal
Verify that a parent task can write to a persistent store via `store_put` post-action AND spawn a child task via `spawn_task` post-action in the same step. The child must have correct `parent_task_id` and `spawn_depth`.

### Steps
1. Apply the fixture into project scope:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/wp05-store-spawn.yaml --project wp05-L1A
   ```
2. Create and start a task:
   ```bash
   TASK_ID=$(orchestrator task create \
     --project wp05-L1A \
     --workspace wp05-ws \
     -W wp05-store-spawn-parent \
     --target-file fixtures/wp05-qa/wp05-check.md \
     --goal "test store+spawn" \
     --no-start 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "$TASK_ID" >/dev/null 2>&1
   ```
3. Query task status:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT status FROM tasks WHERE id='${TASK_ID}';"
   ```
4. Verify store entry:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT COUNT(*) FROM workflow_store_entries
      WHERE store_name='context' AND project_id='wp05-L1A' AND key='parent_finding';"
   ```
5. Verify child task:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT id, parent_task_id, spawn_depth FROM tasks
      WHERE parent_task_id='${TASK_ID}';"
   ```

### Expected
- Step 3: Task status = `completed`
- Step 4: Count >= 1 (store entry exists with correct project_id)
- Step 5: Exactly 1 child task with `parent_task_id = $TASK_ID` and `spawn_depth >= 1`

### Expected Data State
```sql
-- Store is project-scoped
SELECT store_name, project_id, key FROM workflow_store_entries
WHERE project_id = 'wp05-L1A';
-- Result: context | wp05-L1A | parent_finding

-- Child task has lineage
SELECT parent_task_id, spawn_depth FROM tasks
WHERE parent_task_id = '<TASK_ID>';
-- Result: <TASK_ID> | 1
```

---

## Scenario 2: Store + Invariants — Halt & Pass (WP01 x WP04) — L1-B

### Preconditions
- Fixture: `fixtures/manifests/bundles/wp05-store-invariant.yaml`
- Contains TWO workflows:
  - `wp05-store-invariant-fail`: invariant command `exit 1`, expect_exit 0 → violation
  - `wp05-store-invariant-pass`: invariant command `exit 0`, expect_exit 0 → pass

### Goal
Verify that `before_complete` invariant violations halt the task (status = failed) while passing invariants allow normal completion. Tests the guard-step invariant integration fixed in `loop_engine.rs`.

### Steps — Violation Path
1. Apply fixture:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/wp05-store-invariant.yaml --project wp05-L1B
   ```
2. Create and start task with the failing workflow:
   ```bash
   TASK_ID=$(orchestrator task create \
     --project wp05-L1B \
     --workspace wp05-ws \
     -W wp05-store-invariant-fail \
     --target-file fixtures/wp05-qa/wp05-check.md \
     --goal "test invariant fail" \
     --no-start 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "$TASK_ID" >/dev/null 2>&1 || true
   ```
3. Verify task failed:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT status FROM tasks WHERE id='${TASK_ID}';"
   ```
4. Verify invariant halt event:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}'
      AND event_type='task_failed'
      AND json_extract(payload_json,'\$.reason')='invariant_halt_before_complete';"
   ```

### Steps — Pass Path
5. Create and start task with the passing workflow:
   ```bash
   TASK_ID2=$(orchestrator task create \
     --project wp05-L1B \
     --workspace wp05-ws \
     -W wp05-store-invariant-pass \
     --target-file fixtures/wp05-qa/wp05-check.md \
     --goal "test invariant pass" \
     --no-start 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "$TASK_ID2" >/dev/null 2>&1
   ```
6. Verify task completed:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT status FROM tasks WHERE id='${TASK_ID2}';"
   ```

### Expected
- Step 3: Task status = `failed`
- Step 4: Count >= 1 (invariant halt event with reason `invariant_halt_before_complete`)
- Step 6: Task status = `completed`

---

## Scenario 3: Dynamic Items + Selection (WP03) — L1-C

### Preconditions
- Fixture: `fixtures/manifests/bundles/wp05-items-select.yaml`
- Workflow generates 3 candidates via `generate_items` post-action, benchmarks each via item-scoped step, then selects winner via `item_select` builtin

### Goal
Verify the full WP03 pipeline: `generate_items` → item-scoped execution → `item_select` with `store_result`.

### Steps
1. Apply fixture:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/wp05-items-select.yaml --project wp05-L1C
   ```
2. Create and start task:
   ```bash
   TASK_ID=$(orchestrator task create \
     --project wp05-L1C \
     --workspace wp05-ws \
     -W wp05-items-select \
     --target-file fixtures/wp05-qa/wp05-check.md \
     --goal "test items+select" \
     --no-start 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "$TASK_ID" >/dev/null 2>&1
   ```
3. Verify dynamic items were generated:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT COUNT(*) FROM task_items WHERE task_id='${TASK_ID}' AND source='dynamic';"
   ```
4. Verify items_generated event:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}' AND event_type='items_generated';"
   ```
5. Verify item_select result persisted to store:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT COUNT(*) FROM workflow_store_entries
      WHERE store_name='evolution' AND project_id='wp05-L1C' AND key='winner_latest';"
   ```

### Expected
- Step 3: Count >= 3 (dynamic source) or total items >= 3
- Step 4: Count >= 1 (`items_generated` event exists)
- Step 5: Count >= 1 (winner stored with correct project_id)

---

## Scenario 4: Dynamic Items + Invariants (WP03 x WP04) — L1-D

### Preconditions
- Fixture: `fixtures/manifests/bundles/wp05-items-invariant.yaml`
- Workflow generates 2 candidates, runs item-scoped implement, then checks `before_complete` invariant (command `exit 0` → passes)

### Goal
Verify that invariants fire correctly after dynamically generated items complete their item-scoped steps. The `before_complete` checkpoint runs inside the guard-step early-return path.

### Steps
1. Apply fixture:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/wp05-items-invariant.yaml --project wp05-L1D
   ```
2. Create and start task:
   ```bash
   TASK_ID=$(orchestrator task create \
     --project wp05-L1D \
     --workspace wp05-ws \
     -W wp05-items-invariant \
     --target-file fixtures/wp05-qa/wp05-check.md \
     --goal "test items+invariant" \
     --no-start 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "$TASK_ID" >/dev/null 2>&1
   ```
3. Verify items generated:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT COUNT(*) FROM task_items WHERE task_id='${TASK_ID}';"
   ```
4. Verify invariant passed:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}' AND event_type='invariant_passed';"
   ```

### Expected
- Task status = `completed`
- Step 3: Count >= 2 (dynamically generated items)
- Step 4: Count >= 1 (`invariant_passed` event — before_complete checkpoint)

---

## Scenario 5: Store + Items + Selection + Spawn (WP01 x WP02 x WP03) — L2-A

### Preconditions
- Fixture: `fixtures/manifests/bundles/wp05-store-items-select.yaml`
- Workflow: generate 2 candidates → benchmark → item_select → store_put journal → spawn child
- Child workflow reads from parent's store via `store_inputs`

### Goal
Verify triple-primitive composition: dynamic items feed into selection, winner is stored, and a child task is spawned that inherits the project scope. This is the most complex composition test.

### Steps
1. Apply fixture:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/wp05-store-items-select.yaml --project wp05-L2A
   ```
2. Create and start task:
   ```bash
   TASK_ID=$(orchestrator task create \
     --project wp05-L2A \
     --workspace wp05-ws \
     -W wp05-store-items-select \
     --target-file fixtures/wp05-qa/wp05-check.md \
     --goal "test store+items+select+spawn" \
     --no-start 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "$TASK_ID" >/dev/null 2>&1
   ```
3. Verify items generated:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}' AND event_type='items_generated';"
   ```
4. Verify winner in store:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT COUNT(*) FROM workflow_store_entries
      WHERE store_name='evolution' AND project_id='wp05-L2A' AND key='winner_latest';"
   ```
5. Verify journal entry in store:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT COUNT(*) FROM workflow_store_entries
      WHERE store_name='journal' AND project_id='wp05-L2A' AND key='run_latest';"
   ```
6. Verify child task spawned with correct parent:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT parent_task_id FROM tasks WHERE parent_task_id='${TASK_ID}';"
   ```

### Expected
- Task status = `completed`
- Step 3: Count >= 1 (`items_generated` event)
- Step 4: Count >= 1 (winner stored under `wp05-L2A` project)
- Step 5: Count >= 1 (journal entry stored under `wp05-L2A` project)
- Step 6: Exactly 1 child task with `parent_task_id = $TASK_ID`

### Expected Data State
```sql
-- All store entries scoped to this project
SELECT store_name, project_id, key FROM workflow_store_entries
WHERE project_id = 'wp05-L2A' ORDER BY store_name, key;
-- Result:
-- evolution | wp05-L2A | winner_latest
-- journal   | wp05-L2A | run_latest

-- Child inherits project scope
SELECT project_id, parent_task_id, spawn_depth FROM tasks
WHERE parent_task_id = '<TASK_ID>';
-- Result: wp05-L2A | <TASK_ID> | 1
```

---

## Unit Test Coverage

| Test | File | Verified |
|------|------|----------|
| `test_post_action_store_put_serde_round_trip` | `config/step.rs` | PostAction::StorePut serialization |
| `test_post_action_spawn_task_serde_round_trip` | `config/step.rs` | PostAction::SpawnTask serialization |
| `test_generate_items_action_full` | `config/dynamic_items.rs` | GenerateItemsAction with mapping |
| `test_invariant_config_defaults` | `config/invariant.rs` | Default check_at includes before_complete |
| `build_segments_item_select_is_task_scoped` | `scheduler/loop_engine.rs` | item_select groups as Task scope |
| `promote_winner_vars_inserts_into_pipeline` | `scheduler/loop_engine.rs` | Winner var promotion |
| `check_invariants_returns_none_for_empty_invariants` | `scheduler/loop_engine.rs` | Empty invariants no-op |
| `test_extract_dynamic_items` | `scheduler/item_generate.rs` | Dynamic item extraction |
| `workflow_spec_to_config_converts_steps` | `resource/workflow/workflow_convert.rs` | Spec → Config with WP fields |

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Store + Spawning (L1-A) | ✅ | 2026-03-07 | claude | 5/5 assertions. store_put + spawn_task in same step, child lineage verified |
| 2 | Store + Invariants halt/pass (L1-B) | ✅ | 2026-03-07 | claude | 3/3 assertions. Guard-step invariant bypass fixed in loop_engine.rs:850-860 |
| 3 | Dynamic Items + Selection (L1-C) | ✅ | 2026-03-07 | claude | 3/3 assertions. generate_items → item_select → store_result pipeline |
| 4 | Dynamic Items + Invariants (L1-D) | ✅ | 2026-03-07 | claude | 3/3 assertions. before_complete invariant fires after dynamic item execution |
| 5 | Store + Items + Select + Spawn (L2-A) | ✅ | 2026-03-07 | claude | 5/5 assertions. Triple composition with child task inheriting project scope |
