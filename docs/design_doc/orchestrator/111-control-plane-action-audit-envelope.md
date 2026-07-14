# Orchestrator - Canonical Control-Plane Action Audit Envelope

**Module**: Orchestrator  
**Status**: Approved  
**Related Plan**: FR-101 canonical mutation envelope, request correlation, idempotency enforcement, and bounded query surface  
**Related QA**: `docs/qa/orchestrator/148-control-plane-action-audit-envelope.md`  
**Created**: 2026-07-14  
**Last Updated**: 2026-07-14

## Background

Attention, handoff/resume, agent-session control, and external-source mutations originally wrote separate audit shapes. Transport authorization was recorded independently in `control_plane_audit`, so an operator could not reliably join the authenticated request, domain mutation, task event, and terminal result. Several legacy requests also lacked a shared reason and retry contract.

## Goals

- Make one bounded, versioned action envelope the durable source of truth for every process-console mutation.
- Correlate authorization, canonical audit, domain evidence, and emitted events with a daemon-issued request ID.
- Enforce trusted actor/role resolution, closed actions, reason codes, retry identity, optimistic version or fencing context, and stable terminal outcomes.
- Keep CLI and GUI failures diagnosable without exposing request bodies, prompts, terminal input, source payloads, handoff contents, or secrets.
- Preserve additive protobuf compatibility while projects move from compatibility mode to enforcement.

## Non-goals

- Replacing mTLS, UDS peer authentication, or existing RBAC.
- Replacing domain tables or storing request/response bodies.
- Adding a dashboard or hosted log aggregation.
- Making client-provided actor or role fields authoritative.

## Scope

- In scope: Attention claim/snooze/resolve/advertised actions; handoff generation and resume plan/execute; session writer attach/heartbeat/detach/input/close; source ingest/bind/replay and provider-originated closed commands.
- In scope: UDS and secure TCP request-ID propagation, canonical persistence, domain/event projections, gRPC/CLI list/get, GUI/CLI error correlation, migration 31, and `RuntimePolicy.spec.action_audit_mode`.
- Out of scope: read-only RPC auditing beyond existing transport policy, historical backfill, product metrics, and retention automation.

## API

- Mutation requests receive an additive optional `ActionAuditContext`:
  - `reason_code`: closed machine-readable reason, 1-64 characters.
  - `operator_reason`: optional bounded explanation, at most 500 bytes.
  - `idempotency_key`: required for business mutations in enforced mode.
- `x-request-id` may be propagated by a caller only when it matches the validated 1-128 character format. Otherwise the daemon issues `req-{uuid}`. Trusted actor and resolved role always come from transport authentication.
- `ActionAuditList` and `ActionAuditGet` are read-only gRPC methods. Both require `project_id`; list additionally filters by actor, target type/ID, action, status, and time range with a maximum of 500 records.
- CLI:
  - `orchestrator audit list --project {project} [filters]`
  - `orchestrator audit get {request_id} --project {project}`
- Mutation responses and errors return `x-request-id`. CLI errors render `request_id: ...`; GUI errors retain the same ID in the humanized message.

## Database Changes

Migration 31 creates `control_action_audit` with:

- identity/scope: `request_id`, `schema_version`, `project_id`;
- authority: `actor`, `resolved_role`, `transport`;
- operation: `target_type`, `target_id`, closed `action`, `reason_code`, optional `operator_reason`;
- concurrency: optional `idempotency_key`, `expected_version`, and `fencing_token`;
- bounded evidence: SHA-256 `request_hash`, terminal `status`, stable `error_code`, optional result type/ID, and timestamps.

The table is the durable source of truth. `control_plane_audit`, `attention_actions`, `resume_executions`, `session_control_actions`, `source_command_actions`, `source_events`, `source_bindings`, and `events` receive an additive `request_id` projection. Direct failed/denied attempts retain the attempted idempotency key but are excluded from active/succeeded retry-identity uniqueness, so an error record cannot block a later valid retry.

Migration is forward-only and preserves populated databases. Rollback disables enforcement and deploys the previous binary; migration 31 remains in place.

## Key Design

1. A handler installs the request ID and reserves the canonical envelope before business side effects. Authorization denial is inserted directly as terminal `denied`; authorized attempts transition `reserved` to `succeeded` or `failed` after the durable outcome.
2. Retry uniqueness is `(project_id, target_type, target_id, action, idempotency_key)`. A matching duplicate receives the original envelope and executes no second side effect. Reusing the key with a different canonical hash fails closed.
3. Canonical hashes cover only allowlisted non-secret fields. Session input records length and a digest; source and handoff bodies remain outside the envelope.
4. Heartbeat is the explicit renewable exemption: it may omit an idempotency key but still records request ID, session target, reason code, expected state version, and fencing token.
5. Provider commands use a deterministic request ID, adapter transport label, locally resolved Trigger role, and the same common repository before their existing domain audit.
6. Task/source events include `request_id` when an event exists. The event writer promotes it into `events.request_id` for indexed joins.

## Alternatives And Tradeoffs

- Query a union over domain audit tables: avoids a new table, but preserves incompatible semantics and makes authorization/result joins unreliable.
- Store complete request/response JSON: simplifies replay debugging, but leaks high-risk content and creates unbounded audit growth.
- Require the new context immediately: simplest server logic, but breaks older clients. A project-scoped transition mode provides controlled rollout.
- Treat each duplicate as a new canonical row: preserves every attempt, but weakens the single retry identity. Transport decisions still record individual attempts; the canonical row represents the governed business action.

## Risks And Mitigations

- Audit and domain projection diverge after an unexpected database failure.
  - Canonical reservation occurs first, terminal failure is retained, and every projection carries the request ID for reconciliation.
- Audit traffic increases write latency and heartbeat volume.
  - Rows are bounded and indexed; heartbeat has an explicit exemption rather than an unbounded payload.
- Client-supplied metadata impersonates an actor.
  - Only the request ID is accepted after validation; actor and role are transport-derived.
- Legacy clients silently bypass required semantics.
  - Compatibility mode generates `legacy_client` and request-scoped retry identity; enforced mode rejects missing context before mutation.

## Observability

- `request_id` is the primary join key across `control_plane_audit`, `control_action_audit`, domain projections, task events, structured logs, CLI errors, and GUI errors.
- `orchestrator audit list --project {project} --status failed` is the bounded operational failure view; `audit get` reconstructs one action without bodies.
- Stable statuses are `reserved`, `succeeded`, `failed`, and `denied`. Stable errors distinguish authorization denial, stale/version conflict, fencing rejection, invalid input, and side-effect availability.
- Aggregate product metrics and dashboards remain deferred to FR-104; request IDs must not become aggregate metric labels.

## Operations / Release

- `RuntimePolicy.spec.action_audit_mode` accepts `compatibility` (default) or `enforced`.
- Roll out upgraded CLI/GUI clients while compatibility mode is active. Inspect `reason_code=legacy_client`, then switch each project to enforced mode after legacy traffic reaches zero.
- To stop enforcement without schema rollback, apply `action_audit_mode: compatibility`. Do not drop migration 31 tables or columns.
- The canonical protobuf is `crates/proto/orchestrator.proto`; additions are optional and wire-compatible with older clients.

## Test Plan

- Unit: validation, canonical key ordering, matching/conflicting retries, concurrent reservation, denial retry isolation, redaction, request-ID validation, heartbeat exemption, RBAC mapping, and CLI parsing/error rendering.
- Migration: upgrade a populated version-30 database and verify all link columns while preserving data.
- Integration: exercise one mutation per domain plus provider-originated commands and verify canonical/domain/event correlation.
- E2E: `scripts/qa/test-control-plane-action-audit.sh` runs an isolated daemon and validates success, stale failure, duplicate/conflict, denial evidence, query filters, and bounded output.

## QA Docs

- `docs/qa/orchestrator/148-control-plane-action-audit-envelope.md`

## Acceptance Criteria

- Every in-scope mutation has a documented trusted actor, reason, retry identity or explicit renewable exemption, target, request ID, and result contract.
- One request ID joins transport, canonical, domain, and event evidence where the latter exist.
- Matching duplicates execute once; changed requests sharing a retry key fail before side effects.
- UDS and secure TCP produce equivalent semantics without trusting client actor/role fields.
- Project-scoped list/get exposes bounded envelope fields and no secret-bearing body.
- Compatibility and enforcement transitions are documented and tested.
- Populated migration, rollback-disable behavior, RBAC, redaction, and concurrency checks pass.
