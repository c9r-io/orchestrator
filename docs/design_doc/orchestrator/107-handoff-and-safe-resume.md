---
lifecycle: active
related_fr: FR-097
---

# Orchestrator - Handoff And Safe Resume

**Module**: Orchestrator  
**Status**: Approved  
**Related Plan**: FR-097 immutable task handoffs, logical resume boundaries, two-stage consequence preview/execution, and provider-neutral session reuse  
**Related QA**: `docs/qa/orchestrator/144-handoff-and-safe-resume.md`  
**Created**: 2026-07-12  
**Last Updated**: 2026-07-25

> Focus entry, trapping, restoration, and async invalidation for the review dialog are governed by [DD-132](132-handoff-dialog-focus-lifecycle.md) and [QA-170](../../qa/orchestrator/170-handoff-dialog-focus-lifecycle.md).

## Background

Task status and semantic timelines explain what happened, but operators still need a concise way to hand work to another session and a safe way to resume from a failure. Direct pause/resume/retry controls do not show consequences, do not detect state drift between review and execution, and cannot distinguish workspace-only replay from repeated external side effects. Provider session identifiers also require a boundary that prevents opaque tokens from entering APIs, logs, or UI state.

## Goals

- Generate immutable, deterministic handoff snapshots from persisted task evidence.
- Offer stable logical resume boundaries independent of destructive git rollback checkpoints.
- Separate consequence planning from mutation and reject stale, expired, or replayed plans.
- Fail closed for undeclared and non-idempotent external effects.
- Reuse provider sessions through an internal runner adapter while keeping provider tokens opaque.
- Make the reviewed handoff/resume flow visible from the task/process workspace and remove direct GUI retry/resume bypasses.

## Non-goals

- Live terminal attachment, session input, or takeover; those are implemented separately by DD-108/QA-145.
- Reconstructing a full transcript or placing raw prompts/output in the briefing.
- Git reset, checkout, stash application, or any other workspace rollback.
- Replacing Attention Inbox, task state, process timeline, or existing low-level CLI lifecycle commands.
- Provider-specific session identifiers in public protobuf or Tauri models.

## Scope

- In scope: migration 28, deterministic projection, side-effect classification, state versions, expiring plans, idempotent execution reservation, four resume modes, scheduler child enqueue, provider adapter, gRPC/CLI/Tauri, and the task-detail panel/dialog.
- Out of scope: arbitrary workflow repair, live session attach, generated prose via an LLM, cross-project handoff search, and full Process Console information architecture.

## UI Interactions

- Page: implemented in `gui/src/pages/TaskDetail.tsx`, presented by FR-100 as `ProcessWorkspace` under Processes.
- Visible entry: "Handoff & safe resume" panel directly after task summary.
- Key buttons: "Generate handoff", "Preview resume", "Create preview", and "Execute reviewed plan".
- The dialog exposes boundary, side-effect class, resume mode, no-rollback statement, expiry, operator reason, and elevated confirmation when required.
- Focus enters the modal, cycles within it, closes on `Escape`, and returns to "Preview resume".
- Existing direct task-detail "Resume" and "Retry" actions are removed so relevant GUI recovery cannot bypass preview.

## API

- `HandoffGenerate(task_id, source_event_cursor?)` requires `operator+` because it persists an immutable snapshot.
- `HandoffGet(id)` and `ResumeBoundaryList(task_id)` require `read_only+`.
- `ResumePlan(task_id, boundary_id, mode, attention_item_id?)` requires `operator+`; it persists an expiring preview but does not mutate task/workspace state.
- `ResumeExecute(plan_id, expected_state_version, operator_reason, idempotency_key, elevated_confirmation)` requires `operator+`.
- Modes are `continue_task`, `retry_item`, `restart_from_boundary`, and `resume_provider_session`.
- Invalid/stale/expired plans return a failed precondition; unsafe replay without both policy and confirmation is denied.

## Database Changes

- `handoff_snapshots`: immutable structured briefing, event cursor, projection version, canonical SHA-256, state version, actor, and timestamp. `UNIQUE(task_id, source_event_cursor, content_hash)` makes same-cursor generation convergent.
- `resume_plans`: boundary, mode, expected version, consequence JSON, side-effect class, expiry, provider command-run reference, and lifecycle status.
- `resume_executions`: actor, required reason, idempotency key, request hash, terminal result, optional correlated child task, and canonical `request_id`. `UNIQUE(plan_id, idempotency_key)` prevents repeated scheduler effects; FR-101 supplies the shared envelope.
- Migration 28 is additive and leaves existing task, event, Attention, and checkpoint state unchanged.

## Key Design

1. Handoff content is structured first. Goal, current state, last success, failure, test/QA/lint evidence, changed-file paths, constraints, decisions, questions, and recommendations are bounded and sensitive keys are removed before canonical hashing.
2. The content hash excludes ID, actor, and timestamps. Repeating generation at the same event cursor returns the same persisted snapshot.
3. A task state version hashes task status/cycle/init state, pipeline variables, execution plan, update timestamp, event watermark, and a git workspace digest covering HEAD, tracked binary diff, and untracked file content. Execute recomputes it after the operator reviews the plan.
4. `SideEffectClass` is declared on `StepBehavior`: `none`, `workspace_only`, `idempotent_external`, or `non_idempotent_external`. The default is non-idempotent; agent/command steps without an explicit safe declaration fail closed.
5. Resume boundaries are logical scheduler positions. A checkpoint ID, when present, is reference-only; the resume executor never calls git rollback.
6. Restart modes create a child through the normal task creation/enqueue path with `parent_task_id`, `spawn_reason`, step filter, and resume correlation variables.
7. `RunnerSessionAdapter` is provider-neutral. The first Claude streaming adapter appends the opaque session token inside the runner boundary. Public models carry only a command-run reference and availability boolean.
8. Audit and Attention-visible state can change only after enqueue/state mutation succeeds. Failed planning and stale execution do not resolve or update human-action state.

## Alternatives And Tradeoffs

- Mutating directly from a "Retry" button is faster but cannot show consequences or detect concurrent state change. The two-stage plan is intentionally more explicit.
- Reusing git checkpoints as resume boundaries would conflate orchestration with destructive source control. Logical boundaries retain checkpoint references without rollback behavior.
- Returning provider tokens would simplify clients but expands the secret surface. Internal resolution adds one database lookup and preserves provider neutrality.
- Generating prose with an LLM could read better but would break deterministic hashing and add cost. Version 1 returns deterministic structure and leaves a future optional prose seam.

## Risks And Mitigations

- Risk: an operator executes a plan after another worker advances the task.
  - Mitigation: expected state version, expiry, exact boundary regeneration, and transactionally reserved execution.
- Risk: two clients repeat the same mutation.
  - Mitigation: unique plan/idempotency key plus durable `executing`/terminal status.
- Risk: a workflow omits side-effect metadata.
  - Mitigation: non-idempotent default and project policy disabled for elevated replay by default.
- Risk: provider resume fails after enqueue.
  - Mitigation: clear new-session fallback text; no silent provider substitution.
- Risk: handoff evidence leaks credentials or huge output.
  - Mitigation: key filtering, bounded strings/lists, no transcript/log bodies, and runner redaction.

## Observability

- Durable `resume_executions` records plan, actor, reason, request hash, child task, status, and error code.
- Successful state changes emit `resume_executed` with plan/execution/boundary/mode correlation but no briefing or provider token.
- Control-plane audit records classify handoff generation/planning/execution as writes and get/boundary list as reads.
- Default recommendation: export counts for planned, stale-rejected, elevated-denied, succeeded, and failed executions plus plan-to-execute latency.

## Operations / Release

- Config: `handoff_enabled` defaults to `true`; `mutating_resume_enabled` and `elevated_resume_enabled` default to `false`.
- Migration: normal migration kernel upgrades schema to 28 before serving the RPCs.
- Rollout: enable `mutating_resume_enabled` per project after workflows declare safe side-effect classes; keep elevated replay disabled unless externally idempotent behavior is reviewed.
- Rollback: disable both feature flags, revert clients/handlers, and leave additive tables in place. No task or workspace rollback is required.
- Compatibility: protobuf additions are additive; legacy lifecycle CLI remains available, while the task-detail GUI routes recovery through preview.

## Test Plan

- Core tests cover deterministic hash/idempotent snapshot retrieval, stale rejection, and default non-idempotent denial.
- Runner tests cover Claude command adaptation, shell quoting, unsupported-provider fallback, and no real provider/API use.
- The isolated daemon script on `127.0.0.1:19197` validates projection, redaction, boundaries, stale no-mutation behavior, correlated child enqueue, unsafe replay denial, and audit persistence.
- React production build and Tauri/Rust gates validate the visible panel and dialog contract.

## QA Docs

- `docs/qa/orchestrator/144-handoff-and-safe-resume.md`

## Acceptance Criteria

- A failed task handoff includes goal/current state, last success or failure, test evidence, changed files, and deterministic recommendations.
- The same task/event cursor produces the same structured content hash and snapshot.
- A stale plan is rejected before task, Attention, scheduler, or workspace mutation.
- Unknown/non-idempotent replay is denied unless project policy and operator confirmation both allow it.
- Restart creates a correlated child using existing enqueue semantics and never performs git rollback.
- Provider reuse failure explicitly recommends `restart_from_boundary`/a new session.
- Execution audit and human-visible updates occur only after an actual state change.

## Major Code Touchpoints

- `core/src/handoff.rs`
- `core/src/persistence/migration_steps.rs`
- `crates/orchestrator-runner/src/runner/session_adapter.rs`
- `crates/orchestrator-scheduler/src/scheduler/phase_runner/spawn.rs`
- `crates/daemon/src/server/handoff.rs`
- `crates/cli/src/commands/handoff.rs`
- `crates/gui/src/commands/handoff.rs`
- `gui/src/components/HandoffPanel.tsx`
