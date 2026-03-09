# Orchestrator - Client/Server Architecture

**Module**: orchestrator
**Scope**: Validate gRPC daemon lifecycle, CLI-to-daemon communication, embedded worker, and service layer correctness
**Scenarios**: 5
**Priority**: Critical

---

## Purpose

This document validates the client/server architecture refactor that splits the monolithic CLI into:

- **orchestratord** (daemon): long-running gRPC server over Unix Domain Socket with embedded background worker
- **orchestrator** (CLI client): lightweight gRPC client that communicates with the daemon
- **core/src/service/**: pure business logic layer extracted from cli_handler

The daemon holds all state (engine, DB, task queue) and the CLI is a thin RPC client with zero dependency on the core engine.

Entry points:
- Daemon: `./target/release/orchestratord [--foreground] [--bind addr] [--workers N]`
- CLI: `./target/release/orchestrator <command>`

---

## Background

### Architecture Overview

```
orchestrator (CLI)  ──gRPC/UDS──>  orchestratord (daemon)
                                      ├── gRPC server (tonic)
                                      ├── embedded worker loop (N workers)
                                      ├── core engine (state, DB, scheduler)
                                      └── lifecycle (PID, socket, signals)
```

### Key Files

| Component | Path |
|-----------|------|
| Proto definition | `proto/orchestrator.proto` |
| Proto codegen | `crates/proto/src/lib.rs` |
| Daemon binary | `crates/daemon/src/main.rs` |
| Daemon gRPC server | `crates/daemon/src/server.rs` |
| Daemon lifecycle | `crates/daemon/src/lifecycle.rs` |
| CLI binary | `crates/cli/src/main.rs` |
| CLI gRPC client | `crates/cli/src/client.rs` |
| Service layer | `core/src/service/{task,resource,store,system,bootstrap}.rs` |

### Transport

- Default: Unix Domain Socket at `$APP_ROOT/data/orchestrator.sock`
- Optional: TCP via `--bind 0.0.0.0:50051`
- PID file at `$APP_ROOT/data/daemon.pid`

---

## Scenario 1: Daemon Startup and Shutdown

### Preconditions
- Workspace initialized with `init` and `apply -f <manifest>` via legacy CLI.
- No other daemon instance running.

### Goal
Verify daemon starts on UDS, creates PID/socket files, and shuts down cleanly on SIGTERM.

### Steps
1. Start daemon in foreground mode:
   ```bash
   ./target/release/orchestratord --foreground &
   DAEMON_PID=$!
   sleep 2
   ```
2. Verify PID file and socket exist:
   ```bash
   test -f data/daemon.pid && echo "PID file exists"
   test -S data/orchestrator.sock && echo "Socket exists"
   cat data/daemon.pid
   ```
3. Verify PID matches:
   ```bash
   [ "$(cat data/daemon.pid)" = "$DAEMON_PID" ] && echo "PID matches"
   ```
4. Send SIGTERM and wait:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```
5. Verify cleanup:
   ```bash
   test ! -f data/daemon.pid && echo "PID file cleaned up"
   test ! -S data/orchestrator.sock && echo "Socket cleaned up"
   ```

### Expected
- Daemon starts, logs `orchestratord starting` with socket path and version info.
- PID file contains correct process ID.
- UDS socket file is created and is a socket type.
- On SIGTERM, daemon logs `received SIGTERM, shutting down`, workers stop, and both PID/socket files are removed.
- Exit code is 0.

---

## Scenario 2: CLI-to-Daemon gRPC Communication

### Preconditions
- Daemon is running (foreground or background).
- Config applied via legacy CLI or previous daemon session.

### Goal
Verify CLI client connects to daemon over UDS and basic RPC round-trips work.

### Steps
1. Start daemon:
   ```bash
   ./target/release/orchestratord --foreground &
   DAEMON_PID=$!
   sleep 2
   ```
2. Test version/ping:
   ```bash
   ./target/release/orchestrator version
   ```
3. Test resource listing:
   ```bash
   ./target/release/orchestrator get workspaces
   ./target/release/orchestrator get agents -o json
   ./target/release/orchestrator get workflows -o yaml
   ```
4. Test debug:
   ```bash
   ./target/release/orchestrator debug --component config
   ```
5. Test preflight check:
   ```bash
   ./target/release/orchestrator check -o json
   ```
6. Stop daemon:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```

### Expected
- `version` shows client version and daemon version (from Ping RPC).
- `get workspaces/agents/workflows` returns the same data as legacy CLI.
- `debug --component config` returns active configuration YAML.
- `check` returns preflight validation results.
- All commands complete without connection errors.

---

## Scenario 3: Task Lifecycle via gRPC

### Preconditions
- Daemon running with at least 1 embedded worker (`--workers 1`).
- Config applied with a valid workspace and workflow.
- A QA project created for isolation: `./target/release/orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project cs-qa`

### Goal
Verify task lifecycle (create, list, info, start, pause, delete) works through gRPC.

### Steps
1. Start daemon with embedded workers:
   ```bash
   ./target/release/orchestratord --foreground --workers 1 &
   DAEMON_PID=$!
   sleep 2
   ```
2. Create task (no-start):
   ```bash
   TASK_ID=$(./target/release/orchestrator task create --name "grpc-test" --goal "test" --project cs-qa --no-start 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   echo "Created: $TASK_ID"
   ```
3. List tasks:
   ```bash
   ./target/release/orchestrator task list -o json
   ```
4. Get task info:
   ```bash
   ./target/release/orchestrator task info "$TASK_ID" -o json
   ```
5. Start task (detach — let embedded worker pick it up):
   ```bash
   ./target/release/orchestrator task start "$TASK_ID" --detach
   ```
6. Wait for worker to finish, then check status:
   ```bash
   sleep 10
   ./target/release/orchestrator task info "$TASK_ID" -o json
   ```
7. View logs:
   ```bash
   ./target/release/orchestrator task logs "$TASK_ID" --tail 20
   ```
8. Clean up:
   ```bash
   ./target/release/orchestrator task delete "$TASK_ID" --force
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```

### Expected
- Task is created with status `created` or `pending`.
- `task list` shows the task in JSON output.
- `task start --detach` enqueues the task (status `pending`).
- Embedded worker picks up and executes the task.
- After execution, task reaches terminal status (`completed` or `failed`).
- `task logs` returns run output grouped by phase.
- `task delete --force` removes the task.

### Expected Data State
```sql
SELECT id, status FROM tasks WHERE id = '{task_id}';
-- Expected: terminal status after worker execution
```

---

## Scenario 4: Embedded Worker Queue Consumption

### Preconditions
- Daemon running with multiple workers (`--workers 3`).
- A QA project created for isolation: `./target/release/orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project cs-qa`

### Goal
Verify embedded daemon workers consume pending tasks concurrently and atomically.

### Steps
1. Start daemon with 3 workers:
   ```bash
   ./target/release/orchestratord --foreground --workers 3 &
   DAEMON_PID=$!
   sleep 2
   ```
2. Create 6 tasks in detach mode:
   ```bash
   for i in $(seq 1 6); do
     ./target/release/orchestrator task create --name "batch-$i" --goal "batch test $i" --project cs-qa --detach
   done
   ```
3. Monitor worker progress:
   ```bash
   sleep 5
   ./target/release/orchestrator task list -o json
   ```
4. Wait for completion and verify all reached terminal status:
   ```bash
   sleep 30
   ./target/release/orchestrator task list -o json | grep -c '"status"'
   ```
5. Stop daemon:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```

### Expected
- All 6 tasks transition from `pending` through `running` to `completed` or `failed`.
- Workers log `claimed task` and `task finished` for each task.
- No task is executed more than once (atomic claim via `claim_next_pending_task`).
- Daemon shuts down cleanly after SIGTERM, waiting for in-progress tasks to finish (up to 30s drain timeout).

### Expected Data State
```sql
SELECT COUNT(*) FROM tasks WHERE status IN ('completed', 'failed');
-- Expected: 6 (all batch tasks reached terminal)

SELECT COUNT(*) FROM events WHERE event_type = 'scheduler_enqueued';
-- Expected: >= 6 (one enqueue event per task)
```

---

## Scenario 5: Resource Management and Project Isolation via gRPC

### Preconditions
- Daemon running.
- A valid manifest YAML file available.

### Goal
Verify resource apply (from file and stdin), store CRUD, and project-scoped resource management work through gRPC.

### Steps
1. Start daemon:
   ```bash
   ./target/release/orchestratord --foreground &
   DAEMON_PID=$!
   sleep 2
   ```
2. Apply manifest from file:
   ```bash
   ./target/release/orchestrator apply -f fixtures/manifests/bundles/output-formats.yaml
   ```
3. Apply from stdin:
   ```bash
   cat fixtures/manifests/bundles/output-formats.yaml | ./target/release/orchestrator apply -f -
   ```
4. Dry-run apply:
   ```bash
   ./target/release/orchestrator apply -f fixtures/manifests/bundles/output-formats.yaml --dry-run
   ```
5. Get and describe resources:
   ```bash
   ./target/release/orchestrator get workspace/default -o yaml
   ./target/release/orchestrator describe workspace/default -o yaml
   ```
6. Store CRUD:
   ```bash
   ./target/release/orchestrator store put qa-store test-key '{"value":42}'
   ./target/release/orchestrator store get qa-store test-key
   ./target/release/orchestrator store list qa-store -o json
   ./target/release/orchestrator store delete qa-store test-key
   ```
7. Apply manifest to project scope and verify isolation:
   ```bash
   ./target/release/orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project iso-test
   ./target/release/orchestrator get agents --project iso-test
   ./target/release/orchestrator describe agent/mock_echo --project iso-test
   ```
8. Create and list tasks in project scope:
   ```bash
   ./target/release/orchestrator task create --name "iso-task" --goal "isolation test" --project iso-test --workflow qa_only --no-start
   ./target/release/orchestrator task list --project iso-test -o json
   ```
9. Delete project resource and reset project:
   ```bash
   ./target/release/orchestrator delete agent/mock_echo --force --project iso-test
   ./target/release/orchestrator delete project/iso-test --force
   ```
10. Stop daemon:
    ```bash
    kill "$DAEMON_PID"
    wait "$DAEMON_PID" 2>/dev/null
    ```

### Expected
- `apply` creates/updates resources and prints `kind/name created|updated|unchanged`.
- `apply --dry-run` prints `would be created` without persisting changes.
- `apply -f -` reads from stdin and works identically to file mode.
- `get workspace/default` returns workspace data in requested format.
- Store `put`, `get`, `list`, `delete` operations succeed over gRPC.
- `apply --project` creates resources in project scope with `(project: iso-test)` suffix.
- `get agents --project` returns only project-scoped agents (not global).
- `task create --project` creates task with correct `project_id`.
- `delete project/<name> --force` deletes the project and all its data (tasks/items/runs/events and config).

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Daemon Startup and Shutdown | ✅ | 2026-03-09 | Claude | PID/socket create+cleanup, startup/shutdown logs |
| 2 | CLI-to-Daemon gRPC Communication | ✅ | 2026-03-09 | Claude | version, get, check all pass via gRPC |
| 3 | Task Lifecycle via gRPC | ✅ | 2026-03-09 | Claude | create→list→info→start(detach)→logs→delete |
| 4 | Embedded Worker Queue Consumption | ✅ | 2026-03-09 | Claude | 3 workers consumed 6 tasks concurrently |
| 5 | Resource Management and Project Isolation via gRPC | ✅ | 2026-03-09 | Claude | apply file/stdin/dry-run + store CRUD + --project isolation + delete project |
