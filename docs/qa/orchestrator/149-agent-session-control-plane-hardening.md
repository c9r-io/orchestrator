---
self_referential_safe: true
---

# Orchestrator - Agent Session Control Plane Hardening

**Module**: Orchestrator  
**Scope**: Migration, stream ownership, fenced input, process identity, restart convergence, RBAC, audit redaction, and Session Inspector re-entry  
**Scenarios**: 5  
**Priority**: High

---

## Background

This document is the executable closure evidence for FR-102 and supersedes the unchecked execution status in QA-145. Use the deterministic shell agent only; do not attach to a live AI provider or a developer daemon.

```bash
cargo build -p orchestratord -p orchestrator-cli
./scripts/qa/test-agent-session-control-plane.sh
cd gui && npm test && npm run test:e2e && npm run build
```

The isolated script uses temporary HOME/data/workspace roots, `127.0.0.1:19102`, a cleanup trap, a controlled FIFO fixture, and a separate restart process.

## Database Schema Reference

| Table | Purpose |
|---|---|
| `agent_sessions` | Canonical lifecycle, process fingerprint, writer lease, fencing token, and state version |
| `session_attachments` | Idempotent reader/writer attachment lifecycle |
| `session_control_actions` | Domain input/close/lease reservation and FR-101 `request_id` projection |
| `control_action_audit` | Bounded canonical action envelope without input or transcript bodies |

---

## Scenario 1: Populated Migration And Deterministic Reconciliation

### Preconditions

- Debug binaries are built.
- No production or developer data directory is exported to the test.

### Goal

Verify populated v28 session rows survive migrations 29-31 and lifecycle reconciliation distinguishes dead, live, mismatched, terminal, and expired-writer states.

### Steps

1. Run the default isolated QA script; do not set `SKIP_TARGETED_TESTS`.
2. Confirm it runs `populated_v28_sessions_upgrade_without_loss_or_state_ambiguity`.
3. Confirm reconciliation, writer race, and terminal-safe expired-lease tests pass.
4. Inspect `agent session list|get -o json` evidence for internal paths or process fingerprints.

### Expected

- Active, detached, and legacy exited rows survive; exited becomes closed.
- Live verified transport converges to active/detached/draining; dead with evidence becomes closed; inconsistent identity becomes failed.
- Expired ownership is cleared without changing closed/failed sessions to detached.
- Public JSON contains no `cwd`, command, FIFO, transcript/output path, or process fingerprint.

### Expected Data State

```sql
SELECT MAX(version) FROM schema_migrations;
-- Expected: 31

SELECT id, state, state_version FROM agent_sessions ORDER BY id;
-- Expected: every seeded id retained; no ambiguous legacy exited state
```

---

## Scenario 2: Independent Bounded Readers And UI Offset Re-entry

### Preconditions

- Apply `fixtures/manifests/bundles/session-control-mock.yaml` in the isolated project.
- A TTY session has been materialized.

### Goal

Verify reader attachments are idempotent, offsets are client-owned, stream occupancy is released, and the Session Inspector reconnects without duplicate committed bytes.

### Steps

1. Attach `reader-a` twice and `reader-b` once.
2. Run `agent session read {session_id} --offset 0 --chunks-json` for both readers and compare `next_offset`.
3. Run the daemon reader-limit/unit tests for the eight-stream bound and disconnect release.
4. Run `npm run test:e2e` and exercise visible "Sessions" navigation, a duplicate output chunk, a stream error, and Process Workspace link navigation.

### Expected

- One active reader row exists for repeated `reader-a`; `reader-b` remains independent.
- Each stream starts at its requested byte offset and reports bounded `next_offset` values.
- A ninth live stream is rejected and a disconnected stream releases its permit.
- Browser reconnect starts from the last committed offset and ignores a repeated `next_offset` chunk.

### Expected Data State

```sql
SELECT client_id, mode, COUNT(*)
FROM session_attachments
WHERE session_id='{session_id}' AND detached_at IS NULL
GROUP BY client_id, mode;
-- Expected: reader-a=1 and reader-b=1 for reader mode
```

---

## Scenario 3: Writer Race, Heartbeat, Fencing, And Atomic Idempotent Input

### Preconditions

- `session_control_enabled` is true in the `_system` RuntimePolicy.
- The mock session has no writer.

### Goal

Verify exactly one writer, monotonic tokens, lease renewal, stale-owner rejection, and exactly-once FIFO input across retries.

### Steps

1. Race writer attach for `writer-a` and `writer-b`; retain the successful token.
2. Heartbeat the winner and send `FR102_ONCE\n` with key `fr102-once`.
3. Repeat the identical request/key, then reuse the key with different input.
4. Detach the winner, acquire the loser, and attempt input and detach with the prior token.
5. Count `mock:FR102_ONCE` in the private fixture stdout only.

### Expected

- Exactly one initial attach succeeds and heartbeat extends its lease.
- The next owner receives a greater fencing token; old input and detach fail.
- Both identical calls report `accepted_bytes=11`, while fixture output contains the line once.
- Conflicting input with the same key fails before FIFO I/O.

### Expected Data State

```sql
SELECT writer_client_id, writer_fencing_token, writer_lease_expires_at
FROM agent_sessions WHERE id='{session_id}';
-- Expected: current owner or NULL; fencing token is monotonic

SELECT idempotency_key, request_hash, result, COUNT(*)
FROM session_control_actions
WHERE session_id='{session_id}' AND idempotency_key='fr102-once'
GROUP BY idempotency_key, request_hash, result;
-- Expected: one accepted row
```

---

## Scenario 4: Policy, RBAC, PID Identity, Close, And Redaction

### Preconditions

- Prepare secure TCP operator/admin and `--uds-max-role read-only` clients in the isolated data directory.
- Keep the session numeric PID live.

### Goal

Verify global feature gating, dynamic role elevation, fail-closed process identity, safe close, correlated audit, and content non-disclosure.

### Steps

1. Disable `_system` `RuntimePolicy.spec.session_control_enabled`; attempt writer attach as operator, then restore it.
2. Under read-only UDS, run List/Get/Read and reader Attach; attempt writer Attach, SendInput, and Close.
3. Replace the stored process fingerprint while retaining the live PID; attempt input and close and verify `kill -0` still succeeds.
4. Restore a verified fingerprint and close by `session_id` with reason/idempotency/version.
5. Search daemon logs and canonical audit output for all deterministic input markers.

### Expected

- The global control flag denies mutations while reads remain available.
- Read-only observation succeeds; writer/input/close are denied with correlated request IDs.
- A mismatched live PID performs no input or signal; verified close transitions through draining and terminates only the fixture PID.
- Every domain mutation row has trusted actor and `request_id`; no input/transcript marker appears in daemon or audit output.

### Expected Data State

```sql
SELECT COUNT(*) FROM session_control_actions
WHERE actor='' OR request_id IS NULL OR request_id='';
-- Expected: 0

SELECT status, error_code, transport, resolved_role
FROM control_action_audit
WHERE target_id='{session_id}' ORDER BY created_at;
-- Expected: succeeded/failed/denied evidence with TCP or UDS role context
```

---

## Scenario 5: Restart Convergence And Session Inspector Entry Visibility

### Preconditions

- Persist an attachable session row pointing to a controlled external process, valid fingerprint, FIFO, and transcript.
- Stop the first isolated daemon.

### Goal

Verify daemon restarts recover a verified session and the visible GUI surfaces accurately expose read/write capability.

### Steps

1. Start the read-only daemon on the persisted data directory and confirm the session converges to detached.
2. Stop it and restart the secure TCP daemon; confirm the session remains detached and attachable.
3. Close the external process through the verified session API and observe draining/process termination.
4. In Playwright, enter through the visible "Sessions" navigation, inspect transcript/state/PID/writer summary, acquire/send/release control, and follow the process link.
5. Repeat with read-only role and inspect focusable controls.

### Expected

- Both restarts converge the live no-writer fixture to detached without losing evidence or reviving terminal ownership.
- Verified close signals the fixture and leaves correlated action evidence.
- Session Inspector and Process Workspace are reachable without direct-route knowledge.
- Read-only DOM contains no "Request control", "Close session", or terminal input control.

### Expected Data State

```sql
SELECT state, writer_client_id, writer_lease_expires_at, ended_at
FROM agent_sessions WHERE id='{session_id}';
-- Expected before close: detached, NULL, NULL, NULL; after convergence: closed with ended_at
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Populated migration and deterministic reconciliation | PASS | 2026-07-14 | Codex | Default isolated script ran all targeted migration/lifecycle regressions |
| 2 | Independent bounded readers and UI offset re-entry | PASS | 2026-07-14 | Codex | Reader idempotency/offset checks, permit tests, and Playwright reconnect passed |
| 3 | Writer race, heartbeat, fencing, and atomic idempotent input | PASS | 2026-07-14 | Codex | One winner, increasing token, accepted replay, conflict, and single fixture write passed |
| 4 | Policy, RBAC, PID identity, close, and redaction | PASS | 2026-07-14 | Codex | Global flag, read-only UDS denials, live mismatch, request joins, and marker scan passed |
| 5 | Restart convergence and Session Inspector entry visibility | PASS | 2026-07-14 | Codex | UDS/TCP restart fixture and five Playwright scenarios passed |
