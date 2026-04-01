---
self_referential_safe: true
---

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
| `crates/orchestrator-config/src/config/step.rs` | `StepScope` enum, `default_scope_for_step_id()`, `resolved_scope()` |
| `crates/orchestrator-scheduler/src/scheduler/loop_engine/mod.rs` + `segment.rs` | `build_scope_segments()`, segment dispatch |
| `crates/orchestrator-scheduler/src/scheduler/item_executor/mod.rs` | `process_item_filtered()` unified loop with `StepExecutionAccumulator` |

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

- Rust toolchain available
- Repository checked out at project root

### Goal

Verify that task-scoped steps (plan, implement) execute exactly once per cycle while item-scoped steps (qa_testing) fan out per QA file — validated via unit tests covering `build_scope_segments()` and `default_scope_for_step_id()`.

### Steps

1. **Code review** — verify scope classification in config:
   ```bash
   rg -n "default_scope_for_step_id|StepScope::" crates/orchestrator-config/src/config/step.rs
   ```
   Confirm `plan` → `Task`, `implement` → `Task`, `qa_testing` → `Item`.

2. **Code review** — verify segment grouping dispatches single-run for Task scope:
   ```bash
   rg -n "StepScope::Task|StepScope::Item|process_task_segment|process_item_segment" crates/orchestrator-scheduler/src/scheduler/loop_engine/mod.rs
   ```

3. **Unit test** — run scope classification and segment grouping tests:
   ```bash
   cargo test --workspace --lib -- default_scope build_segments_groups_contiguous_scopes 2>&1 | tail -5
   ```

### Expected

- `default_scope_for_step_id("plan")` returns `Task`, `default_scope_for_step_id("qa_testing")` returns `Item`
- `build_segments_groups_contiguous_scopes` passes: 5 steps → 3 segments (Task[plan,implement] → Item[qa_testing,ticket_fix] → Task[doc_governance])
- Task-scoped segments dispatch single execution; item-scoped segments fan out per item

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
   sqlite3 ~/.orchestratord/agent_orchestrator.db \
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

- Rust toolchain available

### Goal

Verify that pipeline variables set during task-scoped segments propagate to item-scoped segments — validated via unit tests covering `promote_winner_vars` and `propagate_preserves_existing_item_state`.

### Steps

1. **Code review** — verify pipeline variable promotion in loop engine:
   ```bash
   rg -n "promote_winner_vars|propagate.*item_state|pipeline_vars" crates/orchestrator-scheduler/src/scheduler/loop_engine/ | head -20
   ```

2. **Code review** — verify template rendering resolves pipeline vars:
   ```bash
   rg -n "pipeline_vars_escaped_in_template|resolve_pipeline_var" core/src/ | head -10
   ```

3. **Unit test** — run pipeline variable propagation tests:
   ```bash
   cargo test --workspace --lib -- promote_winner_vars propagate_preserves pipeline_vars_escaped 2>&1 | tail -5
   ```

### Expected

- `promote_winner_vars_inserts_into_pipeline` passes: winner output vars merge into pipeline state
- `propagate_preserves_existing_item_state` passes: existing item state survives propagation
- `pipeline_vars_escaped_in_template` passes: vars are rendered (not literal placeholders) in command templates

---

## Scenario 4: Default Scope Classification Matches SDLC Intent

### Preconditions

- Rust toolchain available

### Goal

Verify that `default_scope_for_step_id()` correctly classifies all self-bootstrap steps without needing YAML `scope` fields.

### Steps

1. **Code review** — verify default scope mapping:
   ```bash
   rg -n "default_scope_for_step_id" crates/orchestrator-config/src/config/step.rs -A 30
   ```

2. **Unit test** — run default scope classification tests:
   ```bash
   cargo test --workspace --lib -- default_scope 2>&1 | tail -5
   ```

### Expected

- Unit tests pass confirming:
  - `plan` → Task, `qa_doc_gen` → Task, `implement` → Task, `self_test` → Task
  - `qa_testing` → Item, `ticket_fix` → Item
  - `align_tests` → Task, `doc_governance` → Task

### Expected Data State

```bash
cargo test --workspace --lib -- default_scope 2>&1 | grep "test result"
# Expected: test result: ok. N passed; 0 failed
```

---

## Scenario 5: Segment Grouping With Mixed Scope Steps

### Preconditions

- Rust toolchain available

### Goal

Verify that `build_scope_segments()` produces correct contiguous groupings and that guard steps are excluded.

### Steps

1. **Unit test** — run segment grouping and scope override tests:
   ```bash
   cargo test --workspace --lib -- build_segments resolved_scope 2>&1 | tail -5
   ```

### Expected

- `build_segments_groups_contiguous_scopes`: 5 steps → 3 segments (Task[plan,implement] → Item[qa_testing,ticket_fix] → Task[doc_governance])
- `build_segments_skips_guards`: loop_guard excluded, only plan segment remains
- `resolved_scope_uses_explicit_override`: `scope: Some(Task)` on a qa_testing step (item-scoped by default) overrides to Task scope

### Expected Data State

```bash
cargo test --workspace --lib -- build_segments resolved_scope 2>&1 | grep "test result"
# Expected: test result: ok. N passed; 0 failed
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Task-Scoped Steps Run Once With Multiple Items | ✅ PASS | 2026-04-01 | Claude | Code review: step.rs:286-426 scope mapping; build_segments_groups_contiguous_scopes passes |
| 2 | Item-Scoped Steps Fan Out Per QA File | ✅ PASS | 2026-04-01 | Claude | DB task 2efdb265: 65 distinct qa_testing executions with unique task_item_id/qa_file_path |
| 3 | Pipeline Variables Propagate From Task to Item Segments | ✅ PASS | 2026-04-01 | Claude | promote_winner_vars, propagate_preserves_existing_item_state pass; resolve_pipeline_var_content in item_generate.rs |
| 4 | Default Scope Classification Matches SDLC Intent | ✅ PASS | 2026-04-01 | Claude | test_default_scope_task_steps, test_default_scope_item_steps pass; step.rs:352-354 mapping verified |
| 5 | Segment Grouping With Mixed Scope Steps | ✅ PASS | 2026-04-01 | Claude | 5 build_segments tests pass; test_resolved_scope_explicit_override passes |
| * | Doc drift fix: corrected Key Files paths (`core/src/...` → `crates/...`), loop_engine.rs → loop_engine/mod.rs | — | 2026-03-19 | Claude | File paths in Key Files table + S1-S3 step commands updated to match actual layout |
