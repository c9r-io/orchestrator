# Orchestrator - Unified Step Execution Model

**Module**: orchestrator
**Status**: Approved
**Related Plan**: Unified Step Execution Model — Clean Rewrite (delete hardcoded step logic, one generic loop, behaviors declared as data)
**Related QA**: `docs/qa/orchestrator/30-unified-step-execution-model.md`
**Created**: 2026-02-28
**Last Updated**: 2026-02-28

---

## Background

The `process_item_filtered()` function in `item_executor.rs` had grown to ~900 lines with two execution paths:

1. **Lines 377-797**: 5 hardcoded steps (plan → qa → ticket_scan → fix → retest) with bespoke control flow, status transitions, and special-case logic per step type.
2. **Lines 799-1050**: Generic loop with special-case branches for self_test, smoke_chain, build/test/implement.

Both paths relied on `WorkflowStepType` enum matching to determine behavior — tightly coupling step identity to hardcoded execution logic. Adding a new step type required modifying multiple match arms across several files.

## Goals

- **Delete all hardcoded step logic** — one generic loop replaces ~900 lines
- **Behaviors declared as data** — `StepBehavior` struct captures on_failure, on_success, captures, post_actions, execution mode
- **String-based step identification** — steps identified by `id` string, not enum variant
- **Accumulator pattern** — `StepExecutionAccumulator` tracks pipeline state across the unified loop
- **No backward compatibility shims** — one-shot clean migration

## Non-goals

- Changes to segment-based execution in `loop_engine.rs` (only import cleanup)
- Changes to prehook CEL evaluation logic (only context construction moves to accumulator)
- Changes to finalize rules or their CEL expressions
- Changes to agent selection or template rendering

---

## Scope

- In scope:
  - Delete `WorkflowStepType` enum and all references (10 files)
  - Add `StepBehavior`, `OnFailureAction`, `OnSuccessAction`, `CaptureDecl`, `CaptureSource`, `PostAction`, `ExecutionMode` types
  - Add `validate_step_type()`, `default_scope_for_step_id()`, `has_structured_output()` free functions
  - Remove `step_type` field from `WorkflowStepConfig` and `TaskExecutionStep`
  - Add `StepExecutionAccumulator` and rewrite `process_item_filtered()` as unified loop
  - Update `normalize_workflow_config()` to use string-based step identification
  - Update all test code (~670 lib + 80 item_executor + 24 integration tests)
- Out of scope:
  - YAML workflow file changes (steps still identified by `id`)
  - Parallel item execution
  - New YAML `behavior:` annotations (defaults suffice for current workflows)

---

## Key Design

### 1. String-Based Step Identification

Replaces the `WorkflowStepType` enum with free functions:

```rust
pub fn validate_step_type(value: &str) -> Result<String, String>
pub fn default_scope_for_step_id(step_id: &str) -> StepScope
pub fn has_structured_output(step_id: &str) -> bool
```

Steps are identified by their `id` field (a `String`), not by an enum variant. The `resolved_scope()` method on `TaskExecutionStep` now calls `default_scope_for_step_id(&self.id)` instead of `self.step_type.default_scope()`.

### 2. StepBehavior Data Structure

```rust
pub struct StepBehavior {
    pub on_failure: OnFailureAction,   // Continue | SetStatus | EarlyReturn
    pub on_success: OnSuccessAction,   // Continue | SetStatus
    pub captures: Vec<CaptureDecl>,    // var + source (Stdout/Stderr/ExitCode/FailedFlag/SuccessFlag)
    pub post_actions: Vec<PostAction>, // CreateTicket | ScanTickets
    pub execution: ExecutionMode,      // Agent | Builtin { name } | Chain
    pub collect_artifacts: bool,
}
```

All fields default to "do nothing" via `Default` impls. Current workflows use `StepBehavior::default()` — the behavior system is in place for future YAML-driven step declarations.

### 3. StepExecutionAccumulator

```rust
pub struct StepExecutionAccumulator {
    pub item_status: String,
    pub pipeline_vars: PipelineVariables,
    pub active_tickets: Vec<String>,
    pub created_ticket_files: Vec<String>,
    pub phase_artifacts: Vec<Artifact>,
    pub flags: HashMap<String, bool>,
    pub exit_codes: HashMap<String, i64>,
    pub step_ran: HashMap<String, bool>,
    pub step_skipped: HashMap<String, bool>,
    pub new_ticket_count: i64,
}
```

Methods:
- `to_prehook_context()` — builds `StepPrehookContext` from accumulated state
- `to_finalize_context()` — builds `ItemFinalizeContext` from accumulated state
- `apply_captures()` — writes capture results into flags/exit_codes/pipeline_vars

### 4. Unified Execution Loop

```
for step in execution_plan.steps:
    1. Evaluate prehook (skip if false)
    2. Execute (match on ExecutionMode: Agent | Builtin | Chain)
    3. Capture outputs (apply CaptureDecl results)
    4. Status transitions (on_success / on_failure actions)
    5. Post-actions (CreateTicket / ScanTickets)
    6. Collect artifacts (if collect_artifacts)
```

~200 lines replaces ~900 lines.

## Alternatives And Tradeoffs

- **Option A: Keep WorkflowStepType with added StepBehavior** — Backward compatible but leaves two parallel identification systems (enum + string). Every new step type still requires enum variant + match arm.
- **Option B: Delete WorkflowStepType entirely, string-based identification** (chosen) — Clean break, one identification system. Steps are data, not code. New step types need zero code changes if they use Agent execution mode.
- Why we chose B: The enum was the root cause of the hardcoded branching. As long as it existed, the temptation to add `match self { ... }` arms remained. Removing it forces all behavior to be declarative.

## Risks And Mitigations

- Risk: Removing `step_type` field could break serialized `execution_plan_json` in existing databases
  - Mitigation: The `step_type` field was `Option<WorkflowStepType>` with `skip_serializing_if = "Option::is_none"`. Steps are looked up by `id` string now via `step_by_id()`. Existing DB rows that contain `step_type` in JSON will be silently ignored by serde (unknown fields are skipped by default).
- Risk: `validate_step_type()` rejects custom step IDs not in the known list
  - Mitigation: This matches the old `WorkflowStepType::from_str()` behavior exactly. Custom step types were never supported.

---

## Observability

### Default Recommendations

- Logs: Existing `step_started`/`step_finished`/`step_skipped` events continue to fire with the step `id` in payload.
- Metrics: No new metrics. The accumulator pattern is internal and does not expose new observable state.
- Tracing: The unified loop emits the same events as the old hardcoded paths, so existing dashboards and log queries remain valid.

## Operations / Release

- Config: No new env vars. No YAML schema changes required.
- Migration: Zero DB migration. Existing `execution_plan_json` works unchanged (the removed `step_type` field is silently ignored during deserialization).
- Backward compatibility: None — this is a one-shot clean migration. All tests updated.

---

## Test Plan

- 670 lib tests pass (including updated config, config_load, resource, ticket, cli_types tests)
- 80 item_executor tests pass (accumulator-based execution model)
- 24 integration tests pass (fixture parsing, step execution, pipeline variables)
- 0 clippy warnings
- Key test categories:
  - `validate_step_type` validation of known and unknown IDs
  - `default_scope_for_step_id` scope classification for all known steps
  - `has_structured_output` identification of structured output steps
  - `resolved_scope` with explicit override vs. id-based default
  - `step_by_id` lookup replacing enum-based `step()` method
  - Full pipeline execution via self-bootstrap fixture

## QA Docs

- `docs/qa/orchestrator/30-unified-step-execution-model.md`

## Acceptance Criteria

- `WorkflowStepType` enum is fully deleted — zero references in codebase
- `step_type` field removed from `WorkflowStepConfig` and `TaskExecutionStep`
- `process_item_filtered()` uses unified loop with `StepExecutionAccumulator`
- All 774 tests pass (670 lib + 80 item_executor + 24 integration)
- `cargo build`, `cargo test`, `cargo clippy` all clean with 0 warnings

---

## Files Changed

| File | Change |
|------|--------|
| `core/src/config.rs` | Delete `WorkflowStepType` enum; add `StepBehavior` types; add `validate_step_type()`, `default_scope_for_step_id()`, `has_structured_output()`; remove `step_type` field from structs; update `resolved_scope()` |
| `core/src/scheduler/item_executor.rs` | Add `StepExecutionAccumulator`; rewrite `process_item_filtered()` as unified loop |
| `core/src/config_load.rs` | Remove `WorkflowStepType` references; use string-based step identification in `normalize_workflow_config()` and `build_execution_plan()` |
| `core/src/resource.rs` | Remove `WorkflowStepType` references; use `validate_step_type()` in spec↔config conversion |
| `core/src/scheduler/loop_engine.rs` | Replace `execution_plan.step(WorkflowStepType::InitOnce)` with `step_by_id("init_once")` |
| `core/src/ticket.rs` | Replace `execution_plan.step(WorkflowStepType::Qa)` with `step_by_id("qa")` |
| `core/src/cli_handler/task_session.rs` | Replace `WorkflowStepType` parsing with `validate_step_type()` and string matching |
| `core/src/cli_handler/parse.rs` | Delegate to `config::validate_step_type()` |
| `core/src/cli_handler/resource.rs` | Replace `step_type` display with `step.id` |
| `core/src/cli_handler/task_exec.rs` | Replace `step_type` lookup with `step.id` |
| `core/src/test_utils.rs` | Remove `WorkflowStepType` from test fixtures |
| `core/src/scheduler.rs` | Remove `WorkflowStepType` from test imports |
| `core/src/cli_types.rs` | Replace enum parsing test with `validate_step_type()` |
| `core/tests/integration_test.rs` | Remove all `WorkflowStepType` references |
