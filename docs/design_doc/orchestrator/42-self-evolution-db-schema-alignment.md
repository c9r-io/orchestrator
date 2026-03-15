# Design Doc #42: Self-Evolution DB Schema Alignment (FR-030)

## Status

Implemented

## Context

The self-evolution workflow requires three database tables to function correctly: `task_items` (for dynamic candidate items), `workflow_store_entries` (for persisting item selection results), and `events` (for monitoring lifecycle events). FR-030 was raised to audit whether the schema, runtime code, and monitoring queries from the self-evolution execution plan (`docs/showcases/self-evolution-execution.md`) are properly aligned.

## Decision

After a thorough audit, all required tables, columns, and event types were confirmed to already exist in the migration chain and runtime code. No schema changes were necessary.

## Design

### Table: `task_items`

Baseline created in **m0001** (`core/src/persistence/migration_steps.rs:49-65`). Three columns required by dynamic item generation were added in **m0008** (`workflow_primitives`):

| Column | Migration | Line | Purpose |
|--------|-----------|------|---------|
| `dynamic_vars_json` | m0008 | 492 | Per-item variable JSON from `generate_items` |
| `label` | m0008 | 498 | Human-readable candidate name |
| `source` | m0008 | 505 | Origin marker (`'static'` default, `'dynamic'` for generated items) |

Runtime writes: `core/src/scheduler/item_generate.rs:147-158` — `create_dynamic_task_items()` inserts rows with `source='dynamic'`.

### Table: `workflow_store_entries`

Created in **m0007** (`core/src/persistence/migration_steps.rs:442-465`):

```
PRIMARY KEY (store_name, project_id, key)
Columns: store_name, project_id, key, value_json, task_id, created_at, updated_at
```

Runtime writes: `core/src/scheduler/loop_engine/segment.rs:564-595` — `persist_selection_to_store()` upserts winner data after `item_select`.

Runtime reads: `core/src/persistence/repository/workflow_store.rs:65-89` — `get()` queries by `(store_name, project_id, key)`.

Note: The FR-030 document and execution plan use the term "namespace" in prose, but the actual column name is `store_name`. The monitoring query correctly uses `store_name`.

### Table: `events`

Baseline created in **m0001** (`core/src/persistence/migration_steps.rs:94-101`). Promoted columns added in **m0003** (step, step_scope, cycle).

Key columns: `task_id`, `event_type`, `payload_json`.

Runtime writes: `core/src/scheduler/loop_engine/segment.rs:176-183` — emits `items_generated` event after dynamic item creation with payload `{"count": N, "replace": bool}`.

### Data Flow

```
evo_plan (post_action: generate_items)
  → create_dynamic_task_items()     [INSERT task_items with source='dynamic']
  → insert_event(items_generated)   [INSERT events]
  → ...item-scoped steps execute...
  → execute_item_select()           [SELECT from task_items pipeline vars]
  → persist_selection_to_store()    [UPSERT workflow_store_entries]
  → insert_event(item_selected)     [INSERT events]
  → evo_apply_winner reads store    [SELECT workflow_store_entries]
```

### Monitoring Queries Validated

All three queries from `docs/showcases/self-evolution-execution.md` section 4.2 execute without SQL errors:

1. `SELECT payload_json FROM events WHERE task_id=? AND event_type='items_generated'`
2. `SELECT id, label, source, status FROM task_items WHERE task_id=?`
3. `SELECT value_json FROM workflow_store_entries WHERE store_name='evolution' AND key='winner_latest'`

## Acceptance Mapping

- `task_items` contains `label`, `source`, `dynamic_vars_json`: confirmed in m0008 migration
- `workflow_store_entries` exists with matching schema: confirmed in m0007 migration
- `events` table has `task_id`, `event_type`, `payload_json`: confirmed in m0001 migration
- Three monitoring SQL queries valid against schema: column names match
- `items_generated` event emitted in code: confirmed in segment.rs
- `cargo test --workspace` passes: verified during closure

## Verification

- `cargo test --workspace`
- Schema audit via code inspection of `core/src/persistence/migration_steps.rs`
