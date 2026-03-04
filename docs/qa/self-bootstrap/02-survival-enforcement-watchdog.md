# Self-Bootstrap - Self-Referential Enforcement & Watchdog

**Module**: self-bootstrap
**Scope**: Validate Layer 3 (self-referential safety enforcement) and Layer 4 (watchdog script) of the survival mechanism
**Scenarios**: 5
**Priority**: High

---

## Background

This document covers the remaining two layers of the self-bootstrap survival mechanism:

- **Layer 3 (Self-Referential Enforcement)**: At task start, `validate_self_referential_safety()` runs when `self_referential: true`. Hard error if `checkpoint_strategy == None`. Warnings for disabled `auto_rollback` or missing `self_test` step.
- **Layer 4 (Watchdog Script)**: `scripts/watchdog.sh` polls every 60 seconds, checks binary health via `--help`, and restores `.stable` after 3 consecutive failures.

Key function: `validate_self_referential_safety()` in `core/src/config_load.rs`.
Watchdog script: `scripts/watchdog.sh`.

### Common Preconditions

> **Important**: Do NOT use `qa project create` for self-referential test scenarios.
> `qa project create` always sets `self_referential: false` on the new workspace,
> which causes validation to never trigger. Instead, use `apply --project` to apply
> manifests directly into the project scope, preserving `self_referential: true`.

```bash
rm -f fixtures/ticket/auto_*.md

QA_PROJECT="qa-enforcement"
./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --force
```

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| Task starts without `[SELF_REF_UNSAFE]` error despite `checkpoint_strategy: none` | `self_referential` resolved to `false` at runtime because `qa project create` was used or the manifest was applied globally without `--project` | Use `apply -f <manifest> --project <name>` to scope the workspace with `self_referential: true` into the project |

---

## Scenario 1: Self-Referential Workspace Without Checkpoint Strategy Fails

### Preconditions
- Common Preconditions applied

### Goal
Verify that starting a task on a self-referential workspace with `checkpoint_strategy: none` produces a hard error and the task does not start.

### Steps
1. Apply a manifest with `self_referential: true` but `checkpoint_strategy: none`:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: unsafe-ws
   spec:
     root_path: "."
     qa_targets: [docs/qa]
     ticket_dir: docs/ticket
     self_referential: true
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: unsafe-workflow
   spec:
     steps:
       - id: plan
         type: plan
         required_capability: plan
         enabled: true
     safety:
       max_consecutive_failures: 3
       auto_rollback: true
       checkpoint_strategy: none
   ```
   ```bash
   ./scripts/orchestrator.sh apply -f /tmp/unsafe-manifest.yaml --project "${QA_PROJECT}"
   ```
2. Create a task and attempt to start it:
   ```bash
   ./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --workflow unsafe-workflow --goal "test unsafe"
   ```

### Expected
- Task start fails with error message containing `[SELF_REF_UNSAFE]`
- Error message includes: "self_referential but checkpoint_strategy is 'none'"
- Task status transitions to `failed` (the task is marked `running` before validation, then `failed` when validation rejects it)

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| Task status is `failed` instead of `pending` | By design: `task start` sets status to `running` before loading runtime context; on validation failure it transitions to `failed` | This is expected behavior — verify the error message is correct |

---

## Scenario 2: Warning When Auto-Rollback Disabled on Self-Referential Workspace

### Preconditions
- Common Preconditions applied

### Goal
Verify that a warning is emitted (not a hard error) when `auto_rollback: false` on a self-referential workspace with a valid checkpoint strategy.

### Steps
1. Apply a manifest with `self_referential: true`, `checkpoint_strategy: git_tag`, but `auto_rollback: false`:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: warn-ws
   spec:
     root_path: "."
     qa_targets: [docs/qa]
     ticket_dir: docs/ticket
     self_referential: true
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: warn-workflow
   spec:
     steps:
       - id: plan
         type: plan
         required_capability: plan
         enabled: true
     safety:
       max_consecutive_failures: 3
       auto_rollback: false
       checkpoint_strategy: git_tag
   ```
   ```bash
   ./scripts/orchestrator.sh apply -f /tmp/warn-manifest.yaml --project "${QA_PROJECT}"
   ```
2. Create a task and start it, capturing stderr:
   ```bash
   ./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --workflow warn-workflow --goal "test warn" 2>/tmp/warn-stderr.txt
   cat /tmp/warn-stderr.txt
   ```

### Expected
- Task starts successfully (no hard error — no `[SELF_REF_UNSAFE]`)
- Stderr contains: `WARN` and `auto_rollback is disabled`
- Task proceeds past safety check (may later fail due to missing agents in test env — that is expected)

---

## Scenario 3: Warning When No Self-Test Step in Self-Referential Workflow

### Preconditions
- Common Preconditions applied

### Goal
Verify that a warning is emitted when a self-referential workspace workflow has no `self_test` step.

### Steps
1. Apply a manifest with `self_referential: true`, valid safety config, but no `self_test` step:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: notest-ws
   spec:
     root_path: "."
     qa_targets: [docs/qa]
     ticket_dir: docs/ticket
     self_referential: true
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: notest-workflow
   spec:
     steps:
       - id: plan
         type: plan
         required_capability: plan
         enabled: true
       - id: implement
         type: implement
         required_capability: implement
         enabled: true
     safety:
       max_consecutive_failures: 3
       auto_rollback: true
       checkpoint_strategy: git_tag
   ```
   ```bash
   ./scripts/orchestrator.sh apply -f /tmp/notest-manifest.yaml --project "${QA_PROJECT}"
   ```
2. Create a task and start it, capturing stderr:
   ```bash
   ./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --workflow notest-workflow --goal "test no self_test" 2>/tmp/notest-stderr.txt
   cat /tmp/notest-stderr.txt
   ```

### Expected
- Task starts successfully (no hard error — no `[SELF_REF_UNSAFE]`)
- Stderr contains: `WARN` and `has no self_test step`
- Task proceeds past safety check (may later fail due to missing agents in test env — that is expected)

---

## Scenario 4: Watchdog Detects Healthy Binary and Resets Failure Counter

### Preconditions
- Release binary exists and is functional (`core/target/release/agent-orchestrator --help` exits 0)
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
  cp core/target/release/agent-orchestrator .stable
  ```
- Backup the real binary for restoration after test

### Goal
Verify that the watchdog restores the `.stable` binary after `WATCHDOG_MAX_FAILURES` consecutive health check failures.

### Steps
1. Back up the real binary and replace it with a broken one:
   ```bash
   cp core/target/release/agent-orchestrator /tmp/agent-orchestrator-backup
   echo "broken" > core/target/release/agent-orchestrator
   chmod +x core/target/release/agent-orchestrator
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
   core/target/release/agent-orchestrator --help >/dev/null 2>&1 && echo "RESTORED" || echo "STILL BROKEN"
   ```
5. Stop watchdog and clean up:
   ```bash
   kill "$WATCHDOG_PID" 2>/dev/null; wait "$WATCHDOG_PID" 2>/dev/null
   # Restore the original binary (in case .stable was different)
   cp /tmp/agent-orchestrator-backup core/target/release/agent-orchestrator
   rm -f /tmp/agent-orchestrator-backup
   ```

### Expected
- Watchdog output shows 3 consecutive failure messages: `health check failed (1/3)`, `(2/3)`, `(3/3)`
- After 3rd failure: `3 consecutive failures — triggering restore`
- Restore message: `[watchdog] binary restored successfully`
- Binary at `core/target/release/agent-orchestrator` is now functional (exits 0 on `--help`)
- Failure counter resets to 0 after successful restore
- If binary recovers before 3 failures, output shows: `binary recovered after N failure(s)`

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Self-Referential Without Checkpoint Fails | ☐ | | | |
| 2 | Warning When Auto-Rollback Disabled | ☐ | | | |
| 3 | Warning When No Self-Test Step | ☐ | | | |
| 4 | Watchdog Detects Healthy Binary | ☐ | | | |
| 5 | Watchdog Restores After 3 Failures | ☐ | | | |
