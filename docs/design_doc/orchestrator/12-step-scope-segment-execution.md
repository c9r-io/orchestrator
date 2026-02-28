# Orchestrator - StepScope & Segment-Based Execution

**Module**: orchestrator
**Status**: Approved
**Related Plan**: StepScope refactor — fix orchestrator item model so task-scoped steps (plan, implement) run once per cycle and item-scoped steps (qa_testing, ticket_fix) fan out per QA file
**Related QA**: `docs/qa/orchestrator/29-step-scope-segment-execution.md`
**Created**: 2026-02-28
**Last Updated**: 2026-02-28

---

## Background

The orchestrator previously created one "item" per QA file and ran the **full** pipeline for each item: plan → qa_doc_gen → implement → self_test → qa_testing → ticket_fix → align_tests → doc_governance. This meant N QA files = N plans + N implementations — fundamentally wrong.

The correct SDLC model: **task-scoped** steps (plan, implement, self_test) run **once per cycle**, while **item-scoped** steps (qa_testing, ticket_fix) fan-out per QA file. Like 1 architect + 1 developer + N QA engineers.

## Goals

- Task-scoped steps (plan, implement, build, test, self_test, qa_doc_gen, align_tests, doc_governance) execute exactly once per cycle regardless of item count
- Item-scoped steps (qa, qa_testing, ticket_fix, ticket_scan, fix, retest) fan out per QA file
- Pipeline variables from task-scoped steps propagate to subsequent item-scoped segments
- Item-scoped pipeline variables do NOT leak back to task scope
- Existing workflows work without YAML changes (default_scope handles classification)
- YAML `scope: task` / `scope: item` override available for non-standard configurations

## Non-goals

- Parallel item execution (future enhancement)
- Changes to agent selection or template rendering
- Changes to prehook evaluation logic
- Changes to finalize rules or task/item creation in task_ops.rs

---

## Scope

- In scope:
  - `StepScope` enum and `default_scope_for_step_id()` free function (formerly `default_scope()` on deleted `WorkflowStepType` enum)
  - Segment-based cycle execution in `loop_engine.rs`
  - Scope-aware step dispatch in `item_executor.rs`
  - YAML spec and config plumbing for `scope` field
  - Unit tests for segmentation logic
- Out of scope:
  - Parallel item execution
  - New YAML annotations in self-bootstrap.yaml (defaults suffice)

---

## Key Design

### 1. StepScope Enum

```rust
pub enum StepScope {
    Task,  // run once per cycle
    Item,  // fan out per QA file (default)
}
```

Each step `id` has a default scope via `default_scope_for_step_id()`:
- **Task**: plan, qa_doc_gen, implement, self_test, align_tests, doc_governance, review, build, test, lint, git_ops, smoke_chain, loop_guard, init_once (and any unknown id)
- **Item**: qa, qa_testing, ticket_fix, ticket_scan, fix, retest

> **Note**: The `WorkflowStepType` enum was deleted in the Unified Step Execution Model refactoring. Steps are now identified by string `id`. See `docs/design_doc/orchestrator/13-unified-step-execution-model.md`.

### 2. Segment-Based Execution

The execution plan steps are grouped into **contiguous segments** of the same scope:

```
Steps:  [plan, implement, self_test, qa_testing, ticket_fix, doc_governance]
Scopes: [Task, Task,      Task,      Item,       Item,       Task         ]

Segments:  ┌─── Task ───┐  ┌── Item ──┐  ┌ Task ┐
           plan+implement  qa_testing    doc_governance
           +self_test      +ticket_fix
```

For each segment:
- **Task segment**: Pick first item as context anchor, run steps once, propagate pipeline_vars forward
- **Item segment**: Iterate all items, run steps for each; item-level vars do NOT flow back to task scope

### 3. Step Filtering via process_item_filtered()

Rather than refactoring `process_item()` into separate functions (high risk, large diff), the approach adds a `step_filter: Option<&HashSet<String>>` parameter. When set, only steps whose `id` is in the filter run. This keeps the existing step execution logic intact while enabling segment-based dispatch.

Key touchpoints:
- Unified loop: all steps filtered by `should_run_step(&step.id)` via `StepExecutionAccumulator`
- Dynamic steps: only run when `step_filter` is `None` (legacy full mode)

> **Note**: The original step filter approach has been superseded by the unified execution loop with `StepExecutionAccumulator`. See `docs/design_doc/orchestrator/13-unified-step-execution-model.md`.

## Alternatives And Tradeoffs

- **Option A: Fully split process_item into process_task_steps + process_item_steps** — Cleaner separation but extremely large diff touching 700+ lines of battle-tested execution logic. High regression risk.
- **Option B: Step filter on existing process_item** (originally chosen) — Minimal diff, preserves existing logic verbatim, segment runner controls scope via filter. Lower risk.
- **Option C: Unified loop with StepExecutionAccumulator** (superseded Option B) — Deleted all hardcoded step logic. One generic loop where behaviors are declared as data. See `docs/design_doc/orchestrator/13-unified-step-execution-model.md`.
- Why Option B was originally chosen: The existing `process_item()` contained complex hardcoded step logic. The filter approach achieved scope-based execution with minimal blast radius. Option C later deleted all that hardcoded logic entirely.

## Risks And Mitigations

- Risk: Task-scoped steps use first item as anchor — item_id in events won't reflect "no item"
  - Mitigation: Acceptable for now; task-scoped events are logged with the anchor item_id. Future: introduce a synthetic "task" item.
- Risk: Scope override via YAML could create invalid segment orderings
  - Mitigation: `default_scope_for_step_id()` handles 100% of standard workflows. Override is opt-in for edge cases.

---

## Observability

- Logs: Existing `step_started`/`step_finished` events continue to fire. A `step_scope_segment` event could be added in future for segment-level tracing.
- Metrics: No new metrics. Existing `task_execution_metric` records final outcome.
- Tracing: Segment boundaries are visible by grouping step events by their scope (derivable from `step` field).

## Operations / Release

- Config: No new env vars. `scope` field in YAML is optional with defaults.
- Migration: Zero migration. Existing YAML/DB states work unchanged.
- Backward compatibility: `process_item_filtered()` unified loop handles all step types. The old `process_item()` wrapper has been removed.

---

## Test Plan

- Unit tests (5 new):
  - `test_default_scope_task_steps` — verifies all task-scoped step IDs
  - `test_default_scope_item_steps` — verifies all item-scoped step IDs
  - `build_segments_groups_contiguous_scopes` — 5 steps → 3 segments (Task/Item/Task)
  - `build_segments_skips_guards` — guard steps excluded from segments
  - `resolved_scope_uses_explicit_override` — explicit `scope: Task` overrides default
- Integration tests: 24 existing tests pass (including self-bootstrap fixture parsing)
- All 148 lib tests + 24 integration tests pass, clippy clean

## QA Docs

- `docs/qa/orchestrator/29-step-scope-segment-execution.md`

## Acceptance Criteria

- Plan runs once per cycle (not N times for N QA files)
- Implement runs once per cycle
- qa_testing fans out per QA file (runs N times)
- Pipeline variables from plan/implement propagate to qa_testing
- Existing self-bootstrap workflow YAML loads correctly without explicit `scope` annotations
- `cargo check`, `cargo test --lib`, `cargo clippy` all pass cleanly

---

## Files Changed

| File | Change |
|------|--------|
| `core/src/config.rs` | `StepScope` enum, `default_scope_for_step_id()`, `scope` field on step structs, `resolved_scope()` |
| `core/src/cli_types.rs` | `scope: Option<String>` on `WorkflowStepSpec` |
| `core/src/resource.rs` | Parse/serialize `scope` in spec↔config conversion |
| `core/src/config_load.rs` | Pass `scope` in `build_execution_plan()` |
| `core/src/scheduler/loop_engine.rs` | `ScopeSegment`, `build_scope_segments()`, segment dispatch loop |
| `core/src/scheduler/item_executor.rs` | `process_item_filtered()` unified loop with `StepExecutionAccumulator` |
