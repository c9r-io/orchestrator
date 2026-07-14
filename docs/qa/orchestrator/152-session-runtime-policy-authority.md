---
self_referential_safe: true
---

# Orchestrator - Session RuntimePolicy Authority

**Module**: Orchestrator  
**Scope**: Deterministic `_system` policy resolution, Session read/control gates, hot apply, restart persistence, and safety regression  
**Scenarios**: 4  
**Priority**: Critical

---

## Background

This document is the executable closure evidence for FR-105 and DD-115. It supplements QA-149: QA-149 remains the Session lifecycle and UI baseline, while this document is authoritative for global RuntimePolicy selection and rollout/rollback behavior.

Use only the provider-free isolated fixture. The script creates temporary HOME, data, and workspace roots, builds current daemon and CLI binaries by default, uses bounded retries only for explicit control-plane rate limiting, and requires exact policy-denial messages before accepting a negative result.

```bash
./scripts/qa/test-agent-session-control-plane.sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cd gui && npm test && npm run test:e2e && npm run build
```

## Scenario 1: Deterministic Global And Project Resolution

### Preconditions

- The unit test configuration store is empty.
- Prepare conflicting `_system` and ordinary-project RuntimePolicy resources.

### Goal

Prove global consumers always select `_system`, project consumers preserve project-to-system fallback, insertion order is irrelevant, and a missing global policy is safe.

### Steps

1. Insert `_system.session_control_enabled=false` and `project.session_control_enabled=true`; assert the global accessor is false.
2. Repeat with the insertion order reversed.
3. Assert the project accessor reads the project value, then remove it and assert fallback to `_system`.
4. Remove `_system`; assert the default enables read but disables mutation.

### Expected

- Both insertion orders produce the same global result.
- No ordinary project can become the global singleton by iteration order.
- Project lookup remains project, `_system`, default.
- Missing global configuration does not fail open for mutation.

## Scenario 2: Immediate Global Mutation Denial Without State Change

### Preconditions

- Start the isolated secure TCP daemon with `_system.session_control_enabled=true`.
- Materialize a Session and acquire a valid writer lease.
- Apply an ordinary project policy with mutation enabled.

### Goal

Prove that a successful apply of `_system.session_control_enabled=false` immediately blocks every mutation before domain or process effects.

### Steps

1. Record the Session state version and writer owner.
2. Apply the disabled `_system` policy, then attempt an invalid `_system` policy and require manifest validation to reject it.
3. Immediately attempt writer Attach, Heartbeat, SendInput, writer Detach, and Close.
4. Require every command to fail with `session mutation APIs are disabled`, not a transport or rate-limit error.
5. Read the Session and compare state version and writer owner with the recorded values.
6. Restore `_system=true`, detach normally, set the ordinary project to false, and prove a writer can still attach because `_system` is authoritative.

### Expected

- All five mutation families fail closed after apply returns.
- Rejected invalid configuration does not replace the last valid disabled snapshot.
- No input marker reaches the fixture and no Session state or lease mutation occurs.
- Reads remain available while only the mutation flag is disabled.
- `_system=true` overrides an ordinary project set to false.

### Expected Data State

```sql
SELECT state_version, writer_client_id
FROM agent_sessions WHERE id='{session_id}';
-- Expected after denied mutations: unchanged from the pre-apply values

SELECT COUNT(*) FROM session_control_actions
WHERE session_id='{session_id}' AND idempotency_key='fr105-policy-denied';
-- Expected: 0
```

## Scenario 3: Read Gate Hot Restore And Restart Persistence

### Preconditions

- The isolated Session remains readable and detached.

### Goal

Verify the independent read gate changes immediately and a disabled mutation gate remains authoritative across daemon restarts.

### Steps

1. Apply `_system.session_read_enabled=false`.
2. Immediately run Session List, Get, Read, and reader Attach; require `session read APIs are disabled` for each.
3. Restore `_system.session_read_enabled=true`; immediately run Get and Read successfully without restart.
4. Persist `_system.session_control_enabled=false`, restart first with read-only UDS and then secure TCP, and attempt writer Attach.
5. Require the post-restart attach to fail with `session mutation APIs are disabled`; restore true and close the controlled fixture.

### Expected

- Read disable and restore take effect on the first request after successful apply.
- Read gating does not terminate the controlled process.
- The persisted `_system` mutation decision is identical before and after both restarts.
- Restoring mutation enables the verified close path.

## Scenario 4: Existing Safety, Privacy, And Client Regression

### Preconditions

- Run the default QA script without `SKIP_BUILD` or `SKIP_TARGETED_TESTS`.

### Goal

Ensure the authority fix changes only policy selection and preserves Session safety and clients.

### Steps

1. Run the default isolated script and require `Agent session control-plane QA: 5 passed, 0 failed`.
2. Confirm writer race, heartbeat, increasing fencing tokens, exactly-once input, stale-owner rejection, live PID mismatch, read-only UDS RBAC, audit joins, restart reconciliation, and marker redaction pass.
3. Run all workspace tests and strict Clippy.
4. Run GUI unit tests, Playwright Session flows, and the production build.

### Expected

- The isolated script builds current binaries and reports exactly five passes.
- Existing lifecycle, authority, audit, and privacy invariants remain green.
- No public RPC, CLI, Tauri, database, or UI behavior regresses.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Deterministic global and project resolution | PASS | 2026-07-15 | Codex | Both insertion orders, project fallback, and missing-global safe default passed |
| 2 | Immediate global mutation denial without state change | PASS | 2026-07-15 | Codex | Invalid apply stayed fail-closed; five mutation families were denied with unchanged state |
| 3 | Read gate hot restore and restart persistence | PASS | 2026-07-15 | Codex | List/Get/Read/reader Attach denial, immediate restore, and UDS/TCP restart authority passed |
| 4 | Existing safety, privacy, and client regression | PASS | 2026-07-15 | Codex | Isolated QA 5/5, workspace tests, strict Clippy, 12 Vitest, 10 Playwright, and build passed |
