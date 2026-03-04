# Trace regression probe `normal-trace` intermittent timeout

**Status**: FAILED
**Module**: orchestrator
**Related QA Doc**: `docs/qa/orchestrator/32-task-trace.md`
**Priority**: Low
**Date**: 2026-03-05

---

## Test Content

The `trace` regression probe group's `normal-trace` scenario creates a task and waits for it to complete within 120 seconds, then checks that the trace output contains cycles, steps, and a completed status.

## Expected Result

Task completes within 120s timeout. Trace output contains cycles, steps, and `completed` status.

## Reproduction Steps

```bash
./scripts/regression/run-cli-probes.sh --group trace --json
```

## Actual Result

```
[WARN]  Task 08b0d257-... did not finish within 120s
[ERROR]   ✗ trace contains cycles (empty)
[ERROR]   ✗ trace status is completed (got: pending)
[ERROR]   ✗ trace has steps (got: no)
[INFO]    ✓ no error-level anomalies
[INFO]    ✓ trace JSON structure valid
```

The task didn't complete within the 120s window. The trace shows `pending` status with no cycles or steps. The other sub-scenario (`low-output-anomaly`) in the same group passed.

## Root Cause Analysis

This is an intermittent timeout issue. The `normal-trace` scenario uses a real workflow execution that depends on agent command execution speed. Under load or when the system is running other tasks concurrently (as was the case during this regression run — multiple probe groups ran sequentially), the task may not complete within the 120s window.

This is not related to the CRD unification refactor — it's a pre-existing test stability issue with the trace probe's timeout budget.

## Suggested Fix

1. Increase the timeout in `probe-trace.sh` from 120s to 180s or 240s
2. Or use a faster/simpler workflow for the normal-trace scenario (e.g., a single-step echo workflow)
3. Or mark this as a known flake with retry logic in the probe runner
