# Orchestrator - StepScope & Segment-Based Execution

**Module**: orchestrator
**Scope**: Validate that task-scoped steps run once per cycle and item-scoped steps fan out per QA file via segment-based execution
**Scenarios**: 5
**Priority**: High

---

## Background

The orchestrator now classifies workflow steps into two scopes:

- **Task-scoped** (`StepScope::Task`): plan, implement, self_test, qa_doc_gen, align_tests, doc_governance, build, test, lint, etc. — run once per cycle using the first item as context anchor.
- **Item-scoped** (`StepScope::Item`): qa, qa_testing, ticket_fix, ticket_scan, fix, retest — fan out per QA file.

The execution plan is grouped into **contiguous segments** of same scope. Each segment dispatches to either single-run (Task) or per-item (Item) execution via `process_item_filtered()`.

**Design doc**: `docs/design_doc/orchestrator/12-step-scope-segment-execution.md`

### Key Files

| File | Role |
|------|------|
| `core/src/config.rs` | `StepScope` enum, `default_scope_for_step_id()`, `resolved_scope()` |
| `core/src/scheduler/loop_engine.rs` | `build_scope_segments()`, segment dispatch |
| `core/src/scheduler/item_executor.rs` | `process_item_filtered()` unified loop with `StepExecutionAccumulator` |

---

## Database Schema Reference

### Table: events

| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER | Primary key |
| task_id | CHAR(36) | FK to tasks |
| task_item_id | CHAR(36) | FK to task_items (nullable) |
| event_type | TEXT | step_started, step_finished, step_skipped |
| payload_json | TEXT | JSON with step name, exit_code, etc. |
| created_at | TEXT | ISO timestamp |

### Table: task_items

| Column | Type | Notes |
|--------|------|-------|
| id | CHAR(36) | Primary key |
| task_id | CHAR(36) | FK to tasks |
| qa_file_path | TEXT | Relative path to QA doc |
| status | TEXT | pending, qa_passed, qa_failed, etc. |

---

## Scenario 1: Task-Scoped Steps Run Once With Multiple Items

### Preconditions

- A workflow with task-scoped steps (plan, implement) and multiple QA target files
- Apply echo-workflow fixture or self-bootstrap-test fixture

### Goal

Verify that plan and implement steps execute exactly once per cycle, regardless of item count.

### Steps

1. Apply a workflow fixture with plan + implement + qa_testing steps:
   ```bash
   rm -f fixtures/ticket/auto_*.md
   QA_PROJECT="qa-scope-${USER}-$(date +%Y%m%d%H%M%S)"
   orchestrator apply -f fixtures/manifests/bundles/self-bootstrap-test.yaml
   orchestrator project reset "${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   orchestrator apply -f fixtures/manifests/bundles/self-bootstrap-test.yaml --project "${QA_PROJECT}"
   ```

2. Create a task targeting multiple QA files:
   ```bash
   orchestrator task create \
     --name "scope-multi-item" \
     --goal "Test scope with multiple items" \
     --project "${QA_PROJECT}" \
     --workflow sdlc_full_pipeline \
     --target-files "docs/qa/file1.md,docs/qa/file2.md,docs/qa/file3.md" \
     --no-start
   ```

3. Start task and wait:
   ```bash
   orchestrator task start {task_id}
   ```

4. Count step executions:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT json_extract(payload_json, '$.step') AS step, COUNT(*) AS cnt
      FROM events
      WHERE task_id = '{task_id}' AND event_type = 'step_started'
      GROUP BY step ORDER BY MIN(created_at)"
   ```

### Expected

- `plan` count: **1** (not 3)
- `implement` count: **1** (not 3)
- `qa_testing` count: **3** (one per QA file)

### Expected Data State

```sql
SELECT json_extract(payload_json, '$.step') AS step, COUNT(*) AS cnt
FROM events WHERE task_id = '{task_id}' AND event_type = 'step_started'
GROUP BY step;
-- Expected: plan=1, implement=1, qa_testing=3
```

---

## Scenario 2: Item-Scoped Steps Fan Out Per QA File

### Preconditions

- Same fixture as Scenario 1 with 3 QA target files
- Task completed from Scenario 1 (or create a new one)

### Goal

Verify that item-scoped steps (qa_testing, ticket_fix) run once per QA file, each with correct `task_item_id`.

### Steps

1. Using the completed task from Scenario 1, query item-level events:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT e.task_item_id, ti.qa_file_path,
             json_extract(e.payload_json, '$.step') AS step
      FROM events e
      JOIN task_items ti ON e.task_item_id = ti.id
      WHERE e.task_id = '{task_id}'
        AND e.event_type = 'step_started'
        AND json_extract(e.payload_json, '$.step') = 'qa_testing'
      ORDER BY e.created_at"
   ```

### Expected

- 3 rows returned, each with a distinct `task_item_id` and `qa_file_path`
- Each QA file gets its own qa_testing execution
- Item statuses reflect individual QA outcomes

### Expected Data State

```sql
SELECT COUNT(DISTINCT e.task_item_id) AS item_count
FROM events e
WHERE e.task_id = '{task_id}'
  AND e.event_type = 'step_started'
  AND json_extract(e.payload_json, '$.step') = 'qa_testing';
-- Expected: 3
```

---

## Scenario 3: Pipeline Variables Propagate From Task to Item Segments

### Preconditions

- Workflow with plan (task-scoped) → qa_testing (item-scoped)
- Plan step produces `plan_output` pipeline variable

### Goal

Verify that pipeline variables set during task-scoped segments (e.g., `plan_output`) are available to item-scoped segments.

### Steps

1. Create and run a task with `sdlc_pipeline_vars` workflow (plan produces output, qa_testing references it):
   ```bash
   orchestrator task create \
     --name "scope-pipeline-vars" \
     --goal "Test pipeline var propagation across segments" \
     --project "${QA_PROJECT}" \
     --workflow sdlc_pipeline_vars \
     --no-start
   orchestrator task start {task_id}
   ```

2. Check that qa_testing received the plan_output:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT command FROM command_runs
      WHERE task_item_id IN (
        SELECT id FROM task_items WHERE task_id = '{task_id}'
      ) AND phase = 'qa_testing'
      ORDER BY started_at LIMIT 1"
   ```

### Expected

- The `command` for qa_testing contains the rendered plan_output value (not the literal `{plan_output}` placeholder)
- Plan step ran once, qa_testing ran for each item with the same propagated plan output

---

## Scenario 4: Default Scope Classification Matches SDLC Intent

### Preconditions

- Self-bootstrap workflow YAML loaded (no explicit `scope` annotations)

### Goal

Verify that `default_scope_for_step_id()` correctly classifies all self-bootstrap steps without needing YAML `scope` fields.

### Steps

1. Apply self-bootstrap manifest:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/self-bootstrap-mock.yaml --dry-run
   ```

2. Run unit test to verify default_scope mapping:
   ```bash
   cd core && cargo test default_scope -- --nocapture
   ```

### Expected

- Unit tests pass confirming:
  - `plan` → Task, `qa_doc_gen` → Task, `implement` → Task, `self_test` → Task
  - `qa_testing` → Item, `ticket_fix` → Item
  - `align_tests` → Task, `doc_governance` → Task
- Self-bootstrap YAML applies without errors (no unknown `scope` field complaints)

### Expected Data State

```bash
cd core && cargo test default_scope 2>&1 | grep "test result"
# Expected: test result: ok. 2 passed; 0 failed
```

---

## Scenario 5: Segment Grouping With Mixed Scope Steps

### Preconditions

- Execution plan with interleaved scopes: [Task, Task, Item, Item, Task]

### Goal

Verify that `build_scope_segments()` produces correct contiguous groupings and that guard steps are excluded.

### Steps

1. Run the segment grouping unit tests:
   ```bash
   cd core && cargo test build_segments -- --nocapture
   ```

2. Run the scope override test:
   ```bash
   cd core && cargo test resolved_scope -- --nocapture
   ```

### Expected

- `build_segments_groups_contiguous_scopes`: 5 steps → 3 segments (Task[plan,implement] → Item[qa_testing,ticket_fix] → Task[doc_governance])
- `build_segments_skips_guards`: loop_guard excluded, only plan segment remains
- `resolved_scope_uses_explicit_override`: `scope: Some(Task)` on a qa_testing step (item-scoped by default) overrides to Task scope

### Expected Data State

```bash
cd core && cargo test build_segments resolved_scope 2>&1 | grep "test result"
# Expected: test result: ok. 3 passed; 0 failed
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Task-Scoped Steps Run Once With Multiple Items | ☐ | | | |
| 2 | Item-Scoped Steps Fan Out Per QA File | ☐ | | | |
| 3 | Pipeline Variables Propagate From Task to Item Segments | ☐ | | | |
| 4 | Default Scope Classification Matches SDLC Intent | ☐ | | | |
| 5 | Segment Grouping With Mixed Scope Steps | ☐ | | | |
