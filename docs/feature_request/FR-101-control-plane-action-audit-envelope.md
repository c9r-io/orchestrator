# FR-101: Canonical Control-Plane Action Audit Envelope

## 优先级: P0

## 状态: Proposed

## 依赖: FR-096, FR-097, FR-098, FR-099 closure artifacts

## 计划闭环产物

- `docs/design_doc/orchestrator/111-control-plane-action-audit-envelope.md`
- `docs/qa/orchestrator/148-control-plane-action-audit-envelope.md`
- `scripts/qa/test-control-plane-action-audit.sh`

## Background

The Agent Process Console introduced several state-changing domains: Attention actions, handoff generation and resume execution, session writer control, and source binding/replay. Each domain currently records some combination of actor, reason, idempotency key, request hash, and result, while the transport authorization audit separately records RPC access. The records cannot be joined reliably because domain tables do not consistently retain the daemon request ID, and several mutation requests do not carry a reason or idempotency contract.

The roadmap requires one governed operator-action contract. Without it, an operator can see that an RPC was authorized or that a domain row changed, but cannot always reconstruct one complete chain from request through policy decision to durable result.

## Goals

- Define one canonical action-audit envelope shared by Attention, handoff/resume, session control, and source mutations.
- Correlate transport authorization, domain mutation, emitted task event, and final result with one daemon-issued request ID.
- Require trusted actor identity, target, action, reason semantics, retry identity, expected version/fencing context, status, and stable error code where applicable.
- Preserve domain-specific audit tables while providing a common query surface.
- Keep audit payloads bounded and exclude prompts, transcript/input bytes, source bodies, secrets, and handoff content.
- Maintain additive gRPC compatibility with older CLI and GUI clients during rollout.

## Non-goals

- Replacing the existing control-plane authentication or RBAC system.
- Storing request/response bodies in the audit log.
- Making the client authoritative for actor or request ID.
- Requiring a user-written prose reason for mechanical lease heartbeats; such operations still need a documented reason code and deterministic correlation identity.
- Building product dashboards, which belong to FR-104.

## Scope

### In scope

- Attention claim, snooze, resolve, and advertised actions.
- Handoff generation, resume planning, and resume execution.
- Session writer attach, heartbeat, detach, input, and close.
- Source ingest, bind, replay, and provider-originated closed commands.
- Request metadata propagation across mTLS, UDS, gRPC handlers, service methods, persistence, and structured events.
- CLI queries for audit records by request ID, actor, target, action, project, status, and time range.

### Out of scope

- Read-only RPC auditing beyond the existing optional policy.
- Hosted log aggregation or cross-installation analytics.
- Historical reconstruction of fields that were never stored before this migration.

## Interfaces And Data Changes

Introduce a versioned `ActionAuditEnvelope` with at least:

- `request_id`, `schema_version`, `project_id`;
- trusted `actor` and resolved role;
- `target_type`, `target_id`, and closed `action` identifier;
- `reason_code` and optional bounded operator reason;
- optional `idempotency_key`, expected state version, and fencing token hash/number;
- request hash over non-secret canonical inputs;
- `status`, stable `error_code`, result reference, and timestamps.

The daemon generates or normalizes `request_id`; callers may propagate one only through validated metadata. Business mutations require a non-empty idempotency key and reason. Lease renewal and other repeatable mechanical operations must define an explicit idempotency exemption using request ID plus fencing/version semantics rather than silently omitting retry behavior.

Persistence may use an additive common `control_action_audit` table or an indexed projection over enriched domain tables. The design must select one durable source of truth and document how existing `attention_actions`, `resume_executions`, `session_control_actions`, and `source_command_actions` reference it.

## Key Design Constraints

- The daemon remains the authority; clients cannot assert role or trusted actor.
- Audit reservation occurs before side effects and terminal status is recorded after the durable outcome.
- Reusing an idempotency key with a different canonical request fails before mutation.
- Failed authorization, stale version, fencing rejection, policy denial, and successful mutation all remain distinguishable.
- Request IDs returned in GUI/CLI errors match the durable audit record.
- Migrations are forward-only and may leave existing domain tables intact during rollback.

## Acceptance Criteria

- [ ] Every in-scope mutation has a documented actor, reason/reason-code, retry identity, target, request ID, and result contract, including explicit exemptions for renewable lease operations.
- [ ] One request ID joins transport authorization, the canonical audit record, domain action row, and emitted task/source event where an event exists.
- [ ] Duplicate matching requests perform no second side effect; changed requests with the same idempotency key fail closed.
- [ ] UDS and mTLS calls produce equivalent audit semantics without trusting user-supplied actor fields.
- [ ] Audit list/get CLI and gRPC queries support project scoping and do not expose secret-bearing request bodies.
- [ ] Existing clients remain readable during rollout, and mutation enforcement has a documented compatibility transition.
- [ ] Populated-database migration, rollback-disable behavior, RBAC, redaction, and concurrency tests pass.

## QA Plan

- Unit tests for envelope validation, canonical hashing, idempotency conflict, redaction, and request-ID propagation.
- Integration tests covering one mutation from each domain over isolated UDS and mTLS-equivalent authenticated contexts.
- An isolated daemon script verifies success, denial, stale-version, duplicate, and conflicting-key records without touching the active database.
- Cross-check GUI error request IDs against `orchestrator audit get {request_id}`.

## Risks And Mitigations

| Risk | Mitigation |
|---|---|
| Additive audit writes increase mutation latency | One bounded transaction and indexed low-cardinality fields |
| Client compatibility breaks when fields become required | Optional proto additions, server-generated defaults, announced enforcement phase |
| Audit logs leak terminal input or source content | Closed field allowlist, hashes/references only, redaction tests |
| Domain and common records diverge | Shared service writes both in one transaction or projects from one durable row |
| Heartbeat volume creates audit noise | Explicit renewable-action policy, sampling/compaction without losing lease ownership transitions |
