# Orchestrator - Process Timeline Read Model

**Module**: Orchestrator  
**Status**: Approved  
**Related Plan**: FR-095 semantic timeline projection, public read APIs, and operator UI  
**Related QA**: `docs/qa/orchestrator/142-process-timeline-read-model.md`  
**Created**: 2026-07-12  
**Last Updated**: 2026-07-12

## Background

Task execution data already existed across `tasks`, `task_items`, `command_runs`, `events`, logs, and the post-mortem trace builder. Operators still had to reconstruct intent, execution, evidence, and failure causes from raw logs. The process timeline provides a stable, semantic read model without becoming a second execution authority.

## Goals

- Produce deterministic, ordered entries for goals, lifecycle transitions, steps, tests, failures, recovery, sessions, and completion.
- Retain bounded links to command-run and artifact evidence.
- Support snapshot pagination and live reconciliation across CLI, Tauri, and React.
- Preserve existing task info, trace, watch, and log contracts.

## Non-goals

- Persisting rendered timeline cards or replacing the `events` audit source.
- Returning stdout/stderr bodies or raw filesystem paths in list responses.
- Implementing attention assignment, handoff generation, or resume mutations.
- Retrofitting every legacy producer with correlation fields in this slice.

## Scope

- In scope: an on-read projection, versioned domain model, opaque cursor, unary and streaming gRPC methods, CLI rendering, Tauri bridge, and default React task-detail timeline.
- Out of scope: global search, transcript indexing, materialized timeline storage, and external source ingestion.

## UI Interactions

- Page: implemented in `gui/src/pages/TaskDetail.tsx`, presented by FR-100 as `ProcessWorkspace` under Processes.
- The default "进程时间线" tab renders semantic entries and evidence.
- The "实时日志" tab preserves explicit log following.
- "专家" and "跟踪" preserve expert data and the raw structured trace.

## API

- `TaskTimeline(TaskTimelineRequest) returns (TaskTimelineResponse)` supports `task_id`, opaque `cursor`, `limit`, and repeated category filters.
- `TaskTimelineFollow(TaskTimelineFollowRequest) returns (stream TimelineDelta)` starts after an event watermark and emits `upsert` or `reset_required` deltas.
- `projection_version = 1` is carried by entries and pages.
- Existing RPCs are additive and unchanged.

## Database Changes

No migration or new table is required. `TaskRepository::load_task_timeline_source` reads a consistent transaction snapshot of the task, items, command runs, and uncapped events, with a maximum-event watermark.

## Key Design

1. `scheduler::timeline` projects semantic entries on read from durable execution records.
2. Source event ID plus a suborder is the canonical ordering key. Display timestamps are not trusted for pagination because clocks and grouped entries can be late.
3. Grouped IDs are SHA-256 hashes of task, category, source event IDs, correlation key, and projection version.
4. Cursors encode projection version, snapshot watermark, source order, and entry ID. Later writes cannot shift pages inside a fixed snapshot.
5. A command run becomes reference-only evidence using daemon-owned `orchestrator://` URIs; raw log paths are not serialized.
6. Live streams are bounded and terminate for terminal tasks. A burst over 200 entries produces `reset_required`, causing clients to reload an authoritative snapshot.
7. Runtime redaction patterns and project SecretStore values are applied before summaries or evidence labels cross the service boundary.

## Alternatives And Tradeoffs

- A materialized timeline table would reduce repeated projection work but would commit event semantics to migrations too early. On-read projection keeps semantics evolvable.
- Timestamp cursors are familiar but unsafe under late events. Source ordering is less intuitive internally and deterministic externally.
- Sending bare streamed entries is simple but cannot express invalidation. `TimelineDelta` makes reconciliation explicit.

## Risks And Mitigations

- Risk: large histories increase projection cost.
  - Mitigation: bounded pages, category filters, snapshot watermarks, and a future-compatible materialization seam.
- Risk: legacy events omit correlation fields.
  - Mitigation: task-item, step, phase, and command-run heuristics with explicit projection versioning.
- Risk: grouped entries hide audit detail.
  - Mitigation: every entry retains `raw_event_ids` and typed evidence references.
- Risk: sensitive runner output leaks.
  - Mitigation: structured extraction, bounded summaries, central redaction, and no raw filesystem paths.

## Observability

- Structured projection logs include task ID, cursor presence, source event count, command-run count, entry count, projection version, and duration without summary text.
- Control-plane protection classifies `TaskTimeline` as read traffic and `TaskTimelineFollow` as stream traffic.
- Default recommendation: add projection latency and response-size histograms if production profiling shows this read path is material.

## Operations / Release

- Config: no new configuration is required; existing runner redaction policy applies.
- Migration: none.
- Rollback: remove the GUI default tab and stop exposing the two additive RPC handlers; persisted execution state remains unchanged.
- Compatibility: existing clients ignore the additive proto surface and continue using `TaskInfo`, `TaskTrace`, `TaskFollow`, and `TaskWatch`.

## Test Plan

- Unit tests cover deterministic IDs, legacy missing fields, redaction, category validation, failed-workflow semantics, and cursor continuity.
- Repository tests cover uncapped event reads and event-watermark snapshots.
- CLI tests cover parser and UTF-8-safe table rendering.
- The isolated daemon QA script verifies a recorded failed workflow, evidence, pagination, and filtering.
- Frontend production build verifies the Tauri/React contract; visible-entry and accessibility checks remain in the QA document.

## QA Docs

- `docs/qa/orchestrator/142-process-timeline-read-model.md`

## Acceptance Criteria

- A failed workflow renders ordered goal, step/test, failure, and lifecycle entries.
- Failures contain a useful reason and command-run evidence.
- Reprojection produces stable IDs and ordering, including legacy events.
- Stable snapshot pagination has no duplicates or omissions.
- Tauri and React render the timeline while preserving logs and expert trace access.
- Existing task read and streaming contracts remain compatible.

## Major Code Touchpoints

- `crates/orchestrator-scheduler/src/scheduler/timeline/`
- `core/src/task_repository/queries.rs`
- `crates/proto/orchestrator.proto`
- `crates/daemon/src/server/task.rs`
- `crates/cli/src/output/timeline.rs`
- `crates/gui/src/commands/task.rs`
- `gui/src/components/ProcessTimeline.tsx`
