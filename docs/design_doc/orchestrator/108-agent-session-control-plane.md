# Orchestrator - Agent Session Control Plane

**Module**: orchestrator
**Status**: Approved
**Related Plan**: FR-098; daemon-authoritative session resources, fenced writer leases, resumable transcript streams, CLI/Tauri/process-console integration
**Related QA**: `docs/qa/orchestrator/145-agent-session-control-plane.md`; hardened closure in `docs/qa/orchestrator/149-agent-session-control-plane-hardening.md`
**Created**: 2026-07-12
**Last Updated**: 2026-07-14

## Background

TTY execution already persisted `agent_sessions`, FIFO paths, output paths, process metadata, and attachments. Those details were internal and did not form a safe public control plane. A GUI disconnect could leave a writer owner behind, PID reuse could make process-only checks unsafe, and clients had no offset-based transcript API.

## Goals

- Make `session_id` a first-class observable and controllable identity.
- Support bounded readers, one explicitly leased writer, fenced input, and governed close.
- Reconcile persisted state with process identity after daemon restart.
- Provide CLI, Tauri, global Session Inspector, and Process Workspace access without disclosing filesystem paths.

## Non-goals

- PID-addressed mutations, multi-writer terminal editing, cross-host PTY migration, Mailbox messaging, or reviving a dead provider process.
- A global process console; that belongs to FR-100.

## Scope

- In scope: migration 29, session repository/runtime ownership, nine gRPC methods, stream protection, CLI, Tauri, the embedded `SessionPanel`, audit events, policy flags, and reconciliation. FR-100 later adds the global list/inspector entry.
- Out of scope: browser terminal protocol optimization and arbitrary file transfer.

## UI Interactions

- Page: embedded TaskDetail implementation, presented by FR-100 as Process Workspace, plus the global Session Inspector.
- Visible entry: the "Agent session" panel appears when the task has sessions.
- Actions: "Request control", "Send", "Release control", and "Close session".
- The transcript viewer starts read-only and resumes from its last committed source offset.

## API

The gRPC service exposes `AgentSessionList`, `AgentSessionGet`, `AgentSessionAttach`, `AgentSessionHeartbeat`, `AgentSessionDetach`, `AgentSessionSendInput`, `AgentSessionRead` (server stream), `AgentSessionClose`, and `AgentSessionResolvePid`.

Public `AgentSession` values contain task relationships, state, diagnostic PID, lease summary, versions, and timestamps. They never contain `cwd`, command text, FIFO paths, transcript paths, stdout/stderr paths, or output JSON paths.

Read APIs and reader attach require `read_only`. Writer attach is dynamically elevated to `operator`; heartbeat, input, writer detach, and close also require `operator`. `session_read_enabled` defaults to true and `session_control_enabled` defaults to false.

## Database Changes

Migration 29 adds the following columns to `agent_sessions`: `state_version`, `writer_actor`, `writer_lease_expires_at`, `writer_last_heartbeat_at`, `writer_fencing_token`, and `process_fingerprint`. It also creates `session_control_actions` with a unique `(session_id, idempotency_key)` constraint and supporting indexes. Legacy `exited` rows migrate to `closed` without deleting sessions.

## Key Design

1. `session_id` is authoritative. PID is accepted only by `ResolvePid` and is never a mutation key.
2. A writer lease contains trusted actor, client instance, expiry, heartbeat, and a monotonically increasing fencing token. Every input validates the exact, unexpired tuple.
3. TTY children remain owned by a daemon task instead of being forgotten. A persisted process fingerprint uses Linux boot ID/start ticks or platform process start time to reject PID reuse.
4. State is `opening -> active -> detached -> draining -> closed`, with `failed` as the inconsistent/abnormal terminal state. Bootstrap reconciliation is fail-closed.
5. Transcript cursors are committed source-byte offsets. The server returns explicit `next_offset`, bounded chunks, cancellation, and EOF only after terminal state or non-follow reads.
6. FIFO and output paths remain server-private. The daemon validates, redacts, audits, and performs I/O. Session control rows project FR-101's canonical `request_id`; input audit evidence stores length and digest, never terminal bytes.

## Alternatives And Tradeoffs

- A dedicated PTY multiplexer would improve fidelity, but retaining the transport-neutral FIFO/output-backed API reduces migration risk.
- Immediate writer stealing would reduce takeover latency, but TTL plus fencing prevents stale reconnects from writing.
- Provider resume is intentionally separate: session control handles a live OS process; DD-107 handles logical continuation.

## Risks And Mitigations

- Risk: PID reuse targets another process. Mitigation: session identity plus creation fingerprint; unverifiable identity rejects mutation.
- Risk: reconnect races create two writers. Mitigation: atomic lease update and monotonic fencing.
- Risk: slow readers exhaust resources. Mitigation: bounded channels/chunks and shared stream occupancy protection.
- Risk: terminal content contains secrets. Mitigation: centralized redaction before persistence/egress and no content logging.

## Observability

- Events: `session_reader_attached`, `session_writer_acquired`, `session_input_accepted`, `session_close_requested`, and reconciliation/lease transitions.
- Safe fields: session ID, actor, client ID, byte count, fencing token, state, and outcome.
- Forbidden log fields: input bytes, transcript text, prompts, commands, and filesystem paths.
- Stream admission uses the existing control-plane traffic class and occupancy audit.

## Operations / Release

- Apply migration 29 before serving the new RPCs.
- Enable mutations explicitly with `RuntimePolicy.spec.session_control_enabled: true`; reads can be rolled back independently with `session_read_enabled`.
- Rollback disables both feature flags. The additive columns/table can remain for forward compatibility.
- Existing sessions remain listable; legacy `exited` projects as `closed`.

## Test Plan

- Unit: migration compatibility, process fingerprint, bounded readers, lease acquisition/heartbeat/release, stale fencing, and reconciliation.
- Integration: RPC roles, stream cursor/reconnect, path non-disclosure, close identity checks, and CLI output.
- UI: visible global/process entry, read-only default, explicit takeover, heartbeat, detach, and offset reconnect.

## QA Docs

- `docs/qa/orchestrator/145-agent-session-control-plane.md`
- `docs/qa/orchestrator/149-agent-session-control-plane-hardening.md` (FR-102 executable closure)
- `docs/qa/orchestrator/152-session-runtime-policy-authority.md` (FR-105 deterministic global policy authority)

## Hardening Closure

DD-112 preserves this public design while tightening restart reconciliation, per-session stream occupancy, atomic 4096-byte input, canonical/domain retry replay, process-incarnation checks, Tauri stream replacement, and committed-offset GUI reconnect. The provider-free acceptance entry point is `scripts/qa/test-agent-session-control-plane.sh`.

DD-115 fixes the rollout authority behind this design: global Session read/control gates resolve only the `_system` RuntimePolicy, while project policy remains relevant to project-scoped consumers such as transcript redaction. The public Session contract is unchanged.

## Acceptance Criteria

- Existing sessions survive migration and are visible through List/Get.
- Readers use independent offsets and reconnect without committed-byte duplication or loss.
- Only the current writer fencing token can input; stale tokens fail deterministically.
- Reader access works for `read_only`, while writer and close require `operator` plus policy.
- Restart reconciliation distinguishes live, detached, closed, failed, stale PID, and PID reuse cases.
