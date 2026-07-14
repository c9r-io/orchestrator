# Orchestrator - Attention Inbox

**Module**: Orchestrator  
**Status**: Approved  
**Related Plan**: FR-096 persistent cross-task attention queue, governed actions, and default operator UI  
**Related QA**: `docs/qa/orchestrator/143-attention-inbox.md`  
**Created**: 2026-07-12  
**Last Updated**: 2026-07-12

## Background

Task status and the process timeline describe work, but neither answers the operator's first question: "What needs my decision now?" Repeated loop failures also create noise when represented as raw events. Attention Inbox adds persistent, mutable operational state for only actionable exceptions, approvals, blockers, stalled work, and low-confidence results.

## Goals

- Materialize a cross-task queue from durable workflow events without making the queue an execution authority.
- Deduplicate repeated active conditions and preserve occurrence and reopen counts.
- Support authenticated claim, snooze, resolve, retry, resume, approve, reject, and acknowledge flows.
- Guarantee optimistic concurrency and retry-safe action idempotency.
- Make Attention Inbox the keyboard-first default desktop workspace while retaining task and wish views.
- Keep list and metric fields free of transcripts, raw output, commands, and secret-bearing error strings.

## Non-goals

- Replacing task state, the event audit log, or the semantic task timeline.
- User-defined attention policy expressions in the first release.
- Slack delivery, cross-control-plane federation, or on-call scheduling.
- Arbitrary executable commands stored in action descriptors.
- Interactive session takeover, implemented separately by DD-108/QA-145.

## Scope

- In scope: migration 27, repository, built-in policy registry, durable projector cursor, daemon reconciliation, unary/streaming gRPC, CLI, Tauri bridge, React UI, RBAC, action audit, and `RuntimePolicy.spec.attention_inbox_enabled`.
- Out of scope: custom CEL policies, notification delivery, global search, and organization-wide escalation schedules.

## UI Interactions

- The desktop default tab is "Attention Inbox" in `gui/src/App.tsx`.
- Filters cover lifecycle state, severity, and assignee (`me` or `unassigned`).
- Cards expose only daemon-advertised actions plus "认领", "稍后处理", "已处理", and "查看进程时间线".
- Keyboard controls are `J`/`K` selection, `C` claim, `R` resolve, and `Enter` timeline deep link.
- `read_only` identities see the same safe queue but all mutation controls are disabled.

## API

- `AttentionList` and `AttentionGet` are `read_only+`.
- `AttentionFollow` is a bounded server stream resumed by monotonic `after_change_id`.
- `AttentionClaim`, `AttentionSnooze`, `AttentionResolve`, and `AttentionExecuteAction` require `operator+`, `expected_version`, and `idempotency_key`.
- The actor is derived from the mTLS subject or UDS peer UID. No actor request field exists.
- List filters are `project_id`, `state`, `kind`, `severity`, `assignee`, and `task_id`.
- Mutation conflicts return gRPC `ABORTED`; malformed requests are `INVALID_ARGUMENT`; idempotency-key reuse with different input is rejected.

## Database Changes

- `attention_items` stores materialized state, active dedupe key, safe presentation fields, version, occurrence/reopen counts, snooze/resolution fields, and source correlation.
- `attention_actions` is the domain mutation/idempotency projection with actor, action ID, target version, result/error, timestamps, and the canonical `request_id`. FR-101's `control_action_audit` is the shared durable envelope.
- `attention_projector_state` stores the durable source-event cursor.
- `attention_changes` provides a monotonic follow sequence.
- `idx_attention_open_dedupe` is a partial unique index across `open`, `claimed`, and `snoozed` records.
- Migration 27 is additive. Existing task/event data is unchanged and replayable.

## Key Design

1. The daemon polls source events in bounded batches. Attention operations and cursor advancement commit in one SQLite transaction, making crash replay convergent.
2. A built-in policy registry maps relevant events to severity, structured titles, stable dedupe keys, safe action descriptors, and clear conditions. Initial kinds include approval, question, failed/retry-exhausted, policy/sandbox denial, stalled, budget, low-confidence, and degenerate-loop conditions.
3. Summaries are constructed only from validated identifiers such as task and step IDs. Arbitrary `error`, `message`, prompt, transcript, stdout, and stderr fields are never copied.
4. Repeated active conditions increment `occurrence_count`. A cleared condition resolves the matching step/task; recurrence reopens the same row and increments `reopen_count`.
5. Human mutations update by exact version and persist the authenticated actor in the same transaction.
6. External actions use a two-phase database reservation. Exactly one caller receives `should_execute=true`; completion records success or failure. A replay of the same action key returns current state without repeating the external side effect.
7. Follow clients consume `attention_changes` and reconcile by item ID. Existing rows remain readable when materialization is disabled.

## Alternatives And Tradeoffs

- Deriving the Inbox on every read would avoid tables but cannot represent ownership, snooze, resolution, or action audit. Materialization is required.
- Synchronous hooks at every event producer reduce latency but create broad coupling. A 750 ms bounded reconciler plus durable cursor provides simpler repair semantics.
- CEL policies are flexible but would expand the executable policy surface. A built-in registry is safer for the first release.
- Resolving immediately after a successful retry/resume reservation makes the action responsive; subsequent source failure reopens the same condition, preserving truth and history.

## Risks And Mitigations

- Risk: repeated loop events flood the queue.
  - Mitigation: active partial unique index and deterministic dedupe key.
- Risk: two operators trigger the same recovery.
  - Mitigation: version gate plus durable action reservation and request hash.
- Risk: projector restart loses or duplicates work.
  - Mitigation: transactionally advanced event cursor and idempotent upsert.
- Risk: raw agent text leaks into the default UI.
  - Mitigation: identifier allowlist, generic structured summaries, and no raw-output fields in the API.
- Risk: auto-resolution surprises an operator.
  - Mitigation: `resolution_json`, `attention_actions`, change history, and reopen counters remain queryable.

## Observability

- Reconciliation logs report source-event count and committed cursor without titles or summaries.
- Operational counts are directly queryable from `attention_items` by state/kind/severity; claim/resolution latency and outcomes are derivable from `attention_actions` timestamps.
- Projection lag is `MAX(events.id) - attention_projector_state.last_event_id`.
- Control-plane protection classifies follow as stream traffic, mutations as write traffic, and reads as read traffic.
- Default recommendation: export these SQL-derived counts and latency histograms through the platform metrics surface when a general metrics exporter is introduced.

## Operations / Release

- Config: `RuntimePolicy.spec.attention_inbox_enabled` defaults to `true`. Setting it to `false` stops new materialization but does not delete or hide existing rows.
- Migration: migration 27 runs through the normal migration kernel before the daemon starts reconciliation.
- Rollback: disable materialization, revert clients/RPC handlers, and leave additive tables in place. No task/event rollback is needed.
- Compatibility: proto additions are backward compatible; existing task, trace, timeline, and GUI progress flows remain available.

## Test Plan

- Repository unit tests cover dedupe aggregation, version conflicts, mutation idempotency, action reservation concurrency, and replay safety.
- Projector unit tests cover clear-event mapping and exclusion of raw error content.
- The isolated daemon QA script on port `19196` verifies materialization, duplicate aggregation, concurrent claim exclusion, auto-resolution, audit reason, RBAC mapping, and response safety.
- Workspace tests and clippy validate additive compatibility.
- Tauri compilation and the React production build validate the desktop contract; visible entry, keyboard controls, filters, and deep links are inspected against QA-143.

## QA Docs

- `docs/qa/orchestrator/143-attention-inbox.md`

## Acceptance Criteria

- Repeated identical failed-step events produce one active item and increment `occurrence_count`.
- Concurrent claims or actions cannot both execute against one version; replaying one action key does not repeat its side effect.
- Reprocessing source conditions converges on the same active dedupe set.
- Successful step/resume events automatically resolve matching active conditions with an auditable reason.
- Read-only identities cannot mutate; operator/admin identities follow the existing control-plane role policy.
- The default GUI supports navigation, filters, claim, snooze, resolve, allowlisted actions, and timeline deep links.
- List responses, logs, and operational counts contain no transcript body, raw command output, or unredacted secret text.

## Major Code Touchpoints

- `core/src/attention.rs`
- `core/src/persistence/migration_steps.rs`
- `crates/orchestrator-scheduler/src/service/attention.rs`
- `crates/daemon/src/server/attention.rs`
- `crates/cli/src/commands/attention.rs`
- `crates/gui/src/commands/attention.rs`
- `gui/src/pages/AttentionInbox.tsx`
