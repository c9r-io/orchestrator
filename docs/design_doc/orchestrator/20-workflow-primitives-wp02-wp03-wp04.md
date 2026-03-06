# Orchestrator - Workflow Primitives (WP02 / WP03 / WP04)

**Module**: orchestrator
**Status**: Approved
**Related Plan**: Unified abstraction design for task spawning, dynamic items + selection, and invariant constraints
**Related QA**: `docs/qa/orchestrator/47-task-spawning.md`, `docs/qa/orchestrator/48-dynamic-items-selection.md`, `docs/qa/orchestrator/49-invariant-constraints.md`
**Created**: 2026-03-07
**Last Updated**: 2026-03-07

## Background

The orchestrator engine needs three new primitives to support autonomous, evolutionary workflows:

- **WP02 Task Spawning**: Steps create new tasks from output (goal discovery, work decomposition). A parent task can spawn children via `PostAction::SpawnTask` (single) or `PostAction::SpawnTasks` (batch from JSON).
- **WP03 Dynamic Items + Selection**: Runtime candidate generation with parallel evaluation and tournament selection. `PostAction::GenerateItems` injects items mid-execution; `item_select` builtin step picks a winner.
- **WP04 Invariant Constraints**: Tamper-proof safety assertions enforced by the engine at cycle checkpoints. Configured via `SafetyConfig.invariants`, pinned immutably at task start in `TaskRuntimeContext`.

WP01 (Persistent Store) data layer was already implemented; these primitives build on the same infrastructure patterns.

## Goals

- Enable workflows to autonomously discover and decompose work into child tasks
- Support evolutionary candidate generation with competitive selection
- Enforce immutable safety invariants at engine level (not agent level)
- Maintain full backward compatibility with existing workflows

## Non-goals

- Full CEL expression engine (simple `exit_code == N` assertions only in initial version)
- CLI commands for managing invariants at runtime
- Cross-task dependency graphs or task DAG scheduling
- Real-time monitoring UI for spawn trees

## Scope

- In scope: Config types, scheduler modules, M8 migration, PostAction integration, accumulator buffering, spawn depth safety
- Out of scope: Loop engine checkpoint wiring (deferred to integration phase), CLI `task list --parent` / `task tree` commands (future)

## Database Changes

### Migration M8: `m0008_workflow_primitives`

**Table `tasks`** — WP02 lineage:

| Column | Type | Notes |
|--------|------|-------|
| parent_task_id | TEXT | FK to parent task (nullable) |
| spawn_reason | TEXT | `"spawn_task"` or `"spawn_tasks"` |
| spawn_depth | INTEGER NOT NULL DEFAULT 0 | Depth in spawn tree |

Index: `idx_tasks_parent_id ON tasks(parent_task_id)`

**Table `task_items`** — WP03 dynamic metadata:

| Column | Type | Notes |
|--------|------|-------|
| dynamic_vars_json | TEXT | Per-item variables as JSON (nullable) |
| label | TEXT | Human-readable label (nullable) |
| source | TEXT NOT NULL DEFAULT 'static' | `"static"` or `"dynamic"` |

All columns added via `ensure_column` for idempotency.

## Key Design

1. **PostAction tagged union extension**: Three new variants (`SpawnTask`, `SpawnTasks`, `GenerateItems`) added to the existing `PostAction` enum. All use `#[serde(default)]` for backward compatibility.

2. **Spawn depth safety**: `SafetyConfig` gains `max_spawn_depth: Option<usize>` and `max_spawned_tasks: Option<usize>`. Depth is validated before each spawn via `validate_spawn_depth()`. Child tasks inherit `parent_spawn_depth + 1`.

3. **GenerateItems buffering**: The `GenerateItems` post-action does not create items immediately. It stores the action in `StepExecutionAccumulator.pending_generate_items`. The loop engine creates items after the segment completes, preventing mutation during item iteration.

4. **Item selection is cross-item**: Unlike normal steps that see one item, `item_select` needs all item evaluation states. The selection logic runs in `scheduler::item_select` with four strategies: `min`, `max`, `threshold`, `weighted`.

5. **Invariant immutability**: `TaskRuntimeContext.pinned_invariants` uses `Arc<Vec<InvariantConfig>>` so invariants are set once at task start and cannot be mutated by agents.

6. **Protected file detection**: Invariants can specify `protected_files` globs. The engine runs `git diff --name-only HEAD` and matches against patterns using simple prefix/suffix glob matching.

## Alternatives And Tradeoffs

- **Full CEL engine vs simple assertions**: Chose simple `exit_code == N` / `exit_code != N` for now. Full CEL adds a dependency and complexity; can be added later.
- **Immediate vs buffered item creation**: Buffered via accumulator prevents race conditions during item-scoped fan-out. Slightly more complex but safer.
- **Spawn as DB-only vs queue-based**: Chose DB-only (insert task row, set spawn_depth). Queue-based execution is orthogonal and uses existing task scheduling.

## Risks And Mitigations

- **Spawn bomb**: A workflow could spawn unlimited children. Mitigated by `max_spawn_depth` and `max_spawned_tasks` in SafetyConfig.
- **Invariant command injection**: Invariant commands run as shell commands. Mitigated by the fact that invariants are configured by workflow authors (trusted), not agents.
- **Dynamic item explosion**: `GenerateItems` could inject thousands of items. Future mitigation: add `max_items` cap in `GenerateItemsAction`.

## Observability

- **Events**: `task_spawned`, `tasks_spawned`, `step_finished` (with spawn/select metadata), `invariant_checked` (planned)
- **Logs**: `tracing::info` for spawn operations, `tracing::warn` for spawn depth violations and skipped items
- **Metrics**: spawn_depth tracked per task in DB; item source (`static`/`dynamic`) queryable

## Operations / Release

- Config: No new env vars. All configuration via workflow YAML `safety:` section
- Migration: M8 runs automatically on startup (idempotent `ensure_column`)
- Compatibility: Fully backward-compatible. All new fields use `#[serde(default)]`. Existing workflows unchanged.
- Rollback: New columns are nullable/defaulted; old binary ignores them

## Test Plan

- Unit tests: Config serde round-trips (spawn, invariant, item_select, dynamic_items), json_extract extraction, spawn depth validation, template resolution, selection strategies (min/max/threshold/weighted), tie-breaking, invariant evaluation, protected file matching
- Integration tests: M8 migration idempotency, PostAction match arms in apply.rs
- E2E (planned): Spawn tree creation, invariant violation halting, dynamic item generation + selection

## QA Docs

- `docs/qa/orchestrator/47-task-spawning.md`
- `docs/qa/orchestrator/48-dynamic-items-selection.md`
- `docs/qa/orchestrator/49-invariant-constraints.md`

## Acceptance Criteria

- Workflows can define `spawn_task` / `spawn_tasks` post-actions that create child tasks with lineage tracking
- Workflows can define `generate_items` post-actions that inject dynamic task items with per-item variables
- `item_select` builtin step picks a winner from evaluated candidates using configurable strategies
- Invariants defined in `safety.invariants` are evaluated at configured checkpoints
- Protected file modification is detected and blocks execution when `on_violation: halt`
- Spawn depth limits are enforced; excess spawns are logged and skipped
- All existing tests pass; no regressions
