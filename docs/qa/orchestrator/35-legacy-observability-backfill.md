# Orchestrator - Legacy Observability Backfill

**Module**: orchestrator
**Scope**: `step_scope` backfill for legacy events, `unknown` → `legacy` display semantic, `config backfill-events` CLI
**Scenarios**: 5
**Priority**: High

---

## Background

Events created before the scope-aware observability feature lack `step_scope` in their `payload_json`. This causes `task trace --verbose` and `task watch` to display `scope=unknown`, making it hard to distinguish "old data" from "broken data". Phase 3 Task 04 adds:

- A controlled backfill that infers `step_scope` from `task_item_id` presence (item binding → `"item"`, no binding → `"task"`)
- Changed display label from `"unknown"` to `"legacy"` for events that still lack scope after backfill
- Explanatory annotation in verbose trace output for legacy-scoped events
- A `config backfill-events` CLI for manual backfill with statistics
- Automatic backfill on startup via `backfill_legacy_data`

### Key Files

| File | Role |
|------|------|
| `core/src/events_backfill.rs` | `backfill_event_step_scope` function |
| `core/src/events.rs` | `observed_step_scope_label(None)` → `"legacy"` |
| `core/src/scheduler/trace.rs` | `split_observed_item_binding` None → `"legacy"`, verbose annotation |
| `core/src/scheduler/query.rs` | Watch frame `"~"` for legacy scope |
| `core/src/main.rs` | Startup backfill integration in `backfill_legacy_data` |
| `core/src/cli.rs` | `ConfigLifecycleCommands::BackfillEvents` |
| `core/src/cli_handler/config_lifecycle.rs` | Backfill CLI handler |

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

## Scenario 3: Display Semantic Changed From "unknown" to "legacy"

### Preconditions

- Orchestrator crate compiles

### Goal

Verify that all display paths show `"legacy"` instead of `"unknown"` for events missing `step_scope`.

### Steps

1. Run the label test:
   ```bash
   cd core && cargo test --lib observed_step_scope_label_returns_legacy -- --nocapture
   ```

2. Run the trace build test:
   ```bash
   cd core && cargo test --lib build_trace_marks_legacy_step_scope_as_legacy -- --nocapture
   ```

3. Run the watch frame test:
   ```bash
   cd core && cargo test --lib render_watch_frame_shows_legacy_scope -- --nocapture
   ```

### Expected

- `observed_step_scope_label(None)` returns `"legacy"` (not `"unknown"`)
- Trace steps with no `step_scope` in payload have `scope == "legacy"` in `StepTrace`
- Watch frame displays `~` (tilde) for legacy scope instead of `?`
- All 3 tests pass

---

## Scenario 4: Verbose Trace Explains Legacy Scope

### Preconditions

- A task with at least one legacy event (no `step_scope` in payload)
- Or use the unit test directly

### Goal

Verify that `task trace --verbose` appends an explanatory annotation when scope is `"legacy"`.

### Steps

1. If a legacy task exists:
   ```bash
   ./scripts/orchestrator.sh task trace {task_id} --verbose
   ```

2. If no legacy task exists, verify via unit test:
   ```bash
   cd core && cargo test --lib build_trace_marks_legacy -- --nocapture
   ```

### Expected

- Verbose output for legacy steps shows: `scope=legacy (pre-scope event, step_scope not recorded)`
- Non-legacy steps show only: `scope=task` or `scope=item` without the parenthetical annotation
- The annotation helps users understand that "legacy" means pre-scope-awareness data, not an error

---

## Scenario 5: Config Backfill-Events CLI

### Preconditions

- Orchestrator binary built and available

### Goal

Verify `config backfill-events` provides a manual entry point for event backfill with statistics.

### Steps

1. Run backfill without `--force` (safety gate):
   ```bash
   ./scripts/orchestrator.sh config backfill-events 2>&1; echo "exit=$?"
   ```

2. Run backfill with `--force`:
   ```bash
   ./scripts/orchestrator.sh config backfill-events --force
   ```

3. Run again to confirm idempotency:
   ```bash
   ./scripts/orchestrator.sh config backfill-events --force
   ```

### Expected

- Without `--force`: prints warning to stderr and exits with code 1; no database changes occur.
- First `--force` run outputs: `scanned N events, updated M, skipped K (already had step_scope)` where M >= 0
- Second `--force` run outputs: `scanned 0 events, updated 0, skipped 0 (already had step_scope)` (all events already backfilled)
- Exit code 0 on both `--force` runs
- Non-step events (cycle_started, task_completed, etc.) are never counted in scanned/updated

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Backfill Infers Step Scope From Item Binding | ☐ | | | |
| 2 | Backfill Is Idempotent | ☐ | | | |
| 3 | Display Semantic Changed From "unknown" to "legacy" | ☐ | | | |
| 4 | Verbose Trace Explains Legacy Scope | ☐ | | | |
| 5 | Config Backfill-Events CLI | ☐ | | | |
