# FR-095: Process Timeline Read Model

## 优先级: P0

## 状态: Proposed

## 依赖: None (Agent Process Console Phase 1)

## 计划闭环产物

- `docs/design_doc/orchestrator/105-process-timeline-read-model.md`
- `docs/qa/orchestrator/142-process-timeline-read-model.md`

## Background

`TaskInfo` already returns task, item, command-run, and event data, while `TaskTrace` reconstructs cycles, steps, durations, and anomalies. The Tauri bridge currently projects only the task and item summaries, and the React task detail emphasizes raw log streaming. Operators therefore have data but not an explanation of what happened, why the current state exists, or which evidence supports it.

## Goals

- Provide a deterministic, paginated, operator-oriented timeline for one task/process.
- Group low-level events into semantic entries without losing links to raw evidence.
- Explain goals, state transitions, steps, tests, failures, retries, human actions, source updates, checkpoints, sessions, and completion.
- Reuse events, command runs, task trace, artifacts, and log files rather than duplicating execution state.
- Support both snapshot queries and live updates.

## Non-goals

- Replacing the `events` audit table.
- Storing rendered UI cards in SQLite.
- Returning complete stdout/stderr bodies in timeline list responses.
- Changing scheduler ordering or task lifecycle semantics.
- Implementing attention assignment or resume mutations.

## Scope

### In scope

- Timeline projection domain types and builder.
- Canonical event-envelope additions needed for correlation.
- Evidence references and bounded summaries.
- gRPC, CLI, Tauri, and React timeline reads.
- Cursor pagination, live invalidation, redaction, and projection-version handling.

### Out of scope

- Cross-task/global timeline search.
- Full-text transcript indexing.
- External source ingestion; FR-099 will supply source events later.
- Materialized timeline tables in the first implementation.

## Interfaces and Data Changes

### Canonical timeline entry

```rust
pub struct TimelineEntry {
    pub id: String,
    pub task_id: String,
    pub occurred_at: String,
    pub category: TimelineCategory,
    pub title: String,
    pub summary: String,
    pub status: Option<String>,
    pub actor: Option<ActorRef>,
    pub step_id: Option<String>,
    pub task_item_id: Option<String>,
    pub command_run_id: Option<String>,
    pub session_id: Option<String>,
    pub checkpoint_id: Option<String>,
    pub source_event_id: Option<String>,
    pub evidence: Vec<EvidenceRef>,
    pub raw_event_ids: Vec<String>,
    pub projection_version: u32,
}
```

`TimelineCategory` initially includes `goal`, `source`, `lifecycle`, `cycle`, `step`, `tool`, `test`, `artifact`, `failure`, `recovery`, `human_action`, `session`, and `completion`.

### Evidence reference

```rust
pub struct EvidenceRef {
    pub kind: String,
    pub label: String,
    pub uri: Option<String>,
    pub content_type: Option<String>,
    pub digest: Option<String>,
    pub redacted: bool,
}
```

Evidence references may point to daemon-owned log/artifact retrieval APIs. Filesystem paths must not be returned to remote clients unless authorized and normalized under the workspace or runtime data roots.

### Event envelope additions

New producers should include, when known:

- `schema_version`
- `actor_type` and `actor_id`
- `correlation_id`
- `causation_id`
- `run_id` or execution incarnation
- `command_run_id`
- `session_id`
- `checkpoint_id`
- `evidence_refs`

Existing events remain valid. The builder must tolerate missing fields and use current task item/phase/time heuristics as a compatibility fallback.

### gRPC

```proto
rpc TaskTimeline(TaskTimelineRequest) returns (TaskTimelineResponse);
rpc TaskTimelineFollow(TaskTimelineFollowRequest) returns (stream TimelineEntry);
```

Requests support `task_id`, `after_cursor`, `limit`, category filters, and an `include_notice` flag. The cursor is opaque and stable for the query ordering `(occurred_at, entry_id)`.

## Key Design

### Projection rather than new execution state

The timeline builder reads a consistent snapshot of task metadata, task items, command runs, events, and trace anomalies. It converts these records into semantic entries. The first implementation computes the projection on read so event semantics can mature without a timeline migration.

If profiling later shows unacceptable query cost, the same builder can write a versioned materialized projection without changing the public API.

### Stable identity and grouping

- One-to-one entries use the source event ID.
- Grouped entries use a deterministic hash of task ID, category, correlation key, source event range, and projection version.
- Repeated heartbeats and log chunks are summarized, not rendered as individual timeline entries.
- A step entry may aggregate `step_started`, `step_spawned`, `step_finished`, the command run, and linked artifacts.
- Failure entries include the nearest structured cause and evidence, never only `exit code 1` when richer data exists.

### Snapshot and live behavior

`TaskTimeline` is authoritative. `TaskTimelineFollow` delivers newly projectable entries or invalidation notifications. The GUI periodically reconciles with a snapshot cursor so reconnection and late event arrival cannot permanently reorder the view.

### Redaction

Summaries are derived from already-normalized fields where possible. Any text from stdout, stderr, transcripts, tool inputs, or external source payloads passes through the runtime redaction policy before serialization.

## Tradeoffs

- On-read projection minimizes schema commitment but increases query CPU. Cursor limits and caching are sufficient for the first local-first release.
- Semantic grouping is more useful than raw audit order but can hide detail. Every entry therefore retains raw event IDs and evidence links.
- A generic entry model avoids UI coupling but can become vague. Categories and evidence kinds are closed enums in Rust/proto while payload extensions remain versioned.

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| Late or duplicated events create unstable entries | Deterministic IDs, source ordering, and reconciliation |
| Old events lack correlation fields | Compatibility heuristics plus explicit `projection_version` |
| Timeline response becomes too large | Cursor pagination and reference-only evidence |
| Sensitive output leaks into summaries | Central redaction and bounded structured extraction |
| UI depends on raw event names | Stable timeline categories at the API boundary |

## Observability and Operations

- Metrics: `timeline_projection_seconds`, `timeline_entries_total{category}`, `timeline_projection_error_total`, and response byte histograms.
- Logs include task ID, cursor, event count, command-run count, projection version, and duration, but not summary content.
- A debug-only CLI flag may include raw event IDs for projection diagnosis.
- Projection-version changes require fixture snapshots and release notes when rendered semantics change materially.

## Testing and Acceptance

Detailed QA will be created at `docs/qa/orchestrator/142-process-timeline-read-model.md` after implementation is approved.

Acceptance criteria:

- [ ] A recorded failed workflow produces ordered goal, step, test, failure, and lifecycle entries.
- [ ] The failure entry links to command-run evidence and exposes a useful structured reason.
- [ ] Replaying the same source rows produces byte-equivalent entry IDs and ordering.
- [ ] Missing optional correlation fields do not break legacy tasks.
- [ ] Pagination has no duplicates or omissions across stable snapshots.
- [ ] Tauri and React render the timeline while preserving access to current logs and expert trace data.
- [ ] Existing `TaskInfo`, `TaskTrace`, `TaskFollow`, and CLI output remain compatible.
