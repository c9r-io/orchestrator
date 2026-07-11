# Agent Process Console Roadmap

**Status**: Active roadmap; Phase 1 closed
**Governed FRs**: FR-096 through FR-100 (FR-095 closed)
**Planned closure artifacts**: design docs 105-110 and QA docs 142-147  
**Created**: 2026-07-12

## Background

Agent Orchestrator already provides durable tasks, workflow loops, structured events, command runs, task traces, anomaly detection, checkpoints, triggers, streaming runners, and persisted interactive sessions. The current GUI exposes these capabilities primarily as a wish pool and a progress observer. That presentation is useful for monitoring but does not yet optimize the scarce resource in loop engineering: human attention.

The product direction is an **Agent Process Console**. Agents should continue autonomously by default. Humans should see only decisions, approvals, exceptions, and blocked work, while retaining the ability to inspect evidence, resume execution, or take over an agent session without reconstructing context manually.

The first release must prove one vertical outcome:

> A failed or approval-blocked process appears in the Attention Inbox; an operator inspects its structured timeline and evidence, generates a handoff, resumes from a safe boundary or attaches to the agent session, and the attention item closes when the process advances.

## Goals

- Reframe the GUI from task-progress tracking to exception-driven process operations.
- Introduce stable product semantics for process timelines, attention items, handoffs, resumable execution, sessions, and external source bindings.
- Reuse the existing local-first daemon, SQLite persistence, gRPC control plane, Tauri shell, React frontend, event table, task trace, and session store.
- Deliver capability in independently testable vertical slices with explicit dependency gates.
- Preserve CLI and existing workflow compatibility while adding product-oriented read and control APIs.

## Non-goals

- Renaming the persisted `tasks` model to `processes` in the first release.
- Replacing SQLite or implementing a distributed scheduler.
- Shipping a browser-hosted multi-tenant SaaS control plane in this roadmap.
- Making Slack the canonical task model; Slack is one source adapter.
- Allowing arbitrary checkpoint rollback or terminal input without RBAC, audit, and idempotency controls.
- Generating QA documentation before each design slice is approved for implementation.

## Scope

### In scope

- Product semantics and dependency-ordered delivery for timeline, attention, handoff/resume, session control, source bindings, and GUI information architecture.
- Additive changes across core persistence/services, scheduler projections, gRPC, CLI, Tauri, and React.
- Release gates, cross-cutting security/observability rules, and implementation acceptance boundaries.

### Out of scope

- Detailed implementation code, migrations, proto definitions, frontend components, or QA procedures in this roadmap document.
- A commitment to calendar dates before the vertical slices are estimated against approved designs.
- Capabilities explicitly excluded by FR-095 through FR-100.

## Interfaces and Data Changes

This roadmap introduces no runtime interface by itself. It coordinates the additive interfaces and tables proposed by FR-095 through FR-100. Public changes must follow this order:

1. versioned domain/read models and forward-only migrations;
2. core repository and service boundaries;
3. additive gRPC and CLI contracts;
4. Tauri bridges and React views;
5. source adapters and notifications after the internal control loop is stable.

No phase may make the GUI or a provider adapter the authority for persisted state.

## Product Model

The first release keeps `Task` as the execution aggregate while exposing a process-oriented projection:

| Product concept | Initial backing model | Evolution point |
|---|---|---|
| Process | `tasks` plus descendants and source bindings | Split durable intent from runs only when cross-run requirements demand it |
| Run | task execution incarnation, cycle, and command runs | Add explicit run identity if retries need independent lifecycle |
| Step | workflow step plus task item and command run | Stable timeline entry identity |
| Session | `agent_sessions` and `session_attachments` | Provider-neutral resume adapters |
| Checkpoint | workflow safety checkpoint plus logical resume boundary | Persist logical checkpoint metadata |
| Attention item | new materialized table | Rules, assignment, SLA, notification routing |
| Evidence | command runs, artifacts, event payloads, log paths | Typed evidence registry if cross-source search is required |
| Handoff | new immutable snapshot table | Versioned summaries and export formats |
| Source binding | new source event/binding tables | Slack, GitHub, webhook, code analysis, and documents |

## Key Design

The roadmap is organized around vertical operational outcomes rather than horizontal component completion. Each phase must include persistence/service work where needed, public control-plane contracts, at least one client surface, observability, and a live or recorded end-to-end demonstration. Later phases reuse earlier product APIs instead of creating Slack-only or GUI-only mutation paths.

## Roadmap and Dependency Gates

### Phase 0: Semantic foundation

**Outcome**: shared vocabulary, event contract, and compatibility rules are fixed before schema and UI work.

| Task | Deliverable | Depends on | Acceptance gate |
|---|---|---|---|
| P0-01 | Approve the roadmap and FR-095 through FR-100 | None | Product terms and non-goals accepted |
| P0-02 | Define canonical event envelope additions | FR-095 | Required correlation fields and schema-version policy agreed |
| P0-03 | Freeze additive gRPC compatibility policy | FR-095, FR-096 | Existing CLI and GUI calls remain valid |
| P0-04 | Define operator RBAC/action audit policy | FR-096, FR-098 | Every mutating action has actor, reason, idempotency key, and audit event |

### Phase 1: Process timeline read model (Closed)

**Outcome**: operators can understand a task without reading raw logs.

| Task | Deliverable | Depends on | Acceptance gate |
|---|---|---|---|
| P1-01 | Timeline projection types and deterministic builder | P0 | Fixture events produce stable ordered entries |
| P1-02 | Evidence reference projection | P1-01 | Test results, artifacts, log paths, and failures are linkable |
| P1-03 | Timeline gRPC and CLI query | P1-01 | Pagination and cursor semantics validated |
| P1-04 | Tauri bridge and React timeline | P1-03 | Task detail explains state transitions and failure cause |
| P1-05 | Regression and performance tests | P1-01..04 | Existing `TaskInfo`, trace, logs, and watch behavior unchanged |

Closed by `docs/design_doc/orchestrator/105-process-timeline-read-model.md` and verified by `docs/qa/orchestrator/142-process-timeline-read-model.md`.

### Phase 2: Attention Inbox

**Outcome**: the default operational view contains only actionable human work.

| Task | Deliverable | Depends on | Acceptance gate |
|---|---|---|---|
| P2-01 | Attention schema, repository, and migration | P0 | State transitions and deduplication are transactional |
| P2-02 | Event-to-attention projector and policy registry | P1-01 | Replayed events converge without duplicate open items |
| P2-03 | List/get/claim/snooze/resolve/action RPCs and CLI | P2-01..02 | RBAC, optimistic versioning, and idempotency enforced |
| P2-04 | Attention Inbox UI and notifications | P2-03, P1-04 | Keyboard-first triage and deep-link to timeline work |
| P2-05 | Noise and recovery tests | P2-01..04 | Repeated loop failures create one actionable item |

Governed by [FR-096](FR-096-attention-inbox.md).

### Phase 3: Handoff and safe resume

**Outcome**: an operator can continue work without manually reconstructing context.

| Task | Deliverable | Depends on | Acceptance gate |
|---|---|---|---|
| P3-01 | Logical resume-boundary projection | P1 | Safe boundaries are explainable and immutable |
| P3-02 | Handoff snapshot schema and deterministic briefing | P1, P2 | Snapshot records source event watermark and content hash |
| P3-03 | Provider-neutral resume plan and execution API | P3-01..02 | Retry, new-session resume, and live attach remain distinct |
| P3-04 | Handoff and resume UI | P3-03 | Operator previews consequences before mutation |
| P3-05 | Rollback, stale-version, and redaction tests | P3-01..04 | Unsafe or stale resume requests fail closed |

Governed by [FR-097](FR-097-handoff-and-safe-resume.md).

### Phase 4: Agent session control plane

**Outcome**: active agent sessions become observable and safely attachable first-class resources.

| Task | Deliverable | Depends on | Acceptance gate |
|---|---|---|---|
| P4-01 | Session query service and gRPC read APIs | P0 | Existing session rows are inspectable without filesystem access |
| P4-02 | Transcript streaming and reader attachment | P4-01 | Offset resume and bounded backpressure work |
| P4-03 | Writer lease, send-input, detach, and close | P4-02 | Single-writer and audit invariants hold under races |
| P4-04 | Sessions UI and task-detail embed | P4-01..03, P1 | Live state, transcript, and lease ownership are visible |
| P4-05 | Crash, stale PID, cleanup, and security tests | P4-01..04 | PID is never accepted as the authoritative write identity |

Governed by [FR-098](FR-098-agent-session-control-plane.md). FR-098 supersedes the proposed implementation ordering in DD-075 while retaining its two-layer mailbox/session distinction.

### Phase 5: External source bindings and Slack pilot

**Outcome**: multiple Slack events correlate into durable processes without coupling process semantics to Slack.

| Task | Deliverable | Depends on | Acceptance gate |
|---|---|---|---|
| P5-01 | Source event and source binding schema | P0, P1 | Provider event ingestion is idempotent |
| P5-02 | Correlation and routing policy service | P5-01 | Same-thread append, new-thread create, and explicit branch are deterministic |
| P5-03 | Slack webhook verification and normalization adapter | P5-01..02 | Signature, replay, retry, and redaction tests pass |
| P5-04 | Slack actions for approve/retry/open-console | P2, P3, P5-03 | Actions use the same audited command path as GUI/CLI |
| P5-05 | Sources UI and provenance on timeline | P5-01..04, P1 | Every external update links to its process and source |

Governed by [FR-099](FR-099-source-events-and-slack-binding.md).

### Phase 6: Console information architecture and release hardening

**Outcome**: the GUI consistently presents the new operating model and is ready for daily use.

| Task | Deliverable | Depends on | Acceptance gate |
|---|---|---|---|
| P6-01 | Navigation migration to Attention/Processes/Sessions/Sources/System | P1..P5 | Existing expert functions remain reachable |
| P6-02 | Responsive three-pane operating layouts | P1..P5 | Dense views remain usable without blur support |
| P6-03 | Keyboard command surface and accessibility pass | P6-01..02 | Focus, contrast, labels, and reduced motion pass |
| P6-04 | Product telemetry and operational dashboards | P2..P5 | Attention and re-entry metrics are observable |
| P6-05 | Migration, release notes, and rollback runbook | All | Upgrade preserves existing task and session data |

Governed by [FR-100](FR-100-process-console-ui.md).

## Recommended Delivery Increments

The phases are dependency ordered, but releases should stay vertical:

1. **Read-only alpha**: P1 timeline plus read-only session inspection.
2. **Attention alpha**: one automatically materialized failure item with claim/resolve.
3. **Recovery beta**: handoff plus retry/new-session resume for one failed step family.
4. **Interactive beta**: live transcript and guarded writer attachment.
5. **Slack pilot**: one workspace, one routing policy, approve/retry buttons.
6. **Console v1**: complete navigation, accessibility, metrics, migration, and runbooks.

Each increment must be demoable against a recorded fixture and a live daemon. A phase is not complete when only schema or UI scaffolding exists.

## Tradeoffs

- FR-095 precedes FR-096 because actionable records need a trustworthy explanation and evidence surface.
- Attention precedes Slack because external volume would amplify noise before triage semantics are stable.
- Handoff precedes mutating session controls because safe asynchronous recovery is lower risk than interactive shell authority.
- Tauri remains the first client to reuse the current architecture; browser/multi-tenant deployment remains an explicit later decision.
- The roadmap avoids calendar promises so scope and dependency gates, rather than speculative duration, govern delivery.

## Cross-cutting Architecture Rules

- The daemon remains the authority; GUI and Slack adapters never mutate SQLite directly.
- All writes flow through service methods and gRPC with RBAC, audit, optimistic versioning where applicable, and idempotency keys.
- `events` remains the audit source; timeline is a read projection, while attention and handoff are materialized operational state.
- Raw provider session IDs and resume tokens are opaque values owned by a runner adapter.
- Transcript, evidence, source payload, and handoff rendering use the existing redaction policy before leaving the daemon.
- New streams use bounded buffers, cancellation, and control-plane occupancy limits.
- Existing CLI/task lifecycle commands remain backward compatible.
- Product UI uses the existing design tokens, but dense operational views prioritize contrast and scanability over glass effects.

## Product Metrics

- `attention_open_total{kind,severity}`
- `attention_time_to_claim_seconds`
- `attention_time_to_resolution_seconds`
- `process_human_attention_seconds`
- `process_autonomous_completion_ratio`
- `handoff_generation_seconds`
- `handoff_to_productive_action_seconds`
- `resume_attempt_total{mode,result}`
- `session_attachment_total{mode,result}`
- `source_event_deduplicated_total{provider}`
- repeated failure and degenerate-loop rates

Metrics must not include prompt text, transcripts, source message bodies, or secrets.

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| The product model diverges from persisted task semantics | Add projections first; defer destructive renames |
| Attention becomes a noisy error feed | Materialize only actionable policies; deduplicate and auto-resolve |
| Resume is mistaken for rollback or live attach | Use distinct APIs and UI verbs with consequence previews |
| Session controls expose arbitrary shell access | RBAC, single-writer lease, audit, redaction, and fail-closed lifecycle checks |
| Slack becomes tightly coupled to workflow state | Normalize into source events and use provider-neutral commands |
| Timeline queries overload SQLite | Cursor pagination, indexed projections, bounded payloads, and performance fixtures |
| GUI polish delays the control loop | Ship vertical operational slices before navigation-wide redesign |

## Observability and Operations

- Every new mutation emits a structured audit event with actor, target, request ID, reason, and result.
- Projection failures emit counters and retain replay cursors; they never silently discard source events.
- Migrations must be forward-only, restart-safe, and verified against a populated database fixture.
- Feature flags gate attention materialization, mutating session controls, resume actions, and Slack ingestion independently.
- Rollback disables new writers and projectors while preserving additive tables and read compatibility.

## Testing and Acceptance

Implementation QA will be authored in:

- `docs/qa/orchestrator/142-process-timeline-read-model.md`
- `docs/qa/orchestrator/143-attention-inbox.md`
- `docs/qa/orchestrator/144-handoff-and-safe-resume.md`
- `docs/qa/orchestrator/145-agent-session-control-plane.md`
- `docs/qa/orchestrator/146-source-events-and-slack-binding.md`
- `docs/qa/orchestrator/147-process-console-ui.md`

The roadmap is accepted when:

- The product model and phase boundaries are approved.
- Each implementation task maps to one owning module and one acceptance gate.
- The first vertical slice can be delivered without Slack or a new deployment model.
- Existing task, trace, trigger, and GUI capabilities have an explicit reuse or compatibility path.
