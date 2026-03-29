---
self_referential_safe: true
---

# QA #81: Self-Evolution DB Schema Alignment (FR-030)

## Scope

Verify that the database schema supports the self-evolution workflow's dynamic item generation, workflow store persistence, event monitoring, and that the execution plan's SQL monitoring queries are valid.

## Scenarios

### S-01: `task_items` table has required columns for dynamic items

**Steps**:
1. Inspect migration m0008 in `core/src/persistence/migration_steps.rs` (lines 490-509)
2. Confirm `ALTER TABLE task_items ADD COLUMN dynamic_vars_json TEXT`
3. Confirm `ALTER TABLE task_items ADD COLUMN label TEXT`
4. Confirm `ALTER TABLE task_items ADD COLUMN source TEXT NOT NULL DEFAULT 'static'`

**Expected**:
- All three columns are present in the migration
- `source` defaults to `'static'` for backward compatibility

### S-02: `workflow_store_entries` table exists with correct schema

**Steps**:
1. Inspect migration m0007 in `core/src/persistence/migration_steps.rs` (lines 442-465)
2. Confirm PRIMARY KEY is `(store_name, project_id, key)`
3. Confirm columns: `store_name`, `project_id`, `key`, `value_json`, `task_id`, `created_at`, `updated_at`

**Expected**:
- Table creation and indexes are defined
- Column names match runtime code in `core/src/persistence/repository/workflow_store.rs`

### S-03: `events` table has required columns

**Steps**:
1. Inspect migration m0001 in `core/src/persistence/migration_steps.rs` (lines 94-101)
2. Confirm columns: `task_id`, `event_type`, `payload_json`, `created_at`

**Expected**:
- All columns exist in baseline migration

### S-04: `items_generated` event is emitted after dynamic item creation

**Steps**:
1. Inspect `crates/orchestrator-scheduler/src/scheduler/loop_engine/segment.rs` for the `items_generated` event emission
2. Confirm `insert_event()` is called with `event_type="items_generated"` after `create_dynamic_task_items_async()`

**Expected**:
- Event emitted with payload containing `count` and `replace` fields

### S-05: Monitoring query 1 — events query is valid

**Steps**:
1. Run `SELECT payload_json FROM events WHERE task_id='test' AND event_type='items_generated'` against an initialized database

**Expected**:
- Query executes without SQL error (empty result set is acceptable)

### S-06: Monitoring query 2 — task_items query is valid

**Steps**:
1. Run `SELECT id, label, source, status FROM task_items WHERE task_id='test'` against an initialized database

**Expected**:
- Query executes without SQL error (empty result set is acceptable)

### S-07: Monitoring query 3 — workflow_store_entries query is valid

**Steps**:
1. Run `SELECT value_json FROM workflow_store_entries WHERE store_name='evolution' AND key='winner_latest'` against an initialized database

**Expected**:
- Query executes without SQL error (empty result set is acceptable)

### S-08: Workspace regression gates remain green

**Steps**:
1. Code review confirms S1-S7 schema validation covers all tables used by workspace tests
2. Run `cargo test --workspace --lib` (safe: does not affect running daemon)
3. Verify zero test failures

**Expected**:
- All workspace lib tests pass
- If any schema column were missing, S5/S6/S7 SQL queries would have failed, confirming schema completeness

## Result

Verified on 2026-03-12:

- Schema audit confirmed all tables, columns, and migrations are in place
- All three monitoring queries are valid against current schema
- `cargo test --workspace`: passed

**Re-verified 2026-03-29:**

- S-01: `task_items` columns confirmed (dynamic_vars_json, label, source)
- S-02: `workflow_store_entries` table confirmed with correct PK and columns
- S-03: `events` table confirmed with required columns
- S-04: `items_generated` event emission confirmed in segment.rs
- S-05: Query valid against live DB (empty result set)
- S-06: Query valid against live DB (empty result set)
- S-07: Query valid against live DB (1 row returned from evolution store)
- S-08: `cargo test --workspace --lib`: 435 passed, 0 failed

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☑ | S-01–S-08: PASS (2026-03-29); S-08: 435 passed, 0 failed |
