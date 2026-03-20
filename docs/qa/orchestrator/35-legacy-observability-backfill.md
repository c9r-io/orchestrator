---
self_referential_safe: true
---

# Orchestrator - Legacy Observability Backfill

**Module**: orchestrator
**Scope**: `step_scope` backfill for legacy events, `unknown` → `unspecified` display semantic, automatic backfill via migration
**Scenarios**: 5
**Priority**: High

---

## Background

Events created before the scope-aware observability feature lack `step_scope` in their `payload_json`. This causes `task trace --verbose` and `task watch` to display `scope=unknown`, making it hard to distinguish "old data" from "broken data". Phase 3 Task 04 adds:

- A controlled backfill that infers `step_scope` from `task_item_id` presence (item binding → `"item"`, no binding → `"task"`)
- Changed display label from `"unknown"` to `"unspecified"` for events that still lack scope after backfill
- Explanatory annotation in verbose trace output for unspecified-scoped events
- Automatic backfill on startup via database migration (m0002) and `backfill_legacy_data`

### Key Files

| File | Role |
|------|------|
| `core/src/events_backfill.rs` | `backfill_event_step_scope` function |
| `core/src/events.rs` | `observed_step_scope_label(None)` → `"unspecified"` |
| `core/src/scheduler/trace.rs` | `split_observed_item_binding` None → `"unspecified"`, verbose annotation |
| `core/src/scheduler/query.rs` | Watch frame `"~"` for unspecified scope |
| `crates/daemon/src/main.rs` | Startup backfill integration in the daemon bootstrap path |
| `core/src/service/system.rs` | daemon-exposed observability and maintenance entrypoints |

---

## Scenario 1: Backfill Infers Step Scope From Item Binding

### Preconditions

- Orchestrator crate compiles

### Goal

Verify that `backfill_event_step_scope` correctly infers `step_scope` based on `task_item_id` presence.

### Steps

1. Run the inference tests:
   ```bash
   cd core && cargo test --lib backfill_infers -- --nocapture
   ```

### Expected

- `backfill_infers_item_scope_when_task_item_id_present` passes: events with `task_item_id` get `step_scope: "item"`
- `backfill_infers_task_scope_when_task_item_id_absent` passes: events without `task_item_id` get `step_scope: "task"`
- After backfill, `query_step_events` returns the correct `ObservedStepScope` variant

---

## Scenario 2: Backfill Is Idempotent

### Preconditions

- Orchestrator crate compiles

### Goal

Verify that running backfill multiple times does not re-modify already-backfilled events.

### Steps

1. Run the idempotency test:
   ```bash
   cd core && cargo test --lib backfill_is_idempotent -- --nocapture
   ```

2. Run the skip test:
   ```bash
   cd core && cargo test --lib backfill_skips_events_already_having_step_scope -- --nocapture
   ```

### Expected

- First run: `updated > 0`
- Second run: `scanned == 0, updated == 0` (events already have `step_scope` in payload, filtered by `NOT LIKE '%step_scope%'`)
- Events that originally had `step_scope` in their payload are never touched

---

## Scenario 3: Display Semantic Changed From "unknown" to "unspecified"

### Preconditions

- Orchestrator crate compiles

### Goal

Verify that `observed_step_scope_label(None)` returns `"unspecified"` for events missing `step_scope`. The `"unspecified"` label is the intended design for events missing step_scope, distinguishing pre-scope-awareness data from errors.

### Steps

1. Run the label test:
   ```bash
   cd core && cargo test --lib observed_step_scope_label_returns_unspecified_for_none -- --nocapture
   ```

### Expected

- `observed_step_scope_label(None)` returns `"unspecified"` (not `"unknown"`)
- Test passes

---

## Scenario 4: Verbose Trace Explains Legacy Scope

### Preconditions

- Orchestrator crate compiles

### Goal

Verify that trace formatting correctly annotates legacy (pre-scope-awareness) events with an explanatory label.

### Steps

1. **Code review** — confirm `split_observed_item_binding` in `core/src/scheduler/trace.rs` returns `"unspecified"` for `None` scope:
   ```bash
   rg -n "unspecified" core/src/scheduler/trace.rs
   ```

2. **Code review** — confirm watch frame uses `"~"` for unspecified scope in `core/src/scheduler/query.rs`:
   ```bash
   rg -n "unspecified|~" core/src/scheduler/query.rs
   ```

3. Run related unit tests:
   ```bash
   cargo test --workspace --lib -- trace
   ```

### Expected

- `split_observed_item_binding` maps `None` scope to `"unspecified"` label
- Watch frame uses `"~"` shorthand for unspecified scope
- Trace-related unit tests pass

---

## Scenario 5: Automatic Backfill via Database Migration

### Preconditions

- Orchestrator crate compiles

### Goal

Verify that event backfill is handled automatically via database migration (m0002) on startup. There is no `config backfill-events` CLI command — backfill is automatic and requires no manual intervention.

### Steps

1. Run the backfill unit tests:
   ```bash
   cd core && cargo test --lib backfill_event_step_scope -- --nocapture
   ```

2. Run all backfill-related tests to confirm the automatic mechanism works:
   ```bash
   cd core && cargo test --lib backfill -- --nocapture
   ```

### Expected

- `backfill_event_step_scope` tests pass, confirming the function correctly infers and writes `step_scope`
- Backfill is triggered automatically on daemon startup via migration — no CLI entry point exists or is needed
- The backfill function scans events lacking `step_scope` and infers scope from `task_item_id` presence

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Backfill Infers Step Scope From Item Binding | ✅ | 2026-03-20 | Claude | `m0002_backfills_event_step_scope_from_task_item_id` passes |
| 2 | Backfill Is Idempotent | ✅ | 2026-03-20 | Claude | `m0002_skips_event_with_existing_step_scope` passes |
| 3 | Display Semantic Changed From "unknown" to "unspecified" | ✅ | 2026-03-20 | Claude | `observed_step_scope_label_returns_unspecified_for_none` passes |
| 4 | Verbose Trace Explains Legacy Scope | ✅ | 2026-03-20 | Claude | Code review: `builder.rs:35` → "unspecified", `watch.rs:304` → "~"; test `render_watch_frame_shows_unspecified_scope_marker_for_missing_scope_event` passes |
| 5 | Automatic Backfill via Database Migration | ✅ | 2026-03-20 | Claude | `backfill_is_noop_and_returns_zero_stats`, `backfill_promoted_populates_from_json`, `m0002_backfills_empty_agent_id`, `backfill_default_scope_data_updates_blank_task_and_command_run_fields` all pass |
