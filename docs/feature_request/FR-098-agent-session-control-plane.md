# FR-098: Agent Session Control Plane

## 优先级: P1

## 状态: Proposed

## 依赖: FR-095; integrates with FR-097

## 计划闭环产物

- `docs/design_doc/orchestrator/108-agent-session-control-plane.md`
- `docs/qa/orchestrator/145-agent-session-control-plane.md`

This request supersedes the proposed implementation ordering in DD-075.

## Background

TTY execution already persists `agent_sessions`, input FIFO paths, transcripts, output paths, process metadata, and reader/writer attachments. `SessionRow` and repository operations support task-scoped lookup, reader attachment, writer acquisition, and release. These capabilities are not yet a complete public control plane and are not exposed through gRPC or the GUI.

DD-075 proposed Mailbox and Session as separate abstractions. This request advances the Session half toward product implementation. Mailbox remains deferred until a concrete asynchronous agent-to-agent use case requires it.

## Goals

- Make sessions first-class observable resources addressed by `session_id`.
- Support list/get, transcript read/follow, reader attach, explicit writer lease, send input, detach, and close.
- Preserve single-writer semantics and allow multiple bounded readers.
- Recover cleanly from GUI disconnects, daemon restarts, stale PIDs, and exited child processes.
- Integrate session state and actions into process timeline and task detail.

## Non-goals

- Addressing mutating operations by PID.
- Multi-writer collaborative terminal editing.
- Cross-host PTY migration.
- General mailbox messaging.
- Guaranteeing that a dead process can be revived; provider resume is handled by FR-097.

## Scope

### In scope

- Session service/repository extensions, gRPC streams, CLI commands, Tauri bridge, and React session views.
- Transcript offset cursors, cancellation, backpressure, lease TTL/heartbeat, close policy, audit, and redaction.
- Reconciliation of persisted state with live process state.

### Out of scope

- Browser terminal emulation protocol optimization.
- Distributed lease coordination beyond one daemon authority.
- Arbitrary file transfer through the session channel.

## Interfaces and Data Changes

### Session states

The canonical state model is:

```text
opening -> active -> detached -> draining -> closed
                   \-> failed
```

Persisted legacy `exited` maps to `closed`; legacy active/detached rows remain readable. A migration may add `state_version`, `lease_expires_at`, and `last_heartbeat_at` while preserving existing columns.

### Identity and lease rules

- `session_id` is the authoritative identity.
- `pid` is diagnostic and may be returned or used only by a resolve/read query.
- Attach defaults to reader mode.
- Writer mode requires an explicit request and operator role.
- A writer lease has owner identity, client instance ID, expiry, heartbeat, and monotonically increasing fencing token.
- Send-input requires the current fencing token.

### gRPC

```proto
rpc AgentSessionList(AgentSessionListRequest) returns (AgentSessionListResponse);
rpc AgentSessionGet(AgentSessionGetRequest) returns (AgentSessionGetResponse);
rpc AgentSessionAttach(AgentSessionAttachRequest) returns (AgentSessionAttachResponse);
rpc AgentSessionHeartbeat(AgentSessionHeartbeatRequest) returns (AgentSessionHeartbeatResponse);
rpc AgentSessionDetach(AgentSessionDetachRequest) returns (AgentSessionDetachResponse);
rpc AgentSessionSendInput(AgentSessionSendInputRequest) returns (AgentSessionSendInputResponse);
rpc AgentSessionRead(AgentSessionReadRequest) returns (stream AgentSessionOutputChunk);
rpc AgentSessionClose(AgentSessionCloseRequest) returns (AgentSessionCloseResponse);
rpc AgentSessionResolvePid(AgentSessionResolvePidRequest) returns (AgentSessionResolvePidResponse);
```

Transcript chunks include `session_id`, offset, timestamp when available, stream kind, redacted bytes/text, and EOF. Clients reconnect using the last committed offset.

### CLI

```text
orchestrator agent session list [--task ID] [--agent ID] [--state STATE]
orchestrator agent session get SESSION_ID
orchestrator agent session attach SESSION_ID [--mode reader|writer]
orchestrator agent session read SESSION_ID [--follow] [--offset N]
orchestrator agent session send-input SESSION_ID --text TEXT --fencing-token TOKEN
orchestrator agent session detach SESSION_ID
orchestrator agent session close SESSION_ID --reason TEXT
orchestrator agent session resolve --pid PID
```

## Key Design

### Daemon-authoritative I/O

Clients never receive FIFO or transcript filesystem paths for direct access. The daemon validates state and role, reads/writes the underlying transport, applies redaction, records audit events, and returns bounded chunks.

### Lease fencing

Clearing `writer_client_id` alone is insufficient during reconnect races. A fencing token prevents a stale client from writing after another client acquires the lease. Lease heartbeats use a bounded TTL; graceful detach releases immediately.

### Reconciliation

At startup and periodically, the daemon reconciles sessions:

- live PID/process plus readable transport -> active or detached;
- dead process with completed transcript -> closed;
- missing process/transport or inconsistent metadata -> failed;
- expired writer lease -> release and emit audit event.

PID reuse cannot make a session live because process identity checks include creation metadata or an owned child/runtime registry when available.

### Stream limits

Session reads use the same control-plane stream occupancy protection as task-follow streams. Each client has bounded buffers, maximum chunk size, cancellation, idle timeout, and per-subject stream limits.

## Tradeoffs

- FIFO/transcript-backed control reuses current implementation but is less capable than a dedicated PTY multiplexer. The API remains transport-neutral so the backend can evolve.
- Lease TTL may briefly delay takeover after a client crash; fencing is safer than immediate silent stealing.
- Redaction can alter terminal fidelity. Safety takes priority, and authorized raw local filesystem access remains outside this API.

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| Stale writer sends input after takeover | Fencing token on every write |
| PID reuse targets the wrong process | Session identity plus process reconciliation; PID never authorizes writes |
| Transcript stream exhausts resources | Bounded chunks, occupancy limits, cancellation, and offsets |
| Input exposes shell-level authority | Operator RBAC, explicit writer request, audit, and session policy |
| Secrets appear in transcript | Central redaction before egress and retention policy |
| GUI disconnect leaves lease stuck | TTL heartbeat and graceful detach |

## Observability and Operations

- Metrics: active/detached/failed sessions, reader streams, writer lease acquisition/conflict/expiry, input bytes, transcript lag, reconciliation failures, and stream occupancy rejections.
- Audit events: attach, lease acquire/deny/expire, input accepted/rejected, detach, close, and reconciliation state change.
- Session IDs may be logged; input and transcript contents must not be logged.
- Feature flags separate read APIs from mutating writer controls.
- Cleanup must retain active/detached sessions and apply TTL only to terminal states.

## Testing and Acceptance

Detailed QA will be created at `docs/qa/orchestrator/145-agent-session-control-plane.md` after implementation is approved.

Acceptance criteria:

- [ ] Existing persisted sessions are visible through list/get without migration loss.
- [ ] Multiple readers can follow from independent offsets.
- [ ] Only one current fencing token can send input; stale tokens fail deterministically.
- [ ] Reader attach is available to read-only roles; writer and close require operator policy.
- [ ] Daemon restart reconciles active, detached, closed, and failed states correctly.
- [ ] A stale or reused PID cannot authorize attach, input, or close.
- [ ] GUI disconnect and reconnect resume transcript streaming without duplicate or missing committed chunks.
