---
lifecycle: active
related_fr: FR-102
---

# Orchestrator - Agent Session Control Plane Hardening

**Module**: Orchestrator  
**Status**: Approved  
**Related Plan**: FR-102 session lifecycle, stream, fencing, process-identity, and UI acceptance hardening  
**Related QA**: `docs/qa/orchestrator/149-agent-session-control-plane-hardening.md`  
**Created**: 2026-07-14  
**Last Updated**: 2026-07-15

## Background

DD-108 introduced daemon-authoritative agent sessions, but its initial QA document contained five unexecuted scenarios. The implementation also left several acceptance risks at layer boundaries: restart reconciliation could not distinguish a dead process from a live PID with the wrong incarnation, action-audit replay could intercept an accepted input retry before the domain idempotency record returned its byte count, GUI stream replacement could leave an earlier subscription active, and the deterministic FIFO fixture exited after its first writer disconnected.

FR-102 closes those gaps without changing the public session abstraction. The hardened contract remains one session identity, independent reader offsets, one fenced writer, bounded input, verified process signaling, and role-gated UI controls.

## Goals

- Preserve populated session rows through migration 29-31 and reconcile every non-terminal state deterministically.
- Bound transcript streams per session and make source-byte offsets independently resumable by every reader.
- Guarantee one atomic FIFO write for one accepted idempotency identity, including identical retry responses.
- Reject stale fencing tokens and live PID/fingerprint mismatches before input, detach, or signaling.
- Resume Session Inspector streams from the last committed offset and remove mutation controls for read-only users.
- Provide one isolated, provider-free acceptance script that joins runtime, database, audit, RBAC, redaction, and restart evidence.

## Non-goals

- Terminal emulation, ANSI interpretation, multi-writer collaboration, remote shells, or file transfer.
- Provider-specific conversation resume; logical re-entry remains owned by DD-107.
- Cross-process product metrics and dashboards; those remain owned by FR-104.
- Keeping an orchestrator-owned child alive after daemon runtime teardown. Restart reconciliation is validated with a controlled external process fixture.

## Scope

- In scope: `core/src/session_store.rs`, migration regression fixtures, session gRPC handlers, CLI structured chunk output, Tauri stream ownership, React Session Inspector behavior, browser E2E, and isolated-daemon QA.
- In scope: active, detached, draining, closed, failed, expired-writer, dead-process, and mismatched-fingerprint convergence.
- Out of scope: changes to the nine existing `AgentSession*` RPC identities or public path disclosure policy.

## Interfaces And Data Changes

- No protobuf method was added. `orchestrator agent session read --chunks-json` is an additive CLI rendering mode that emits JSONL records with `offset`, `next_offset`, `stream`, `text`, `eof`, and `redacted`.
- Each daemon holds at most eight concurrent transcript stream permits per session. A permit lives until its stream task ends or the client disconnects.
- Session input is limited to 4096 bytes and written with one FIFO `write`; partial or unavailable transport is terminal failure for that attempt.
- An accepted duplicate `(session_id, idempotency_key, input digest)` returns the original `accepted_bytes` without another FIFO write. A different digest with the same key fails closed.
- Migration 31's `request_id` projection remains the action correlation key; no input or transcript body is stored in canonical audit evidence.

## Key Design

1. Process authority is the tuple of `session_id`, live process existence, and process-creation fingerprint. Signal zero proves only existence; a mismatched or unavailable fingerprint never grants mutation authority.
2. Restart reconciliation maps a verified live process plus transport to `active`, `detached`, or existing `draining`; dead processes with evidence become `closed`; inconsistent live identity or transport becomes `failed`. Lease expiry clears writer ownership without resurrecting a terminal session.
3. Reader state is client-owned. The server reads bounded chunks from the requested source-byte offset, and the UI commits `next_offset` only after appending a new chunk. Reconnect uses the committed per-session offset and ignores repeated chunks.
4. Writer acquisition is an atomic database update. Every successful acquisition increments `writer_fencing_token`; heartbeat, input, and writer detach require the exact unexpired tuple.
5. Input uses two coordinated retry records: FR-101's canonical action envelope and `session_control_actions`. Canonical matching replay consults the accepted domain reservation so the client receives the original byte count without re-execution.
6. Tauri replaces an existing stream token by cancelling it before registration. React maintains transcript and offset maps keyed by `session_id`, cancels timers/listeners on selection change, and retries from the committed offset.

## Alternatives And Tradeoffs

- A PTY multiplexer would avoid FIFO reopen behavior but would expand the transport and lifecycle scope. The hardened fixture keeps the current FIFO abstraction and re-enters reads after EOF.
- Persisting a shared reader cursor would simplify reconnect, but would couple readers and allow one client to advance another. Client-owned offsets remain explicit.
- Retrying failed transport writes automatically inside the daemon could hide uncertain delivery. A failed reservation is retryable through compare-and-swap, while an in-progress reservation remains fail-closed.
- Immediate writer takeover would improve convenience but weaken stale-client safety. Explicit detach or lease expiry remains required.

## Risks And Mitigations

- PID reuse could signal an unrelated process. Mutation and close require a matching creation fingerprint, and QA keeps the numeric PID live while injecting a mismatch.
- Slow or abandoned readers could exhaust tasks. Per-session semaphore permits, bounded channels, chunk caps, and cancellation release occupancy.
- Input retry could duplicate terminal bytes. Domain reservation precedes the single atomic write; accepted replay returns metadata only.
- GUI reconnect could duplicate text or leak subscriptions. Offsets and transcript buffers are session-keyed, repeated `next_offset` values are ignored, and prior Tauri tokens are cancelled.
- QA could affect a developer daemon. The script uses temporary HOME/data/workspace roots, a non-default port, disposable processes, bounded polling, and a cleanup trap.

## Observability

- Safe evidence includes session ID, trusted actor, client ID, byte count, digest, fencing token, state, request ID, and terminal result.
- `session_control_actions.request_id` joins the FR-101 canonical envelope; authorization denials remain visible with transport-derived role.
- Forbidden evidence includes terminal input, transcript text, prompt text, command lines, FIFO/output paths, and process fingerprints in public responses.
- `scripts/qa/test-agent-session-control-plane.sh` scans daemon and audit output for deterministic input markers.

## Operations / Release

- Keep `RuntimePolicy.spec.session_read_enabled: true` for observation and explicitly enable `session_control_enabled` in the `_system` RuntimePolicy for mutation rollout.
- Rollback first disables `session_control_enabled`; the additive migration columns and audit rows remain compatible.
- Run `cargo build -p orchestratord -p orchestrator-cli` before isolated QA. The script defaults to `127.0.0.1:19102` and accepts `BIND_ADDR`, `ORCH`, and `ORCHD` overrides.
- `KEEP_QA=1` retains isolated artifacts for diagnosis. `SKIP_TARGETED_TESTS=1` is a local iteration aid; closure evidence uses the default path.

## Test Plan

- Unit: populated v28 migration, PID incarnation checks, state reconciliation, terminal-safe lease cleanup, reader permit release, chunk offsets, 4096-byte boundary, heartbeat, and concurrent writer race.
- Integration: isolated TCP and read-only UDS daemons, every control path, policy disable/enable, audit joins, PID mismatch, and external-process restart reconciliation.
- UI: Vitest plus Playwright coverage for visible Sessions navigation, offset replay deduplication, writer lifecycle, process linking, and absent read-only mutation controls.
- Regression: `cargo test --workspace`, strict workspace Clippy, GUI tests/build, QA doc lint, and the isolated script.

## QA Docs

- `docs/qa/orchestrator/149-agent-session-control-plane-hardening.md`
- `docs/qa/orchestrator/152-session-runtime-policy-authority.md` supersedes QA-149 only for deterministic `_system` rollout/rollback authority.
- Supersedes the unexecuted status in `docs/qa/orchestrator/145-agent-session-control-plane.md` while retaining that document as the original FR-098 specification.

## Acceptance Criteria

- Populated sessions migrate without loss, legacy terminal state is canonicalized, and public output contains no authority paths.
- Independent bounded readers resume from committed offsets and release occupancy.
- Exactly one writer wins; tokens increase; stale ownership cannot write or detach.
- Identical input retry writes once and returns the original byte count; conflicting reuse fails closed.
- Read-only observation succeeds while writer/input/close require operator and the global feature flag.
- Live PID mismatch cannot authorize input or signaling.
- Restart and lease reconciliation converge every supported state without terminal resurrection.
- Session Inspector reconnect and read-only focusability are verified in browser E2E.
- The default isolated QA script reports five passes and zero failures.

## Follow-up

DD-115 makes `_system` the deterministic authority for the global Session read/control flags and extends the same isolated script with conflict-order, hot-apply, and restart assertions. All lifecycle, fencing, process-identity, RBAC, UI, audit, and redaction contracts in this document remain unchanged.
