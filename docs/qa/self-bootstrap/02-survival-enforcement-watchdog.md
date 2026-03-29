---
self_referential_safe: false
self_referential_safe_scenarios: [S1, S2, S3]
---

# Self-Bootstrap - Self-Referential Enforcement & Watchdog

**Module**: self-bootstrap
**Scope**: Validate Layer 3 (self-referential safety enforcement) and Layer 4 (watchdog script) of the survival mechanism
**Scenarios**: 5
**Priority**: High

---

## Background

This document covers the remaining two layers of the self-bootstrap survival mechanism:

- **Layer 3 (Self-Referential Enforcement)**: At task start, the unified self-referential policy runs when `self_referential: true`. Hard errors now cover `checkpoint_strategy == none`, `auto_rollback == false`, and missing builtin `self_test`. `binary_snapshot == false` remains warning-only.
- **Layer 4 (Watchdog Script)**: `scripts/watchdog.sh` polls every 60 seconds, checks binary health via `--help`, and restores `.stable` after 3 consecutive failures.

Key function: `validate_self_referential_safety()` in `core/src/config_load.rs`.
Watchdog script: `scripts/watchdog.sh`.

### Common Preconditions

> **Important**: For self-referential test scenarios, apply a manifest that explicitly
> sets `self_referential: true` on the workspace. Use `apply -f <manifest> --project`
> with a manifest that preserves `self_referential: true`.

```bash
rm -f fixtures/ticket/auto_*.md

QA_PROJECT="qa-enforcement"
orchestrator delete "project/${QA_PROJECT}" --force
```

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| Task starts without `[SELF_REF_UNSAFE]` error despite `checkpoint_strategy: none` | `self_referential` resolved to `false` at runtime because `apply --project` was used or the manifest was applied globally without `--project` | Use `apply -f <manifest> --project <name>` to scope the workspace with `self_referential: true` into the project |

---

## Scenario 1: Self-Referential Workspace Without Checkpoint Strategy Fails

### Preconditions
- None (unit test verification)

### Goal
Verify that `validate_self_referential_safety()` rejects a self-referential workspace with `checkpoint_strategy: none`.

### Steps
1. Run the dedicated unit test:
   ```bash
   cargo test -p agent-orchestrator --lib -- validate_self_referential_safety_errors_without_checkpoint_strategy
   ```
2. Code review — verify the validation logic in `core/src/config_load/validate/tests.rs` (line 503):
   - Test constructs a `WorkflowConfig` with `checkpoint_strategy: None` and `self_referential: true`
   - Calls `validate_self_referential_safety()` and asserts error containing `"self_ref.checkpoint_strategy_required"`

### Expected
- Unit test passes
- Error path confirmed: `checkpoint_strategy: none` on a self-referential workspace produces `self_ref.checkpoint_strategy_required`

---

## Scenario 2: Warning When Auto-Rollback Disabled on Self-Referential Workspace

### Preconditions
- None (unit test verification)

### Goal
Verify that `validate_self_referential_safety()` rejects `auto_rollback: false` on a self-referential workspace with a valid checkpoint strategy.

### Steps
1. Run the dedicated unit test:
   ```bash
   cargo test -p agent-orchestrator --lib -- validate_self_referential_safety_errors_disabled_auto_rollback
   ```
2. Code review — verify the validation logic in `core/src/config_load/validate/tests.rs` (line 761):
   - Test constructs a `WorkflowConfig` with `checkpoint_strategy: GitStash`, `auto_rollback: false`, and `self_referential: true`
   - Calls `validate_self_referential_safety()` and asserts error containing `"self_ref.auto_rollback_required"`

### Expected
- Unit test passes
- Error path confirmed: `auto_rollback: false` on a self-referential workspace produces `self_ref.auto_rollback_required`

---

## Scenario 3: Warning When No Self-Test Step in Self-Referential Workflow

### Preconditions
- None (unit test verification)

### Goal
Verify that `validate_self_referential_safety()` rejects a self-referential workflow without a builtin `self_test` step.

### Steps
1. Run the dedicated unit test:
   ```bash
   cargo test -p agent-orchestrator --lib -- validate_self_referential_safety_errors_missing_self_test
   ```
2. Code review — verify the validation logic in `core/src/config_load/validate/tests.rs` (line 351):
   - Test constructs a `WorkflowConfig` with `checkpoint_strategy: GitTag`, `auto_rollback: true`, but only an `implement` step (no `self_test`)
   - Calls `validate_self_referential_safety()` and asserts error containing `"self_ref.self_test_required"`
3. Verify complementary positive test:
   ```bash
   cargo test -p agent-orchestrator --lib -- validate_self_referential_safety_passes_with_self_test
   ```

### Expected
- Both unit tests pass
- Error path confirmed: missing `self_test` step on a self-referential workflow produces `self_ref.self_test_required`

---

## Scenario 4: Watchdog Detects Healthy Binary and Resets Failure Counter

### Preconditions
- Release binary exists and is functional (`target/release/orchestratord --help` exits 0)
- No watchdog process currently running

### Goal
Verify that the watchdog script correctly identifies a healthy binary and resets its failure counter after recovery.

### Steps
1. Start the watchdog with a short poll interval for testing:
   ```bash
   WATCHDOG_POLL_INTERVAL=2 WATCHDOG_MAX_FAILURES=3 \
     scripts/watchdog.sh > /tmp/watchdog-out.txt 2>&1 &
   WATCHDOG_PID=$!
   ```
2. Wait for at least 2 poll cycles (5 seconds):
   ```bash
   sleep 5
   ```
3. Check watchdog output:
   ```bash
   cat /tmp/watchdog-out.txt
   ```
4. Stop the watchdog:
   ```bash
   kill "$WATCHDOG_PID" 2>/dev/null; wait "$WATCHDOG_PID" 2>/dev/null
   ```

### Expected
- Watchdog starts with message: `[watchdog] started`
- No failure messages in output (binary is healthy)
- No restore actions triggered
- Watchdog shuts down gracefully on SIGTERM with: `[watchdog] shutting down gracefully`

---

## Scenario 5: Watchdog Restores Binary After 3 Consecutive Failures

### Preconditions
- `.stable` binary exists and is valid:
  ```bash
  cp target/release/orchestratord .stable
  ```
- Backup the real binary for restoration after test

### Goal
Verify that the watchdog restores the `.stable` binary after `WATCHDOG_MAX_FAILURES` consecutive health check failures.

### Steps
1. Back up the real binary and replace it with a broken one:
   ```bash
   cp target/release/orchestratord /tmp/orchestratord-backup
   echo "broken" > target/release/orchestratord
   chmod +x target/release/orchestratord
   ```
2. Start the watchdog with short intervals:
   ```bash
   WATCHDOG_POLL_INTERVAL=2 WATCHDOG_MAX_FAILURES=3 WATCHDOG_HEALTH_TIMEOUT=2 \
     scripts/watchdog.sh > /tmp/watchdog-restore.txt 2>&1 &
   WATCHDOG_PID=$!
   ```
3. Wait for at least 4 poll cycles (10 seconds) to allow 3 failures + restore:
   ```bash
   sleep 10
   ```
4. Check watchdog output and verify binary was restored:
   ```bash
   cat /tmp/watchdog-restore.txt
   # Verify the binary works again
   target/release/orchestratord --help >/dev/null 2>&1 && echo "RESTORED" || echo "STILL BROKEN"
   ```
5. Stop watchdog and clean up:
   ```bash
   kill "$WATCHDOG_PID" 2>/dev/null; wait "$WATCHDOG_PID" 2>/dev/null
   # Restore the original binary (in case .stable was different)
   cp /tmp/orchestratord-backup target/release/orchestratord
   rm -f /tmp/orchestratord-backup
   ```

### Expected
- Watchdog output shows 3 consecutive failure messages: `health check failed (1/3)`, `(2/3)`, `(3/3)`
- After 3rd failure: `3 consecutive failures — triggering restore`
- Restore message: `[watchdog] binary restored successfully`
- Binary at `target/release/orchestratord` is now functional (exits 0 on `--help`)
- Failure counter resets to 0 after successful restore
- If binary recovers before 3 failures, output shows: `binary recovered after N failure(s)`

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Self-Referential Without Checkpoint Fails | ✅ | 2026-03-29 | Claude | Unit test passed |
| 2 | Warning When Auto-Rollback Disabled | ✅ | 2026-03-29 | Claude | Unit test passed |
| 3 | Warning When No Self-Test Step | ✅ | 2026-03-29 | Claude | Unit test passed + complementary positive test passed |
| 4 | Watchdog Detects Healthy Binary | SKIPPED | | | Unsafe in self-referential mode (requires watchdog.sh) |
| 5 | Watchdog Restores After 3 Failures | SKIPPED | | | Unsafe in self-referential mode (requires watchdog.sh) |
