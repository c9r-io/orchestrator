# Orchestrator - Daemon Lifecycle And Runtime Metrics

**Module**: orchestrator
**Scope**: Validate daemon runtime metrics, worker activity counters, graceful drain, and restart state reset
**Scenarios**: 4
**Priority**: Critical

---

## Purpose

This document validates the FR-005 daemon lifecycle closure:

- shared daemon runtime snapshot in `core`
- real `Ping` / `WorkerStatus` fields
- live worker/task counters for embedded daemon workers
- graceful signal-driven drain for long-running tasks
- readable daemon status through `orchestrator debug --component daemon`

Use only mock fixtures from `fixtures/manifests/bundles/`.

All setup commands in Preconditions require a running daemon. If no daemon is running yet, start a temporary daemon in another shell, complete the project reset/apply step, then stop that temporary daemon before the scenario begins.

---

## Scenario 1: Idle Daemon Reports Serving State And Worker Capacity

### Preconditions
- Release binaries are built:
  ```bash
  cargo build --release -p orchestratord -p orchestrator-cli
  ```
- Runtime is initialized if needed:
  ```bash
  test -f data/agent_orchestrator.db || ./target/release/orchestrator init
  ```
- Isolated QA project is reset and seeded with a mock fixture:
  ```bash
  ./target/release/orchestrator delete project/fr005-qa --force || true
  ./target/release/orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project fr005-qa
  ```

### Goal
Verify that an idle daemon reports serving lifecycle state, non-zero uptime, and correct idle worker capacity.

### Steps
1. Start daemon with 2 workers:
   ```bash
   ./target/release/orchestratord --foreground --workers 2 >/tmp/fr005-daemon-idle.log 2>&1 &
   DAEMON_PID=$!
   sleep 2
   ```
2. Check version/ping path:
   ```bash
   ./target/release/orchestrator version
   ```
3. Check daemon status view:
   ```bash
   ./target/release/orchestrator debug --component daemon
   ```
4. Stop daemon:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```

### Expected
- `version` prints both client and daemon versions.
- `debug --component daemon` prints:
  - `lifecycle_state:    serving`
  - `shutdown_requested: false`
  - `configured_workers: 2`
  - `active_workers:     0`
  - `idle_workers:       2`
  - `running_tasks:      0`
- `uptime_secs` is present and greater than or equal to `1`.

---

## Scenario 2: Live Task Execution Updates Active, Idle, And Running Counts

### Preconditions
- Release binaries are built.
- Isolated QA project is reset and seeded with the long-running mock fixture:
  ```bash
  ./target/release/orchestrator delete project/fr005-sleep --force || true
  ./target/release/orchestrator apply -f fixtures/manifests/bundles/pause-resume-workflow.yaml --project fr005-sleep
  ```

### Goal
Verify that daemon metrics reflect a real in-flight task and return to idle after completion.

### Steps
1. Start daemon with 1 worker:
   ```bash
   ./target/release/orchestratord --foreground --workers 1 >/tmp/fr005-daemon-busy.log 2>&1 &
   DAEMON_PID=$!
   sleep 2
   ```
2. Create and start a long-running task:
   ```bash
   TASK_ID=$(./target/release/orchestrator task create --name "sleep-task" --goal "busy metrics" --project fr005-sleep --workflow qa_sleep 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   echo "$TASK_ID"
   sleep 2
   ```
3. Inspect daemon status while the task is running:
   ```bash
   ./target/release/orchestrator debug --component daemon
   ./target/release/orchestrator task info "$TASK_ID" -o json
   ```
4. Wait for task completion:
   ```bash
   sleep 16
   ./target/release/orchestrator debug --component daemon
   ./target/release/orchestrator task info "$TASK_ID" -o json
   ```
5. Stop daemon:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```

### Expected
- During execution, `debug --component daemon` prints:
  - `lifecycle_state:    serving`
  - `shutdown_requested: false`
  - `configured_workers: 1`
  - `active_workers:     1`
  - `idle_workers:       0`
  - `running_tasks:      1`
- After completion, the same command prints:
  - `active_workers:     0`
  - `idle_workers:       1`
  - `running_tasks:      0`
- `task info` shows the task as `running` during the first inspection and terminal (`completed` or `failed`) after the wait.

### Expected Data State
```sql
SELECT status FROM tasks WHERE id = '{task_id}';
-- Expected after the final wait: completed
```

---

## Scenario 3: SIGTERM Drains A Running Task And Leaves It Paused

### Preconditions
- Release binaries are built.
- The long-running mock fixture is seeded in an isolated project before starting the scenario daemon:
  ```bash
  ./target/release/orchestrator delete project/fr005-sleep --force || true
  ./target/release/orchestrator apply -f fixtures/manifests/bundles/pause-resume-workflow.yaml --project fr005-sleep
  ```

### Goal
Verify that a signal-triggered daemon shutdown drains a running task, pauses the unfinished task, and cleans PID/socket files.

### Steps
1. Start daemon with 1 worker:
   ```bash
   ./target/release/orchestratord --foreground --workers 1 >/tmp/fr005-daemon-drain.log 2>&1 &
   DAEMON_PID=$!
   sleep 2
   ```
2. Create and start a long-running task:
   ```bash
   TASK_ID=$(./target/release/orchestrator task create --name "drain-task" --goal "drain" --project fr005-sleep --workflow qa_sleep 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   echo "$TASK_ID"
   sleep 2
   ```
3. Send `SIGTERM` while the task is still running:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```
4. Verify daemon cleanup and task state:
   ```bash
   test ! -f data/daemon.pid
   test ! -S data/orchestrator.sock
   sqlite3 data/agent_orchestrator.db "SELECT status FROM tasks WHERE id = '${TASK_ID}';"
   sqlite3 data/agent_orchestrator.db "SELECT event_type FROM events WHERE event_type IN ('daemon_shutdown_requested','task_drain_started','task_drain_completed','daemon_shutdown_completed') ORDER BY id;"
   ```

### Expected
- Daemon exits cleanly after the signal.
- `data/daemon.pid` and `data/orchestrator.sock` are removed.
- SQLite shows the task status as `paused`.
- Events include:
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

## Scenario 4: Fresh Restart Resets Lifecycle Flags And Worker Counts

### Preconditions
- Scenario 3 completed and no daemon instance is running.
- The `fr005-sleep` project from Scenario 3 still exists.

### Goal
Verify that a fresh daemon process starts in clean `serving` state after a drained shutdown.

### Steps
1. Start daemon with 1 worker:
   ```bash
   ./target/release/orchestratord --foreground --workers 1 >/tmp/fr005-daemon-restart.log 2>&1 &
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
- The restarted daemon reports:
  - `lifecycle_state:    serving`
  - `shutdown_requested: false`
  - `configured_workers: 1`
  - `active_workers:     0`
  - `idle_workers:       1`
  - `running_tasks:      0`
- The paused task from Scenario 3 does not cause stale worker or running-task counts on startup.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Idle Daemon Reports Serving State And Worker Capacity | ✅ | 2026-03-11 | Codex | `debug --component daemon` showed serving state, uptime, and 2 idle workers |
| 2 | Live Task Execution Updates Active, Idle, And Running Counts | ✅ | 2026-03-11 | Codex | Long-running `qa_sleep` task moved `active_workers/running_tasks` to `1` and returned to idle after completion |
| 3 | SIGTERM Drains A Running Task And Leaves It Paused | ✅ | 2026-03-11 | Codex | Fresh daemon seed + signal shutdown produced `paused` task state and persisted drain events |
| 4 | Fresh Restart Resets Lifecycle Flags And Worker Counts | ✅ | 2026-03-11 | Codex | New daemon process returned to clean `serving` state with no stale running-task counts |
