# Orchestrator - Step Variable Expansion Governance

**Module**: orchestrator
**Status**: Approved
**Related Plan**: Add QA coverage that verifies variable expansion is correct and complete across all supported workflow step families
**Related QA**: `docs/qa/orchestrator/82-step-variable-expansion-completeness.md`
**Created**: 2026-03-13
**Last Updated**: 2026-03-13

## Background

Workflow variable expansion is split across multiple runtime paths:

- `core/src/qa_utils.rs` renders basic placeholders such as `{rel_path}`, `{ticket_paths}`, `{phase}`, `{task_id}`, `{cycle}`, and `{unresolved_items}`
- `core/src/collab/context.rs` renders task/item/workspace context, pipeline vars, upstream outputs, shared state, and `{artifacts.count}`
- `core/src/scheduler/phase_runner/mod.rs` injects rendered step-template prompts into agent commands and expands runtime placeholders before spawn
- `core/src/scheduler/item_executor/dispatch.rs` renders builtin command steps through the same pipeline-aware context
- `core/src/scheduler/trace/anomaly.rs` detects leftover unexpanded placeholders in persisted command runs

Existing QA docs covered specific slices such as prompt delivery, self-bootstrap pipeline variables, and agent env resolution, but there was no single regression document defining completeness for variable expansion across all known step families.

## Goals

- Define one QA regression document that covers every supported placeholder family and every step-family render path.
- Make completeness explicit by tying step IDs to one of the actual rendering entry points.
- Add a guardrail scenario for leftover literal placeholders so regressions are caught even when individual commands still exit `0`.

## Non-goals

- Changing runtime rendering behavior.
- Adding new placeholder names or step IDs.
- Replacing focused QA docs such as prompt delivery or self-bootstrap pipeline coverage.

## Scope

- In scope: documentation for renderer unit coverage, pipeline/spill-file propagation, step-family entry-point audit, and task-trace anomaly detection.
- In scope: README index updates for the new design and QA documents.
- Out of scope: security docs, UI/UX docs, or new product behavior.

## Key Design

1. Treat “complete coverage” as three rendering surfaces plus one anomaly backstop:
   - basic placeholder renderer
   - agent/pipeline/shared-state renderer
   - runtime command/prompt injection path
   - task-trace detection of leftovers
2. Define step completeness by render path, not by duplicating twenty near-identical scenarios.
3. Reuse existing deterministic assets:
   - unit tests in `core/src/qa_utils.rs`, `core/src/collab/context.rs`, `core/src/scheduler.rs`, and `core/src/scheduler/trace/tests.rs`
   - mock manifest `fixtures/manifests/bundles/self-bootstrap-mock.yaml`
   - step inventory reference in `docs/qa/orchestrator/30-unified-step-execution-model.md`

## Alternatives And Tradeoffs

- Option A: one runtime workflow that executes every known step ID.
  - Pro: strongest end-to-end signal.
  - Con: high maintenance, heavy fixture authoring, and redundant with existing focused runtime tests.
- Option B: documentation built only from unit tests.
  - Pro: cheap and deterministic.
  - Con: does not prove the runtime command/prompt wiring or anomaly backstop.
- Why we chose: a hybrid document gives broad coverage with low drift by combining deterministic unit tests, one runtime propagation test, and one step-family audit.

## Risks And Mitigations

- Risk: future step IDs could be added without updating the completeness document.
  - Mitigation: the QA doc explicitly cross-checks the known step inventory in `docs/qa/orchestrator/30-unified-step-execution-model.md`.
- Risk: reviewers may assume prompt delivery docs already cover all placeholder families.
  - Mitigation: this doc separates prompt transport from prompt/command rendering semantics.
- Risk: silent regressions may leave literal placeholders in persisted commands while tasks still complete.
  - Mitigation: the QA doc includes explicit task-trace anomaly verification.

## Observability

- Logs: rendered commands are persisted in `command_runs.command`
- Metrics: no new metrics; rely on existing command-run and task lifecycle records
- Tracing: `task trace` anomaly output is the primary runtime diagnostic for unexpanded placeholders

## Operations / Release

- Config: no new config or environment variables
- Migration / rollback: doc-only change; rollback is removal of the new docs and README index entries
- Compatibility: additive only; no runtime behavior changes

## Test Plan

- Unit tests: renderer coverage in `core/src/qa_utils.rs` and `core/src/collab/context.rs`
- Integration tests: scheduler propagation and spill-file behavior in `core/src/scheduler.rs`
- E2E/manual QA: step-family audit and trace anomaly verification in `docs/qa/orchestrator/82-step-variable-expansion-completeness.md`

## QA Docs

- `docs/qa/orchestrator/82-step-variable-expansion-completeness.md`

## Acceptance Criteria

- The repository contains one QA document that defines complete variable-expansion coverage for all known workflow step families.
- The QA document references concrete commands, unit tests, and runtime evidence instead of generic prose-only checks.
- The QA and design README indexes include the new documents.
