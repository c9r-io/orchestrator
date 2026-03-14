---
self_referential_safe: false
---
# Orchestrator - Daemon Crash Resilience And Worker Survival

**Module**: orchestrator
**Scope**: Validate worker auto-respawn on panic, stale PID crash recovery, panic hook crash log, supervisor health monitoring, and total_worker_restarts metric
**Scenarios**: 5
**Priority**: Critical

---

## Background

This document validates the FR-032 daemon crash resilience closure:

- Worker loop `catch_unwind` wrapping: panics trigger `continue` (not `break`), preserving the worker
- Worker supervisor monitors worker health every 30s and respawns dead workers
- Stale PID detection at startup emits `daemon_crash_recovered` event
- Panic hook appends to `data/daemon_crash.log` before default handler
- `total_worker_restarts` counter exposed via `WorkerStatusResponse` and `debug --component daemon`

All setup commands in Preconditions require a running daemon. If no daemon is running yet, start a temporary daemon in another shell, complete the project reset/apply step, then stop that temporary daemon before the scenario begins.

### Common Preconditions

```bash
# 1. Build release binaries
cargo build --release -p orchestratord -p orchestrator-cli

# 2. Ensure runtime is initialized
test -f data/agent_orchestrator.db || ./target/release/orchestrator init

# 3. Apply mock fixture and set up isolated QA project
QA_PROJECT="fr032-qa-${USER}-$(date +%Y%m%d%H%M%S)"
./target/release/orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
./target/release/orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project "${QA_PROJECT}"
```

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| `debug --component daemon` missing `total_worker_restarts` line | CLI binary is stale; proto regeneration required | Run `cargo clean -p orchestrator-proto && cargo build --release -p orchestratord -p orchestrator-cli` |
| `daemon_crash_recovered` event not emitted on restart | Previous daemon shut down cleanly (PID file removed) | Simulate crash by `kill -9` (SIGKILL) instead of `kill` (SIGTERM) |
| Daemon fails to start with socket bind error | Stale socket from crashed daemon | `rm -f data/orchestrator.sock` before starting |

---

## Scenario 1: Idle Daemon Reports Zero Worker Restarts

### Preconditions

- Common Preconditions applied

### Goal

Verify that a freshly started daemon reports `total_worker_restarts: 0` and all workers idle.

### Steps

1. Start daemon with 2 workers:
   ```bash
   ./target/release/orchestratord --foreground --workers 2 >/tmp/fr032-idle.log 2>&1 &
   DAEMON_PID=$!
   sleep 2
   ```

2. Inspect daemon status:
   ```bash
   ./target/release/orchestrator debug --component daemon
   ```

3. Stop daemon:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```

### Expected

- `debug --component daemon` prints:
  - `lifecycle_state:    serving`
  - `configured_workers: 2`
  - `idle_workers:       2`
  - `active_workers:     0`
  - `running_tasks:      0`
  - `total_worker_restarts: 0`

---

## Scenario 2: Stale PID Detection Emits Crash Recovery Event

### Preconditions

- Common Preconditions applied
- No daemon instance running

### Goal

Verify that when a previous daemon was killed without cleanup (simulating a crash), the next daemon startup detects the stale PID and emits a `daemon_crash_recovered` event.

### Steps

1. Start a daemon, then kill it with SIGKILL (simulating a crash — no cleanup):
   ```bash
   ./target/release/orchestratord --foreground --workers 1 >/tmp/fr032-crash-sim.log 2>&1 &
   DAEMON_PID=$!
   sleep 2
   kill -9 "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null || true
   ```

2. Verify stale PID file remains:
   ```bash
   test -f data/daemon.pid && echo "PID file exists (stale)" || echo "PID file missing"
   cat data/daemon.pid
   ```

3. Restart daemon (it should detect the stale PID):
   ```bash
   rm -f data/orchestrator.sock
   ./target/release/orchestratord --foreground --workers 1 >/tmp/fr032-recover.log 2>&1 &
   NEW_DAEMON_PID=$!
   sleep 3
   ```

4. Check daemon log for crash recovery message:
   ```bash
   grep "stale PID file detected" /tmp/fr032-recover.log
   ```

5. Check events table for `daemon_crash_recovered`:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT event_type, payload_json FROM events WHERE event_type = 'daemon_crash_recovered' ORDER BY id DESC LIMIT 1;"
   ```

6. Stop daemon:
   ```bash
   kill "$NEW_DAEMON_PID"
   wait "$NEW_DAEMON_PID" 2>/dev/null
   ```

### Expected

- After SIGKILL, `data/daemon.pid` remains (not cleaned up by the killed process)
- Daemon log contains: `stale PID file detected — previous daemon likely crashed`
- Events table contains a `daemon_crash_recovered` event with payload `{"source":"stale_pid_detection"}`

### Expected Data State

```sql
SELECT event_type, payload_json FROM events
  WHERE event_type = 'daemon_crash_recovered'
  ORDER BY id DESC LIMIT 1;
-- Expected: daemon_crash_recovered | {"source":"stale_pid_detection"}
```

---

## Scenario 3: Panic Hook Writes Crash Log File

### Preconditions

- Common Preconditions applied

### Goal

Verify that the global panic hook writes crash information to `data/daemon_crash.log` when a panic occurs.

### Steps

1. Start daemon:
   ```bash
   ./target/release/orchestratord --foreground --workers 1 >/tmp/fr032-panic-hook.log 2>&1 &
   DAEMON_PID=$!
   sleep 2
   ```

2. Run a normal task to confirm the daemon is healthy:
   ```bash
   TASK_ID=$(./target/release/orchestrator task create \
     --name "hook-test" \
     --goal "Verify panic hook" \
     --project "${QA_PROJECT}" \
     --workflow qa_only 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   sleep 5
   ./target/release/orchestrator task info "$TASK_ID"
   ```

3. Verify crash log file location exists (file may be empty if no panics have occurred during this run):
   ```bash
   ls -la data/daemon_crash.log 2>/dev/null || echo "No crash log yet (expected for clean run)"
   ```

4. Stop daemon:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```

### Expected

- Task completes successfully (status: `completed`)
- `data/daemon_crash.log` may or may not exist depending on whether any previous run produced a panic
- When it does exist, each line follows the format: `[epoch=<unix_seconds>] <panic info>`
- The panic hook does not interfere with normal task execution

---

## Scenario 4: Worker Supervisor Keeps Workers Alive During Task Execution

### Preconditions

- Common Preconditions applied (echo-workflow.yaml)

### Goal

Verify that the worker supervisor maintains configured worker count and workers process tasks normally. After task completion, all workers return to idle and counters are consistent.

### Steps

1. Start daemon with 2 workers:
   ```bash
   ./target/release/orchestratord --foreground --workers 2 >/tmp/fr032-supervisor.log 2>&1 &
   DAEMON_PID=$!
   sleep 2
   ```

2. Create and run a task:
   ```bash
   TASK_ID=$(./target/release/orchestrator task create \
     --name "supervisor-test" \
     --goal "Test supervisor keeps workers alive" \
     --project "${QA_PROJECT}" \
     --workflow qa_only 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   sleep 5
   ```

3. Inspect daemon status after task:
   ```bash
   ./target/release/orchestrator debug --component daemon
   ./target/release/orchestrator task info "$TASK_ID"
   ```

4. Verify supervisor log messages:
   ```bash
   grep "worker supervisor started\|initial workers spawned\|worker started" /tmp/fr032-supervisor.log
   ```

5. Verify worker state events:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT event_type, payload_json FROM events WHERE event_type IN ('worker_state_changed','worker_panic_recovered','worker_respawned') AND payload_json LIKE '%supervisor%' OR event_type IN ('worker_state_changed') ORDER BY id DESC LIMIT 10;"
   ```

6. Stop daemon:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```

### Expected

- Task status: `completed`, Failed: 0
- `debug --component daemon` after task completion:
  - `configured_workers: 2`
  - `idle_workers:       2`
  - `active_workers:     0`
  - `running_tasks:      0`
  - `total_worker_restarts: 0`
- Daemon log contains `worker supervisor started` and `initial workers spawned`
- Worker state events show transitions: `new→idle`, `idle→busy`, `busy→idle`
- No `worker_panic_recovered` or `worker_respawned` events (clean run)

---

## Scenario 5: Graceful Shutdown Unchanged (Regression)

### Preconditions

- Common Preconditions applied
- Apply the long-running fixture for drain testing:
  ```bash
  ./target/release/orchestrator delete project/fr032-sleep --force 2>/dev/null || true
  ./target/release/orchestrator apply -f fixtures/manifests/bundles/pause-resume-workflow.yaml --project fr032-sleep
  ```

### Goal

Verify that the supervisor-based architecture does not regress the existing graceful SIGTERM shutdown and task drain behavior.

### Steps

1. Start daemon with 1 worker:
   ```bash
   ./target/release/orchestratord --foreground --workers 1 >/tmp/fr032-graceful.log 2>&1 &
   DAEMON_PID=$!
   sleep 2
   ```

2. Create and start a long-running task:
   ```bash
   TASK_ID=$(./target/release/orchestrator task create \
     --name "drain-regression" \
     --goal "Verify graceful shutdown" \
     --project fr032-sleep \
     --workflow qa_sleep 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   echo "$TASK_ID"
   sleep 2
   ```

3. Send SIGTERM while the task is running:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```

4. Verify cleanup:
   ```bash
   test ! -f data/daemon.pid && echo "PID file cleaned" || echo "PID file remains"
   test ! -S data/orchestrator.sock && echo "Socket cleaned" || echo "Socket remains"
   sqlite3 data/agent_orchestrator.db "SELECT status FROM tasks WHERE id = '${TASK_ID}';"
   sqlite3 data/agent_orchestrator.db \
     "SELECT event_type FROM events WHERE event_type IN ('daemon_shutdown_requested','task_drain_started','task_drain_completed','daemon_shutdown_completed') ORDER BY id DESC LIMIT 4;"
   ```

### Expected

- Daemon exits cleanly after SIGTERM
- `data/daemon.pid` and `data/orchestrator.sock` are removed
- Task status is `paused`
- Shutdown events are emitted in order:
  - `daemon_shutdown_requested`
  - `task_drain_started`
  - `task_drain_completed`
  - `daemon_shutdown_completed`

### Expected Data State

```sql
SELECT status FROM tasks WHERE id = '{task_id}';
-- Expected: paused
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Idle Daemon Reports Zero Worker Restarts | ✅ | 2026-03-13 | chenhan | serving, 2 idle workers, total_worker_restarts: 0 |
| 2 | Stale PID Detection Emits Crash Recovery Event | ✅ | 2026-03-13 | chenhan | SIGKILL→stale PID→daemon_crash_recovered event with source:stale_pid_detection |
| 3 | Panic Hook Writes Crash Log File | ✅ | 2026-03-13 | chenhan | Task 90/90 completed, crash log absent (no panics in clean run), hook does not interfere |
| 4 | Worker Supervisor Keeps Workers Alive During Task Execution | ✅ | 2026-03-13 | chenhan | 90/90 completed, 2 idle workers post-task, supervisor spawned/monitored correctly, no panic/respawn events |
| 5 | Graceful Shutdown Unchanged (Regression) | ✅ | 2026-03-13 | chenhan | PID+socket cleaned, task paused, all 4 drain events in order |
