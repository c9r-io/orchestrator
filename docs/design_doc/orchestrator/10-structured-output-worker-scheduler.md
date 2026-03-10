# Orchestrator - Structured Output Mainline and Worker Scheduler

**Module**: orchestrator
**Status**: Approved
**Related Plan**: Close implementation gap where `collab` stayed off mainline; switch scheduler to structured output decision path and introduce a real queue/worker scheduling layer for C/S mode.
**Related QA**: `docs/qa/orchestrator/20-structured-output-worker-scheduler.md`
**Created**: 2026-02-23
**Last Updated**: 2026-02-23

## Background And Goals

## Background

Audit feedback identified two architecture gaps:

- `collab` data model existed but scheduler decisions still centered on `exit_code + log files`.
- CLI task execution still relied on per-command runtime creation and optional inline execution.

The refactor integrated structured outputs into phase execution and added queue/worker control for daemon-managed execution.

## Goals

- Enforce structured output validation for critical phases (`qa`, `fix`, `retest`, `guard`).
- Persist structured phase outputs for queryable post-run analysis.
- Publish phase results to the collaboration message bus from scheduler mainline.
- Add queue + worker command flow aligned with daemon-managed C/S execution.

## Non-goals

- Replace current workflow execution with DAG scheduling in this phase.
- Redesign all existing workflow semantics.

## Scope And User Experience (If Applicable)

## Scope

- In scope:
  - Strict output validation module and scheduler integration.
  - `command_runs` schema extension for structured data.
  - Scheduler events for validation and publication.
  - Queue-based task lifecycle commands and embedded daemon workers.
  - Shared process runtime for CLI instead of per-command runtime creation.

- Out of scope:
  - DAG execution engine replacement.
  - Web UI changes.

## UI Interactions (If Applicable)

- Not applicable (CLI-focused change).

## Interfaces And Data (If Applicable)

## API (If Applicable)

- No external HTTP/gRPC interface changes.

## Database Changes (If Applicable)

- Added `command_runs` columns:
  - `output_json`
  - `artifacts_json`
  - `confidence`
  - `quality_score`
  - `validation_status`
- Added indexes for `validation_status` and phase/time access path.

## Key Design And Tradeoffs

## Key Design

1. Introduce `output_validation` as the scheduler-side strict validator and `AgentOutput` normalizer.
2. Keep `run_phase` as execution choke point: validate, persist, emit events, publish bus message in one place.
3. Add `scheduler_service` for enqueue/pending scan/stop-signal behaviors.
4. Preserve foreground command path and add detach mode to minimize operator friction.

## Alternatives And Tradeoffs

- Option A: keep compatibility with plain-text phase output.
  - Pros: easier migration.
  - Cons: audit gap remains and decision boundary stays ambiguous.
- Option B: strict critical-phase JSON requirement (chosen).
  - Pros: deterministic contract and traceable structured decisions.
  - Cons: requires agent template updates.

## Risks And Mitigations

- Risk: existing agent templates output plain text and fail after rollout.
  - Mitigation: explicit `output_validation_failed` events and QA docs for migration verification.
- Risk: worker process coordination confusion in local usage.
  - Mitigation: explicit daemon startup guidance plus `task list/info/watch/logs` for queue observation.

## Observability And Operations (Required)

## Observability

- Logs/events:
  - `output_validation_failed`
  - `phase_output_published`
  - `scheduler_enqueued`
- Persisted run payloads:
  - `command_runs.output_json`
  - `command_runs.artifacts_json`
  - `command_runs.validation_status`

## Operations / Release

- Config: no new environment variables.
- Release flow:
  1. Update agent templates for strict JSON on critical phases.
  2. Run QA doc 20 scenarios.
  3. Roll out worker-based detach usage where needed.
- Rollback:
  - Revert scheduler validation enforcement and schema-dependent reads in same release unit.

## Testing And Acceptance

## Test Plan

- Unit tests:
  - Strict validation pass/fail behavior in `output_validation`.
- Integration tests:
  - Scheduler command run persistence and event emission.
  - CLI detach + worker lifecycle handling.
- Regression tests:
  - Existing task lifecycle and repository refactor suites remain green.

## QA Docs

- `docs/qa/orchestrator/20-structured-output-worker-scheduler.md`
- Updated impacted docs:
  - `docs/qa/orchestrator/10-agent-collaboration.md`
  - `docs/qa/orchestrator/19-scheduler-repository-refactor-regression.md`
  - `docs/qa/orchestrator/02-cli-task-lifecycle.md`
  - `docs/qa/orchestrator/00-command-contract.md`

## Acceptance Criteria

- Scheduler uses structured output validation in main phase execution path.
- `command_runs` stores structured payload and validation status.
- Phase publication events are queryable in `events`.
- Queue-based task lifecycle commands support daemon-managed execution.
- CLI no longer creates a new runtime per task command.
