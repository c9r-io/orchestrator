# Orchestrator - Performance IO and Queue Optimization

**Module**: orchestrator
**Status**: Approved
**Related Plan**: Reduce write amplification and log IO overhead, and harden detached queue concurrency with atomic claim semantics and parallel workers.
**Related QA**: `docs/qa/orchestrator/22-performance-io-queue-optimizations.md`
**Created**: 2026-02-23
**Last Updated**: 2026-02-23

## Background And Goals

## Background

Performance audit and code review identified three hotspots in scheduler runtime:

- `command_runs` structured fields were persisted by a second DB write after initial insert.
- phase validation and `task logs --tail` had full-file read patterns on large stdout/stderr logs.
- detached worker queue used non-atomic pending fetch + status transition and only single-consumer polling behavior by default.

## Goals

- Persist each `command_runs` execution record in one insert payload.
- Bound phase output read volume and preserve validation observability with truncation marker.
- Implement true tail behavior for `task logs --tail` using reverse seek scanning.
- Add atomic pending claim and multi-worker consumption path.
- Keep CLI UX stable and backward-compatible (`task worker start|stop|status`, `task logs`).

## Non-goals

- Replace SQLite with external queue or DB.
- Add distributed worker coordination across hosts.
- Redesign workflow semantics or message-bus contracts.

## Scope And User Experience

## Scope

- In scope:
  - `scheduler.run_phase` single-persist command run payload.
  - bounded output reads for stdout/stderr validation path.
  - reverse-seek log tail helper for CLI task log view.
  - `claim_next_pending_task` transactional queue claim API.
  - `task worker start --workers N` parallel consumer option.
  - QA performance baseline scripts under `docs/qa/script/`.

- Out of scope:
  - full write-coordinator abstraction for all DB writes.
  - cross-process event subscription/wakeup channel.

## UI Interactions (If Applicable)

- Not applicable (CLI only).

## Interfaces And Data

## API

- No HTTP/gRPC API changes.

## CLI Surface

- Added worker option:
  - `task worker start --workers <N>`
- Existing command compatibility preserved.

## Database Changes

- No schema change.
- Access pattern changes:
  - `command_runs` structured fields are inserted with the base run row in one write.
  - pending task claim now uses immediate transaction for atomicity.

## Key Design And Tradeoffs

## Key Design

1. Keep execution choke point in `run_phase` and move all run persistence fields into a single insert payload.
2. Introduce bounded output read helper to cap phase output read size (default 256KiB) and annotate truncation.
3. Replace full-file tail with reverse scan by chunks to avoid read amplification on large logs.
4. Add `claim_next_pending_task` with `BEGIN IMMEDIATE` + conditional update (`status='pending'`) to enforce single winner.
5. Allow multiple worker consumer threads in one process, while respecting existing global semaphore limit.

## Alternatives And Tradeoffs

- Option A: Keep full reads and rely on faster disks.
  - Pros: minimal code change.
  - Cons: scales poorly with large logs and concurrent tasks.
- Option B: Add external queue service.
  - Pros: stronger queue semantics and observability.
  - Cons: operational overhead and architectural expansion.
- Chosen: incremental SQLite-safe optimization.
  - Pros: low migration risk, high immediate gain.
  - Cons: cross-process wake-up still poll-based.

## Risks And Mitigations

- Risk: bounded reads may hide early stdout content needed for debugging.
  - Mitigation: truncation marker in persisted output and original log files retained.
- Risk: multi-worker mode increases lock contention.
  - Mitigation: atomic claim with immediate transaction and global task semaphore bound.
- Risk: behavior drift in CLI docs/tests.
  - Mitigation: dedicated QA regression doc and script baselines.

## Observability And Operations

## Observability

- Existing events remain source of truth (`scheduler_enqueued`, `phase_output_published`, validation events).
- Added measurable perf probes via scripts:
  - `docs/qa/script/test-worker-throughput.sh`
  - `docs/qa/script/test-log-tail-latency.sh`

## Operations / Release

- No new env vars.
- Rollout:
  1. Run unit/integration tests.
  2. Execute QA doc 22 scenarios.
  3. Run throughput/tail baseline scripts and record benchmark deltas.
- Rollback:
  - Revert scheduler/repository/worker patches as one release unit.

## Testing And Acceptance

## Test Plan

- Unit tests:
  - atomic claim single-winner behavior.
- Integration tests:
  - worker CLI path with `--workers`.
  - command run persistence coverage.
- QA docs:
  - `docs/qa/orchestrator/22-performance-io-queue-optimizations.md`
  - `docs/qa/orchestrator/20-structured-output-worker-scheduler.md`
  - `docs/qa/orchestrator/19-scheduler-repository-refactor-regression.md`

## Acceptance Criteria

- Strict phases no longer require follow-up DB update for structured run fields.
- Large phase outputs are bounded and explicitly marked when truncated.
- `task logs --tail` retrieves suffix lines without full-file read behavior.
- Under parallel workers, one pending task is claimed by at most one consumer.
- Running task concurrency remains bounded by global semaphore limit.
