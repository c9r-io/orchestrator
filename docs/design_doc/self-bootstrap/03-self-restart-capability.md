# Self-Bootstrap - Self-Restart Capability

**Module**: self-bootstrap
**Status**: Approved
**Related Plan**: Self-restart capability — rebuild binary, restart process, resume loop with new binary
**Related QA**: `docs/qa/self-bootstrap/07-self-restart-process-continuity.md`
**Created**: 2026-03-05
**Last Updated**: 2026-03-05

## Background

The self-bootstrap workflow already supports self-modification verification (`self_test`), binary snapshots, and auto-rollback. The missing piece: after the orchestrator modifies its own code and passes `self_test`, it should rebuild the binary, restart itself, and continue the loop with the new binary. This completes the self-bootstrapping loop — the orchestrator can evolve itself and immediately run as the evolved version.

## Goals

- Enable the orchestrator to rebuild and restart itself mid-loop after passing self_test
- Preserve exact task/item state across the restart boundary (no item reset)
- Make the restart mechanism independently testable at each layer

## Non-goals

- Hot-reload without process restart (too complex, fragile)
- Multi-binary orchestration (single binary restart only)
- Automatic code modification (that's the `implement` step's job)

## Scope

- In scope: `self_restart` builtin step, `restart_pending` task status, process wrapper loop, priority claiming
- Out of scope: Docker/K8s restart policies, distributed orchestrator instances, watchdog integration (existing watchdog already handles crash recovery)

## Key Design

### 3 Decoupled Layers

```
Layer 1: Builtin Step          Layer 2: Process Wrapper       Layer 3: Task Resumption
┌─────────────────────┐        ┌──────────────────────┐       ┌──────────────────────┐
│ self_restart step    │        │ orchestrator.sh      │       │ task worker / CLI     │
│                      │        │                      │       │                      │
│ 1. cargo build       │  exit  │ while true:          │ start │ claim_next:           │
│ 2. verify binary     │──75──▶ │   run binary         │──────▶│   restart_pending     │
│ 3. snapshot .stable  │        │   if exit==75:       │       │   → running           │
│ 4. status→restart_   │        │     continue (relaunch)│     │   resume at cycle N   │
│    pending           │        │   else:              │       │   loop continues      │
│ 5. process::exit(75) │        │     exit $code       │       │                       │
└─────────────────────┘        └──────────────────────┘       └──────────────────────┘
```

1. **Exit code 75 (EX_TEMPFAIL)**: Used as the restart signal between process and wrapper. Chosen from sysexits.h to avoid collision with standard exit codes.

2. **`restart_pending` status**: A new task status that preserves all item states. Unlike `failed` (which resets unresolved items), `restart_pending` → `running` keeps items exactly as they were pre-restart.

3. **Priority claiming**: `claim_next_pending_task` picks `restart_pending` before `pending` to ensure restart continuity takes priority over new work.

4. **`repeatable: false`**: The `self_restart` step only runs in Cycle 1. Cycle 2 runs the new binary without another restart.

## Alternatives And Tradeoffs

- **Option A (chosen): Exit code + wrapper loop** — Simple, Unix-standard, no IPC needed, wrapper is a thin bash loop
- **Option B: Signal-based (SIGHUP)** — More complex, requires signal handler in async runtime, harder to test
- **Option C: exec() self-replace** — Loses parent supervision, no wrapper to detect crashes

Why we chose A: Simplest to implement, test, and debug. Each layer is independently verifiable. The wrapper script already exists and just needs a loop.

## Risks And Mitigations

- Risk: New binary crashes on startup after restart
  - Mitigation: Wrapper exits with crash code (not 75). Existing watchdog detects and restores `.stable`
- Risk: Build fails mid-loop
  - Mitigation: `on_failure: continue` — loop proceeds with old binary, no restart_pending set
- Risk: Process killed during restart transition
  - Mitigation: Task stays `restart_pending` in DB. Next startup auto-resumes
- Risk: Docker container interprets exit 75 as failure
  - Mitigation: Wrapper loop handles restart internally before container sees exit code

## Observability

- Events: `self_restart_phase` (in-memory, per build/verify/snapshot phase), `self_restart_ready` (persisted, emitted on success)
- Pipeline variable: `self_restart_exit_code` — available to subsequent steps
- Task status transition: `running` → `restart_pending` → `running` (visible in `task info`)
- Events table: `step_finished` with `{"step": "self_restart", "restart": true/false}`
- Wrapper log: `[orchestrator] restart requested (exit 75) — re-launching`

## Operations / Release

- Config: Reuses `ORCH_SELF_TEST_CARGO` env var for testability (mock cargo in tests)
- Binary path: `core/target/release/agent-orchestrator` (same as existing `RELEASE_BINARY_REL`)
- Migration: No DB schema changes — `restart_pending` is a new status value in existing `status TEXT` column
- Compatibility: Backward compatible — old binaries without `self_restart` step simply skip it (unknown step ID would fail validation, but the step is only in the updated workflow YAML)

## Testing And Acceptance

### Unit Tests (implemented)

- `test_execute_self_restart_step_build_fails` — mock cargo failure returns error code, no restart_pending
- `test_execute_self_restart_step_success_returns_exit_restart` — full happy path, verifies EXIT_RESTART and DB status
- `test_exit_restart_constant` — EXIT_RESTART == 75
- `prepare_task_restart_pending_preserves_items` — items NOT reset on restart resume
- `set_task_status_restart_pending_clears_completed_at` — status behavior
- `find_latest_resumable_task_id_includes_restart_pending` — resumability
- `claim_next_prioritizes_restart_pending` — priority ordering

### QA Docs

- `docs/qa/self-bootstrap/07-self-restart-process-continuity.md`

### Acceptance Criteria

- `self_restart` step builds, verifies, snapshots, and exits 75 when build succeeds
- Build failure returns non-zero (not 75), task continues normally
- `orchestrator.sh` relaunches binary on exit 75
- New binary auto-claims `restart_pending` task and resumes at next cycle
- Item statuses are preserved across the restart boundary
