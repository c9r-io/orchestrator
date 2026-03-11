# Orchestrator - Daemon Lifecycle And Runtime Metrics

**Module**: orchestrator
**Status**: Approved
**Related Plan**: FR-005 daemon lifecycle governance and runtime metrics completion
**Related QA**: `docs/qa/orchestrator/60-daemon-lifecycle-runtime-metrics.md`, `docs/qa/orchestrator/53-client-server-architecture.md`
**Created**: 2026-03-11
**Last Updated**: 2026-03-11

## Background

`orchestratord` already supported gRPC over UDS/TCP, embedded workers, and basic PID/socket lifecycle handling. The remaining gap was operational correctness: runtime status fields were incomplete, daemon-executed tasks were not registered in the shared running-task map, and shutdown entry points did not converge on one drain path.

## Goals

- Make daemon shutdown behavior explicit and repeatable.
- Surface trustworthy runtime metrics for uptime, worker activity, and running tasks.
- Prevent new background work from being accepted once shutdown begins.
- Reuse existing scheduler pause/child-process cleanup logic instead of redesigning task execution.

## Non-goals

- Prometheus or external metrics export.
- Distributed worker coordination.
- Changing the existing task/workflow protocol surface beyond additive gRPC fields.

## Scope

- In scope: runtime snapshot state, worker/task counters, shutdown request flag, signal/RPC convergence, draining active tasks, additive Ping/WorkerStatus fields, CLI daemon status output.
- Out of scope: new standalone daemon-status RPC, autoscaling, remote orchestration cluster semantics.

## Interfaces And Data

## API

- `PingResponse` now also returns:
  - `shutdown_requested`
  - `lifecycle_state`
- `WorkerStatusResponse` now also returns:
  - `idle_workers`
  - `running_tasks`
  - `configured_workers`
  - `shutdown_requested`
  - `lifecycle_state`
- `orchestrator debug --component daemon` is a CLI-facing status view built from existing `Ping` + `WorkerStatus` RPCs.

## In-Memory Runtime State

- New shared runtime state attached to `InnerState`
- Tracks:
  - daemon start instant
  - lifecycle state (`serving`, `draining`, `stopped`)
  - shutdown requested flag
  - configured/live/idle/active worker counts
  - running task count

## Key Design

1. Runtime status is stored once in `core`, not recomputed separately in the daemon server and CLI paths.
2. Daemon worker executions now register their `RunningTask` handle in the shared runtime map before calling `run_task_loop`, so shutdown drain can pause real in-flight daemon work.
3. Shutdown is idempotent. Signal and RPC paths both mark the daemon as draining, stop new work intake, and reuse the same task drain behavior.
4. Task drain uses a short grace window first, then falls back to the existing pause/kill-current-child behavior to avoid orphaned process groups.
5. New work RPCs (`TaskStart`, `TaskResume`, `TaskRetry`, and auto-starting `TaskCreate`) are rejected once shutdown begins. Read-side RPCs remain available until the server closes.

## Alternatives And Tradeoffs

- A dedicated daemon-status RPC would have made the CLI simpler, but additive fields on existing RPCs keep compatibility tighter and reduce protocol churn.
- A pure “wait forever for tasks to finish” model would avoid pausing tasks, but it risks hanging daemon shutdown indefinitely on slow or stuck child processes.
- Counting active workers only at the daemon layer would have been less invasive, but it would leave `shutdown_running_tasks()` blind to daemon-owned task runtimes.

## Risks And Mitigations

- Risk: counter skew from early-return worker paths.
  - Mitigation: update worker state on every claim/finish/exit path and keep running-task unregister explicit.
- Risk: shutdown races allow a last task to enqueue during drain.
  - Mitigation: reject new work in the gRPC layer immediately after shutdown is requested.
- Risk: stale documentation still claims daemon waits for all active work to finish.
  - Mitigation: update the existing client/server QA doc and add a dedicated lifecycle-metrics QA doc.

## Observability

- New daemon-level events:
  - `daemon_shutdown_requested`
  - `daemon_shutdown_completed`
  - `worker_state_changed`
  - `task_drain_started`
  - `task_drain_completed`
- `Ping` reports uptime plus daemon lifecycle state.
- `WorkerStatus` reports queue depth plus active/idle/running-task counters.

## Operations / Release

- Default drain grace: 5 seconds before forced task pause.
- Existing worker join timeout remains 30 seconds.
- Operators can inspect current daemon state with `orchestrator debug --component daemon`.
- PID/socket cleanup remains part of the normal daemon exit path.

## Test Plan

- Unit tests: runtime-state counters and `worker_status()` mapping.
- Daemon regression tests: build + package test suite.
- QA scenarios:
  - idle daemon status output
  - live worker/task counters during execution
  - signal-driven drain that pauses a running task
  - clean restart after a drained shutdown

## QA Docs

- `docs/qa/orchestrator/60-daemon-lifecycle-runtime-metrics.md`
- `docs/qa/orchestrator/53-client-server-architecture.md`

## Acceptance Criteria

- `Ping` and `WorkerStatus` expose real runtime values.
- Active daemon tasks are visible to shutdown drain logic.
- Shutdown stops new work intake and pauses remaining long-running tasks after the grace window.
- PID/socket files are cleaned after daemon exit.
- A fresh daemon restart returns to `serving` state with clean worker counts.
