# Self-Restart Socket Continuity

**Module**: orchestrator
**Scope**: Verify socket and PID file consistency across self_restart exec() lifecycle
**Scenarios**: 4

---

## Scenario 1: Build failure does not disrupt daemon connectivity

**Precondition**: Daemon is running; self_restart step triggers but cargo build fails

### Steps

1. Start a task containing a `self_restart` builtin step
2. Ensure the build fails (e.g., introduce a compile error)
3. After the step completes with `SelfRestartOutcome::Failed`, run `orchestrator task list`

### Expected

- Daemon remains running on the same PID
- CLI connects successfully without retry
- PID file points to the running daemon process
- Socket file exists and accepts connections

---

## Scenario 2: CLI retries on transient connection failure

**Precondition**: Daemon socket exists but is temporarily unresponsive

### Steps

1. Stop the daemon and leave a stale socket file in place
2. Start the daemon in background
3. Immediately run `orchestrator task list` before socket is bound

### Expected

- CLI prints retry messages: `daemon connection attempt 1/3 failed, retrying in 1s…`
- CLI successfully connects on a subsequent attempt (within 3s)
- Command completes normally

---

## Scenario 3: exec() path preserves socket file

**Precondition**: Daemon is running; self_restart succeeds and exec() is triggered

### Steps

1. Trigger a successful self_restart (build + verify + snapshot all pass)
2. Observe the daemon shutdown sequence in logs
3. Verify socket file handling before exec()

### Expected

- Log shows `exec-ing new daemon binary`
- Only PID file is removed before exec() (not the socket)
- New daemon process starts, removes stale socket, binds fresh socket
- `daemon_socket_ready` event is emitted after bind
- CLI can connect within 3 seconds of exec()

---

## Scenario 4: PID file consistency after exec() failure

**Precondition**: exec() is attempted but fails (e.g., binary path invalid)

### Steps

1. Corrupt or replace the target binary path so exec() fails
2. Observe the daemon error log

### Expected

- Error logged: `exec failed: ...`
- PID file is re-written with the current process PID before exit
- On next daemon start, stale PID detection correctly identifies the dead process
- `daemon_crash_recovered` event emitted on recovery start

---
