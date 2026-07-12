---
self_referential_safe: true
---

# Orchestrator - Lightweight Step Run

**Module**: Orchestrator
**Scope**: Task step filtering and initial variable persistence for FR-090
**Scenarios**: 5
**Priority**: High

## Scenario 1: Unknown Step IDs Are Rejected

### Steps
1. Run `rg 'unknown step id' core/src/task_ops.rs`.
2. Run `cargo test --lib -p agent-orchestrator -- create_task`.

### Expected
- Validation checks every requested step against the execution plan and tests pass.

## Scenario 2: task create Exposes --step And --set

### Preconditions
- Use an explicit isolated project for any subsequent task creation, for example `--project {qa_project}`.

### Steps
1. Run `cargo run -- task create --help`.

### Expected
- `--step`, `--set`, and `--project` appear in help.

## Scenario 3: step_filter And initial_vars Are Persisted

### Steps
1. Run `rg 'step_filter_json|initial_vars_json' core/src/persistence/migration_steps.rs core/src/task_ops.rs`.

### Expected
- Migration `m0023` adds both columns and task creation persists both values.

## Scenario 4: TaskRuntimeContext Loads step_filter

### Steps
1. Run `rg 'step_filter' crates/orchestrator-scheduler/src/scheduler/runtime.rs crates/orchestrator-config/src/config/execution.rs`.

### Expected
- `TaskRuntimeContext` parses the persisted filter into `Option<HashSet<String>>`.

## Scenario 5: Scope Segments Respect step_filter

### Steps
1. Run `rg 'step_filter' crates/orchestrator-scheduler/src/scheduler/loop_engine/segment.rs`.

### Expected
- Segment construction skips steps absent from the task filter.

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Unknown step IDs are rejected | ☐ | | | |
| 2 | task create exposes --step and --set | ☐ | | | |
| 3 | step_filter and initial_vars are persisted | ☐ | | | |
| 4 | TaskRuntimeContext loads step_filter | ☐ | | | |
| 5 | Scope segments respect step_filter | ☐ | | | |

Runtime injection and direct assembly continue in `docs/qa/orchestrator/138b-lightweight-step-run-direct-assembly.md`.
