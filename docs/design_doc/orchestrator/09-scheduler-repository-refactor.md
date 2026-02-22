# Orchestrator - Scheduler Repository Refactor and Error Observability

**Module**: orchestrator
**Status**: Approved
**Related Plan**: Audit-driven debt remediation for scheduler internals (P0 mapping fix + P1 layering and observability). Replace fragile inline SQL mappings with repository APIs, reduce async-path blocking writes, and convert silent log-read failures to explicit errors.
**Related QA**: `docs/qa/orchestrator/19-scheduler-repository-refactor-regression.md`
**Created**: 2026-02-23
**Last Updated**: 2026-02-23

## Background And Goals

## Background

The scheduler path had a confirmed data-mapping defect in task summary loading and accumulated structural debt:

- Field mapping depended on positional column indexes, which caused `created_at` to be read from the `workflow_id` column.
- Scheduler logic mixed orchestration decisions with many direct SQL operations.
- Some log/file read failures were silently downgraded, reducing diagnosability.
- Async execution path still performed synchronous DB writes on the main runtime path.

## Goals

- Fix task summary mapping correctness and prevent positional-index regressions.
- Introduce a `TaskRepository` boundary for core scheduler data access.
- Keep behavior stable at the CLI level while reducing scheduler/DB coupling.
- Improve observability by surfacing log-read failures as actionable errors.
- Move command-run persistence out of the async hot path.

## Non-goals

- Full module split of `scheduler.rs` into multiple crates/files.
- Product-level redesign of agent collaboration flow (`collab` mainline decision).
- Schema changes or migration of existing table structures.

## Scope And User Experience

## Scope

- In scope:
  - New repository module for task-related reads/writes.
  - Scheduler refactor to consume repository APIs.
  - Regression test additions for repository and scheduler mapping.
  - Task log streaming behavior update from silent fallback to explicit failure.

- Out of scope:
  - New CLI command surface.
  - UI behavior changes.

## UI Interactions (If Applicable)

- Not applicable (CLI-only behavior adjustment).

## Interfaces And Data (If Applicable)

## API (If Applicable)

- No HTTP/gRPC API changes.

## Database Changes (If Applicable)

- Tables/columns: no schema changes.
- Access pattern changes:
  - Scheduler now reads/writes task state through repository methods.
  - `command_runs` insertion is wrapped in `spawn_blocking` to avoid blocking async workers.

## Key Design And Tradeoffs

## Key Design

1. Add `TaskRepository` trait with `SqliteTaskRepository` implementation for task-centric operations.
2. Switch sensitive summary mapping to column-name reads (`row.get("created_at")`) to remove index-coupling risk.
3. Route scheduler lifecycle operations (`resolve`, `summary`, `details`, `delete`, `runtime-context`, status updates) through repository.
4. Preserve current runtime semantics and event model while reducing direct SQL in scheduler orchestration code.
5. Return explicit errors when log file reads fail, including contextual path/run metadata.

## Alternatives And Tradeoffs

- Option A: Keep direct SQL but add local helper functions.
  - Pros: smaller diff.
  - Cons: keeps orchestration/data coupling and repeats mapping risks.

- Option B: Full domain-service split in one step.
  - Pros: cleaner architecture long-term.
  - Cons: larger blast radius and migration risk.

- Chosen: incremental repository extraction.
  - Pros: immediate risk reduction, testable boundary, low behavior drift.
  - Cons: scheduler file still large; further decomposition remains needed.

## Risks And Mitigations

- Risk: behavior drift in task detail/log flows after query extraction.
  - Mitigation: add repository-specific tests and keep CLI-facing scenarios in QA docs.

- Risk: stricter log error behavior surprises existing testers.
  - Mitigation: update QA lifecycle doc expectations to include explicit error outcome.

- Risk: partial refactor leaves mixed patterns.
  - Mitigation: define next-step backlog to continue extracting remaining orchestration concerns.

## Observability And Operations (Required)

## Observability

- Logs:
  - `task logs` now reports read failures with run/path context rather than silently returning empty chunks.
- Metrics:
  - Existing agent metrics remain unchanged.
- Tracing:
  - No new tracing spans introduced in this refactor.

## Operations / Release

- Config: no new environment variables.
- Migration / rollback:
  - Roll forward: deploy code only.
  - Rollback: revert scheduler/repository files; DB schema unchanged.
- Compatibility:
  - Backward compatible with existing SQLite data.

## Testing And Acceptance

## Test Plan

- Unit tests:
  - `scheduler` regression for timestamp mapping.
  - new `task_repository` tests for id resolution, counts, transactional start prep, command run persistence, deletion.
- Integration tests:
  - Existing CLI lifecycle tests remain valid; add QA scenario for log-read error visibility.
- E2E:
  - Not required for this internal refactor.

## QA Docs

- `docs/qa/orchestrator/19-scheduler-repository-refactor-regression.md`
- Impact update: `docs/qa/orchestrator/02-cli-task-lifecycle.md`

## Acceptance Criteria

- `load_task_summary` returns correct `created_at`/`updated_at`.
- Core scheduler task-data operations run through repository APIs.
- `command_runs` persistence is moved off async hot path.
- Missing log file in `task logs` no longer degrades silently.
- All repository and scheduler regression tests pass.
