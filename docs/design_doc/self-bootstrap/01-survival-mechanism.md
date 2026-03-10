# Self-Bootstrap - Survival Mechanism (4-Layer Protection)

**Module**: self-bootstrap
**Status**: Approved
**Related Plan**: Add 4-layer survival mechanism to self-bootstrap orchestrator: binary checkpoint, self-test acceptance gate, self-referential enforcement, and watchdog script
**Related QA**: `docs/qa/self-bootstrap/01-survival-binary-checkpoint-self-test.md`, `docs/qa/self-bootstrap/02-survival-enforcement-watchdog.md`
**Created**: 2026-02-27
**Last Updated**: 2026-02-27

## Background

When the orchestrator runs in self-referential mode (modifying its own source code), a bad code change can corrupt the very binary that is running the pipeline. Without layered protection, a single broken `implement` step can brick the orchestrator with no recovery path. The survival mechanism adds defense-in-depth to ensure the orchestrator can always recover to a known-good state.

## Goals

- Ensure the release binary can be restored to a known-good version after a bad code change
- Gate code changes with a fast compile-and-test check before expensive QA steps
- Enforce that self-referential workspaces always have adequate safety configuration
- Provide an external watchdog that can restore the binary even if the orchestrator process itself is corrupted

## Non-goals

- Full integration test coverage in the self-test step (kept lightweight: `cargo check` + `cargo test --lib`)
- Watchdog-managed orchestrator process restarts (watchdog only restores the binary file)
- Binary snapshot for non-self-referential workspaces

## Scope

- In scope: Binary snapshot/restore lifecycle, self-test builtin step, self-referential config validation, watchdog shell script
- Out of scope: Container-level health checks, systemd service management, remote binary distribution

## Key Design

### Layer 1: Binary Checkpoint

At cycle start (after git tag checkpoint), the scheduler copies the release binary (`target/release/orchestratord`) to `.stable` in the workspace root. On auto-rollback (after `max_consecutive_failures` exceeded), the `.stable` binary is restored over the live binary.

- Functions: `snapshot_binary()`, `restore_binary_snapshot()` in `core/src/scheduler.rs`
- Config field: `binary_snapshot: bool` in `SafetyConfig` and `SafetySpec`
- Condition: Only runs when `binary_snapshot == true && self_referential == true`

### Layer 2: Self-Test Acceptance Gate

A builtin step (`self_test`) inserted between `implement` and `qa_testing` in the workflow. Runs three phases sequentially:

1. `cargo check --message-format=short` in `core/`
2. `cargo test --lib` in `core/`
3. `orchestrator manifest validate -f docs/workflow/self-bootstrap.yaml` (if binary exists)

If any phase fails, the step returns a non-zero exit code. Pipeline variables `self_test_exit_code` and `self_test_passed` are set for downstream prehook evaluation.

- Step id: `"self_test"` (identified by string, `WorkflowStepType` enum deleted)
- Function: `execute_self_test_step()` in `core/src/scheduler.rs`
- Normalization: `self_test` id auto-sets `builtin: "self_test"` in `config_load.rs`

### Layer 3: Self-Referential Enforcement

At task start, when the workspace has `self_referential: true`, `validate_self_referential_safety()` runs:

- **Hard error**: `checkpoint_strategy == none` causes task start to fail
- **Hard error**: `auto_rollback == false` causes task start to fail
- **Hard error**: No enabled builtin `self_test` step causes task start to fail
- **Warning**: `binary_snapshot == false` is reported but does not block startup

### Layer 4: Watchdog Script

`scripts/watchdog.sh` runs as an independent process, polling every 60 seconds (configurable via `WATCHDOG_POLL_INTERVAL`). Health check runs `$BINARY_PATH --help` with a timeout. After `WATCHDOG_MAX_FAILURES` (default 3) consecutive failures, it copies `.stable` over the live binary.

Environment variables: `BINARY_PATH`, `STABLE_PATH`, `WATCHDOG_POLL_INTERVAL`, `WATCHDOG_MAX_FAILURES`, `WATCHDOG_HEALTH_TIMEOUT`

## Alternatives And Tradeoffs

- Option A: Run full `cargo build --release` in self-test step
  - Pro: Catches more issues; Con: Too slow (minutes) for a gate step
  - Why not: `cargo check` + `cargo test --lib` provides 90% coverage in seconds
- Option B: Use filesystem inotify instead of polling in watchdog
  - Pro: Zero-latency detection; Con: Platform-specific, complex
  - Why not: 60s polling is acceptable for a safety net
- Option C: Embed watchdog in the orchestrator process
  - Pro: Single process; Con: Cannot recover from process corruption
  - Why not: Independent process is the whole point of Layer 4

## Risks And Mitigations

- Risk: `.stable` binary is stale after many successful cycles
  - Mitigation: Binary is re-snapshotted at every cycle start, so it is always from the last successful cycle
- Risk: Watchdog restores binary while orchestrator is mid-write
  - Mitigation: `cp` is atomic at the filesystem level for the final rename; orchestrator reads binary into memory at process start
- Risk: Self-test passes but QA testing fails
  - Mitigation: Expected — self-test is a fast gate, not a replacement for full QA

## Observability

- Events: `binary_snapshot_created`, `binary_snapshot_restored`, `self_test_phase`, `step_finished` (for self_test), `self_referential_policy_checked`
- Pipeline variables: `self_test_exit_code`, `self_test_passed`
- Watchdog stdout: `[watchdog]`-prefixed log lines for health check status, failure counts, restore actions
- Startup rejection: `[SELF_REF_POLICY_VIOLATION]` prefix for hard errors with per-rule detail lines

## Operations / Release

- Config: `binary_snapshot: true` in workflow safety section; `self_referential: true` on workspace
- Migration: No DB changes. New fields have `#[serde(default)]` so existing configs load without modification.
- Compatibility: Fully backward compatible. `binary_snapshot` defaults to `false`; `SelfTest` step type is additive.
- Watchdog: Start with `nohup scripts/watchdog.sh &` or via process supervisor. Graceful shutdown on SIGTERM/SIGINT.

## Test Plan

- Unit tests: `snapshot_binary()` / `restore_binary_snapshot()` file operations, `validate_self_referential_safety()` error/warning paths
- Integration tests: Full cycle with self_test step pass/fail, binary snapshot create/restore event verification
- E2E: Watchdog script health check loop with mock binary

## QA Docs

- `docs/qa/self-bootstrap/01-survival-binary-checkpoint-self-test.md`
- `docs/qa/self-bootstrap/02-survival-enforcement-watchdog.md`

## Acceptance Criteria

- Binary is snapshotted at cycle start when `binary_snapshot: true` and `self_referential: true`
- Binary is restored from `.stable` during auto-rollback
- Self-test step runs `cargo check` + `cargo test --lib` + manifest validate
- Self-test failure sets `self_test_passed: false` and marks item as `self_test_failed`
- Self-referential workspace without checkpoint strategy fails at task start
- Watchdog restores binary after 3 consecutive health check failures
