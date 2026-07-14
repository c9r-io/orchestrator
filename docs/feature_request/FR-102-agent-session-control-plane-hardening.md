# FR-102: Agent Session Control Plane Hardening And Acceptance

## 优先级: P1

## 状态: Proposed

## 依赖: FR-098 closure artifacts, FR-101 audit envelope

## 计划闭环产物

- `docs/design_doc/orchestrator/112-agent-session-control-plane-hardening.md`
- `docs/qa/orchestrator/149-agent-session-control-plane-hardening.md`
- `scripts/qa/test-agent-session-control-plane.sh`

## Background

Migration 29, nine `AgentSession*` RPCs, the CLI/Tauri surfaces, writer fencing, transcript reads, process fingerprints, and Session UI already exist. Core repository tests pass, but QA-145 has no completed scenario. There is no reproducible isolated-daemon acceptance script proving migration compatibility, independent stream offsets, real input idempotency, restart reconciliation, PID reuse defense, and GUI re-entry as one governed slice.

This FR hardens and verifies the existing implementation. It should fix defects exposed by deterministic acceptance tests, but it does not redesign the session abstraction.

## Goals

- Turn the five unverified QA-145 scenarios into reproducible automated evidence.
- Verify reader offset semantics and bounded backpressure against real transcript files and gRPC streams.
- Verify writer acquisition, heartbeat, fencing, idempotent input, detach, and close under races and retries.
- Prove restart reconciliation and PID-reuse defense using controlled subprocess fixtures.
- Verify read-only/operator policy behavior through CLI, Tauri, and the visible Session Inspector/Process Workspace surfaces.
- Ensure session input and transcript content never enter logs, audit payloads, or error messages.

## Non-goals

- Supporting arbitrary remote shells or browser-hosted terminals.
- Adding multi-writer collaboration.
- Depending on a live Claude/Codex/Gemini process for tests.
- Changing provider resume behavior outside the existing opaque session adapter.
- Implementing cross-process product metrics, which belongs to FR-104.

## Scope

### In scope

- Populated legacy database fixture covering `active`, `detached`, and `exited` rows before migration 29.
- Deterministic fake agent process with FIFO/input capture, transcript append, stable process fingerprint, and controlled termination.
- Concurrent readers, reconnect offsets, stream cancellation, reader limit, and terminal EOF behavior.
- Writer lease conflict, heartbeat renewal, expiry, monotonically increasing fencing tokens, stale-token rejection, and identical-input replay.
- Close transition, signal safety, PID reuse/mismatched fingerprint, expired lease cleanup, and daemon restart convergence.
- CLI and Tauri error semantics plus Session Inspector and embedded session-panel behavior.

### Out of scope

- Performance at thousands of simultaneous sessions.
- Terminal emulation, ANSI rendering, file upload, or port forwarding.
- Provider-specific conversation APIs.

## Interfaces And Data Changes

No new public capability is required unless testing exposes an unobservable invariant. Additive diagnostic fields may be introduced for stable error codes, reconciliation result, or stream limits. Any new mutation audit fields must use FR-101 rather than creating another session-only envelope.

The deterministic fixture must run under a temporary `ORCHESTRATORD_DATA_DIR`, temporary HOME, non-default 19xxx port, and disposable workspace. It must never signal or inspect the active daemon process.

## Key Design Constraints

- `session_id` plus verified process fingerprint is authoritative; PID alone is diagnostic.
- Reader offsets are client-owned and never stored as one shared session cursor.
- Only the current unexpired fencing token may write, detach a writer, or renew its lease.
- Input idempotency is checked before FIFO write and retries report the original accepted byte count.
- Restart reconciliation distinguishes live transport, live-without-writer, dead-with-evidence, and inconsistent identity.
- Read-only UI contains no hidden focusable writer/input/close controls.

## Acceptance Criteria

- [ ] A populated pre-migration database upgrades to version 29 without losing sessions; public responses expose no paths, command line, or FIFO details.
- [ ] Two readers consume independent offsets, reconnect without duplicate committed bytes, respect the reader bound, and release stream occupancy on disconnect.
- [ ] Writer races grant exactly one lease; heartbeat renews it; a later writer receives a greater token; stale tokens perform no input or detach.
- [ ] Retrying identical input with one idempotency key writes exactly once; conflicting input with the same key fails closed.
- [ ] Read-only can list/get/read/attach as reader, while writer/input/close require operator plus `session_control_enabled`.
- [ ] PID reuse or mismatched fingerprint prevents input and signaling even when the numeric PID is live.
- [ ] Daemon restart reconciles active, detached, closed, failed, draining, and expired-lease fixtures deterministically.
- [ ] Session Inspector and Process Workspace reconnect from the last committed offset and expose accurate reader/writer state.
- [ ] QA-145 is superseded or updated with all five scenarios marked PASS from executable evidence.

## QA Plan

- Extend repository and daemon tests for reconciliation, stream offsets, input reservation recovery, close failures, and races.
- Run an isolated daemon with a shell fixture that appends transcript output and records input bytes.
- Exercise every public CLI command and the Tauri bridge without a live AI provider.
- Add browser coverage for read-only focusability, writer acquisition, reconnect, lease expiry, and process-link navigation.

## Risks And Mitigations

| Risk | Mitigation |
|---|---|
| Test process is mistaken for a real session authority | Disposable PID/fingerprint fixture under isolated data and workspace roots |
| FIFO timing makes tests flaky | Readiness handshake, bounded polling, explicit timeouts, no fixed long sleeps |
| Restart tests affect the development daemon | Dedicated port, HOME, data directory, PID file, and cleanup trap |
| Platform process fingerprints differ | Platform adapter with deterministic supported/unsupported assertions |
| Input content leaks during failure diagnostics | Record hashes/byte counts only and scan logs/test reports for fixture secrets |
