# Orchestrator - Agent Session Control Plane

**Module**: orchestrator
**Scope**: Session migration, observation, fenced control, restart safety, and Process Console interaction
**Scenarios**: 5
**Priority**: High

---

## Background

Apply the deterministic mock fixture before workflow scenarios:

```bash
orchestrator apply -f fixtures/manifests/bundles/session-control-mock.yaml
```

The public surface is `orchestrator agent session ...` and the nine `AgentSession*` gRPC methods. PID is diagnostic only. Never use a live AI workflow for this QA.

---

## Database Schema Reference

Migration 29 extends `agent_sessions` with `state_version`, writer lease/fencing fields, and `process_fingerprint`. `session_control_actions` stores idempotency/audit reservations. Internal path columns must never appear in gRPC, CLI JSON, Tauri, or UI payloads.

---

## Scenario 1: Migration Compatibility And Public Observation

### Preconditions
- Back up a database containing legacy `active`, `detached`, and `exited` session rows.
- Start the upgraded daemon so migration 29 runs.

### Goal
Verify lossless migration, canonical state projection, filters, and path non-disclosure.

### Steps
1. Run `orchestrator db migrations list` and confirm version 29 is applied.
2. Run `orchestrator agent session list -o json` and `orchestrator agent session get {session_id} -o json`.
3. Repeat List with `--task {task_id}`, `--agent {agent_id}`, and `--state closed`.
4. Search responses for `fifo`, `transcript_path`, `stdout_path`, `stderr_path`, `cwd`, and `command`.

### Expected
- Every legacy row remains present; legacy `exited` is `closed`.
- Filters return only matching sessions.
- No internal path or command field is returned.

### Expected Data State
```sql
SELECT version, name FROM schema_migrations WHERE version=29;
-- Expected: 29 | m0029_agent_session_control_plane
SELECT id, state, state_version FROM agent_sessions WHERE id='{session_id}';
-- Expected: the original id; exited migrated to closed; state_version >= 1
```

---

## Scenario 2: Independent Readers And Offset Reconnect

### Preconditions
- Apply `fixtures/manifests/bundles/session-control-mock.yaml`.
- Create/start a task with workflow `session-control-mock`; capture `{session_id}`.

### Goal
Verify bounded readers can consume independent source offsets and resume without duplicate or missing committed chunks.

### Steps
1. Attach `reader-a` and `reader-b` with `orchestrator agent session attach {session_id} --mode reader --client-id reader-a` and the equivalent command for `reader-b`.
2. Read once from offset `0`; record the final returned `next_offset` as `{offset_a}`.
3. Start another reader at offset `0`, then reconnect the first reader with `orchestrator agent session read {session_id} --offset {offset_a} --follow`.
4. Disconnect and reconnect the Portal while Session Inspector or Process Workspace is displaying the visible session panel.

### Expected
- Readers do not alter each other's offsets.
- Reconnected output begins exactly at the committed offset.
- The visible session surface automatically resumes and does not append a chunk whose `next_offset` was already committed.
- A ninth concurrent reader is rejected by the reader bound.

### Expected Data State
```sql
SELECT client_id, mode, detached_at FROM session_attachments
WHERE session_id='{session_id}' ORDER BY id;
-- Expected: independent active reader-a and reader-b rows; no writer mutation
```

---

## Scenario 3: Single Writer, Heartbeat, Fencing, And Idempotent Input

### Preconditions
- A live mock `{session_id}` exists and session mutations are enabled.

### Goal
Verify explicit writer acquisition, renewal, deterministic stale-token rejection, and retry-safe input.

### Steps
1. Acquire writer control as `writer-a`; record `{token_a}` and the lease expiry.
2. Attempt writer attach as `writer-b` before expiry and verify conflict.
3. Heartbeat `writer-a`, send `hello` with `{idempotency_key}`, then retry the identical request/key.
4. Detach `writer-a`, acquire as `writer-b`, and record `{token_b}`.
5. Attempt input and detach with `{token_a}`, then send `quit` with `{token_b}`.

### Expected
- `{token_b}` is greater than `{token_a}`.
- Only the current unexpired token writes; stale operations fail with a stable failed-precondition response.
- Retrying an accepted idempotency key does not write a second input.
- Input text is absent from daemon logs and control-plane audit.

### Expected Data State
```sql
SELECT writer_client_id, writer_fencing_token, writer_lease_expires_at, state_version
FROM agent_sessions WHERE id='{session_id}';
-- Expected: token is monotonic and owner reflects the current lease or NULL after exit
SELECT COUNT(*) FROM session_control_actions
WHERE session_id='{session_id}' AND idempotency_key='{idempotency_key}';
-- Expected: 1
```

---

## Scenario 4: RBAC, Policy Gates, PID Reuse, And Close

### Preconditions
- Prepare `read_only` and `operator` control-plane clients.
- A live mock session exists.

### Goal
Verify the dynamic role boundary and fail-closed process identity.

### Steps
1. As `read_only`, run List/Get/Read, reader Attach, and ResolvePid.
2. As `read_only`, request writer Attach, SendInput, and Close.
3. Disable `session_control_enabled` and repeat mutations as `operator`; re-enable it afterward.
4. Replace the stored fingerprint with a stale value while retaining the PID, then attempt SendInput and Close.
5. Restore the real fingerprint and close by `session_id` with a reason and expected version; verify no mutation command accepts PID.

### Expected
- Read operations succeed for `read_only`; writer and close require `operator`.
- The mutation feature flag denies even an operator when disabled.
- A stale/reused PID never authorizes input or process signaling.
- Close transitions through `draining`; only a verified process receives the signal.

### Expected Data State
```sql
SELECT state, process_fingerprint, ended_at FROM agent_sessions WHERE id='{session_id}';
-- Expected: closed/failed after verified close; stale fingerprint attempt made no mutation
```

---

## Scenario 5: Daemon Restart Reconciliation And UI Entry Visibility

### Preconditions
- Create live, detached, completed, and deliberately inconsistent mock session fixtures.

### Goal
Verify startup convergence and navigation-first global/process session re-entry.

### Steps
1. Stop and restart `orchestratord` while the fixtures are persisted.
2. List sessions and inspect live/detached/closed/failed outcomes.
3. Navigate from Sessions into Session Inspector, follow its process link into Process Workspace, and do not use a hidden route.
4. Verify the visible transcript, state/PID/writer summary, and operator-only "Request control" and "Close session" actions in the applicable session surface.
5. Acquire and release control, then disconnect/reconnect the GUI and verify follow resumes.

### Expected
- Live verified process plus transport becomes active/detached according to lease ownership.
- Dead process with output evidence becomes closed; missing/inconsistent identity or transport becomes failed.
- Expired leases are released and fenced.
- The feature is discoverable globally and from Process Workspace; read-only users cannot see mutation or input controls.

### Expected Data State
```sql
SELECT id, state, writer_client_id, writer_lease_expires_at
FROM agent_sessions WHERE id IN ('{live_id}','{detached_id}','{closed_id}','{failed_id}');
-- Expected: reconciled canonical states; expired writer_client_id is NULL
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Migration compatibility and public observation | ☐ | | | |
| 2 | Independent readers and offset reconnect | ☐ | | | |
| 3 | Writer heartbeat, fencing, and idempotent input | ☐ | | | |
| 4 | RBAC, policy, PID reuse, and close | ☐ | | | |
| 5 | Restart reconciliation and UI entry visibility | ☐ | | | |
