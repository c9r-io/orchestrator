---
self_referential_safe: false
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
1. Inspect `core/src/scheduler/loop_engine/segment.rs` lines 176-183
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
1. Run `cargo test --workspace`

**Expected**:
- All workspace tests pass

## Result

Verified on 2026-03-12:

- Schema audit confirmed all tables, columns, and migrations are in place
- All three monitoring queries are valid against current schema
- `cargo test --workspace`: passed

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
