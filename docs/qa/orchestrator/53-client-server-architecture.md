---
self_referential_safe: true
---

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
- Daemon: `./target/release/orchestratord [--foreground] [--bind addr] [--workers N]` (`--insecure-bind` requires `dev-insecure` Cargo feature)
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
- Secure TCP: `--bind 0.0.0.0:50051` with auto-bootstrapped mTLS
- Unsafe TCP (development only, requires `dev-insecure` Cargo feature): `--insecure-bind 0.0.0.0:50051`
- PID file at `$APP_ROOT/data/daemon.pid`

For dedicated transport/auth regression coverage, see `docs/qa/orchestrator/58-control-plane-security.md`.

---

## Scenario 1: Daemon Startup and Shutdown (Code Review + Unit Test)

### Preconditions
- Rust toolchain available

### Goal
Verify daemon lifecycle logic: UDS socket creation, PID file management, and clean shutdown on SIGTERM — via code review and implicit compilation.

### Steps
1. Code review — verify PID file creation and cleanup in lifecycle module:
   ```bash
   rg -n "daemon.pid|write_pid|remove_pid|cleanup_pid" crates/daemon/src/lifecycle.rs | head -10
   ```
2. Code review — verify UDS socket creation and cleanup:
   ```bash
   rg -n "orchestrator.sock|bind_uds|remove_socket|cleanup_socket" crates/daemon/src/server.rs crates/daemon/src/lifecycle.rs | head -10
   ```
3. Code review — verify SIGTERM handler triggers graceful shutdown:
   ```bash
   rg -n "SIGTERM|signal_handler|graceful_shutdown|drain" crates/daemon/src/lifecycle.rs | head -10
   ```
4. Implicit compilation and daemon module coherence verified by workspace test:
   ```bash
   cargo test --workspace --lib 2>&1 | tail -5
   ```

### Expected
- `lifecycle.rs` writes PID to `data/daemon.pid` on startup and removes it on shutdown.
- `server.rs` binds UDS socket at `data/orchestrator.sock` and removes it on shutdown.
- SIGTERM handler sets a shutdown flag, triggers task drain grace period (up to 5 s), and supervisor wait (up to 30 s).
- All workspace lib tests pass (implicit compilation proves daemon module compiles correctly).

---

## Scenario 2: CLI-to-Daemon gRPC Communication

### Preconditions
- Daemon is running (foreground or background).
- Config applied via the current CLI or previous daemon session.

### Goal
Verify CLI client connects to daemon over UDS and basic RPC round-trips work.

### Steps
1. Test version/ping:
   ```bash
   ./target/release/orchestrator version
   ```
2. Test resource listing:
   ```bash
   ./target/release/orchestrator get workspaces
   ./target/release/orchestrator get agents -o json
   ./target/release/orchestrator get workflows -o yaml
   ```
3. Test debug:
   ```bash
   ./target/release/orchestrator debug --component config
   ```
4. Test preflight check:
   ```bash
   ./target/release/orchestrator check -o json
   ```

### Expected
- `version` shows client version and daemon version (from Ping RPC).
- `get workspaces/agents/workflows` returns the same data across repeated CLI/daemon sessions.
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
1. Create task (no-start):
   ```bash
   TASK_ID=$(./target/release/orchestrator task create --name "grpc-test" --goal "test" --project cs-qa --no-start 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   echo "Created: $TASK_ID"
   ```
2. List tasks:
   ```bash
   ./target/release/orchestrator task list -o json
   ```
3. Get task info:
   ```bash
   ./target/release/orchestrator task info "$TASK_ID" -o json
   ```
4. Start task (queue it for the embedded worker):
   ```bash
   ./target/release/orchestrator task start "$TASK_ID"
   ```
5. Wait for worker to finish, then check status:
   ```bash
   sleep 10
   ./target/release/orchestrator task info "$TASK_ID" -o json
   ```
6. View logs:
   ```bash
   ./target/release/orchestrator task logs "$TASK_ID" --tail 20
   ```
7. Clean up:
   ```bash
   ./target/release/orchestrator task delete "$TASK_ID" --force
   ```

### Expected
- Task is created with status `created` or `pending`.
- `task list` shows the task in JSON output.
- `task start` enqueues the task (status `pending`).
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
- Daemon running with at least 1 embedded worker.
- A QA project created for isolation: `./target/release/orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project cs-qa`

### Goal
Verify embedded daemon workers consume pending tasks concurrently and atomically.

### Steps
1. Create 6 tasks in detach mode:
   ```bash
   for i in $(seq 1 6); do
     ./target/release/orchestrator task create --name "batch-$i" --goal "batch test $i" --project cs-qa
   done
   ```
2. Monitor worker progress:
   ```bash
   sleep 5
   ./target/release/orchestrator task list -o json
   ```
3. Wait for completion and verify all reached terminal status:
   ```bash
   sleep 30
   ./target/release/orchestrator task list -o json | grep -c '"status"'
   ```

### Expected
- All 6 tasks transition from `pending` through `running` to `completed` or `failed`.
- Workers log `claimed task` and `task finished` for each task.
- No task is executed more than once (atomic claim via `claim_next_pending_task`).

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
1. Apply manifest from file:
   ```bash
   ./target/release/orchestrator apply -f fixtures/manifests/bundles/output-formats.yaml
   ```
2. Apply from stdin:
   ```bash
   cat fixtures/manifests/bundles/output-formats.yaml | ./target/release/orchestrator apply -f -
   ```
3. Dry-run apply:
   ```bash
   ./target/release/orchestrator apply -f fixtures/manifests/bundles/output-formats.yaml --dry-run
   ```
4. Get and describe resources:
   ```bash
   ./target/release/orchestrator get workspace/default -o yaml
   ./target/release/orchestrator describe workspace/default -o yaml
   ```
5. Store CRUD:
   ```bash
   ./target/release/orchestrator store put qa-store test-key '{"value":42}'
   ./target/release/orchestrator store get qa-store test-key
   ./target/release/orchestrator store list qa-store -o json
   ./target/release/orchestrator store delete qa-store test-key
   ```
6. Apply manifest to project scope and verify isolation:
   ```bash
   ./target/release/orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project iso-test
   ./target/release/orchestrator get agents --project iso-test
   ./target/release/orchestrator describe agent/mock_echo --project iso-test
   ```
7. Create and list tasks in project scope:
   ```bash
   ./target/release/orchestrator task create --name "iso-task" --goal "isolation test" --project iso-test --workflow qa_only --no-start
   ./target/release/orchestrator task list --project iso-test -o json
   ```
8. Delete project resource and reset project:
   ```bash
   ./target/release/orchestrator delete agent/mock_echo --force --project iso-test
   ./target/release/orchestrator delete project/iso-test --force
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
| 1 | Daemon Startup and Shutdown | ✅ | 2026-03-21 | Claude | Rewritten: code review of lifecycle.rs/server.rs + implicit compilation via cargo test |
| 2 | CLI-to-Daemon gRPC Communication | ✅ | 2026-03-20 | Claude | version, get workspaces/agents/workflows, debug config, check all pass via gRPC |
| 3 | Task Lifecycle via gRPC | ✅ | 2026-03-20 | Claude | create(workflow explicit)→list→info→start→qa_passed(128 items)→logs→delete |
| 4 | Embedded Worker Queue Consumption | ✅ | 2026-03-20 | Claude | 6 batch tasks: created→pending→completed(128 items each, 0 failed) |
| 5 | Resource Management and Project Isolation via gRPC | ✅ | 2026-03-20 | Claude | apply file/stdin/dry-run + store CRUD + --project isolation + delete project |
