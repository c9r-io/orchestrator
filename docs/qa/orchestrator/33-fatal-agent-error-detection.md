---
self_referential_safe: false
---

# Orchestrator - Fatal Agent Error Detection

**Module**: orchestrator
**Scope**: Regression coverage for provider-side fatal errors that previously surfaced in logs but were still recorded as successful runs
**Scenarios**: 1
**Priority**: High

---

## Background

Some providers can return fatal execution failures in stdout/stderr while the outer CLI still exits `0`.

The orchestrator must treat explicit fatal provider failures as failed runs, including at least:

- `rate-limited`
- provider quota exhaustion (`quota exceeded`, `quota resets in`)
- provider authentication failures

This protects `command_runs.exit_code`, `command_runs.validation_status`, `step_finished.success`, and downstream finalize/loop decisions from false positives.

---

## Scenario 1: Fatal Provider Error Overrides Outer Exit Code 0

### Preconditions
- Latest CLI binary built from current source.
- Runtime initialized.

### Goal
Verify that a phase with outer shell exit code `0` but fatal provider stderr text is persisted as a failed run and does not keep executing downstream steps.

### Steps
1. Run the targeted unit tests:
   ```bash
   cd core
   cargo test fatal_provider_error_marks_run_failed_even_with_zero_exit_code
   cargo test effective_exit_code_maps_validation_failure_to_nonzero
   ```
2. Optionally run the full library suite:
   ```bash
   cargo test --lib
   ```
3. Run a deterministic runtime regression with a temporary workflow that prints `rate-limited` to stderr and exits `0`, then inspect SQLite:
   - `command_runs.exit_code` is non-zero for the fatal step
   - `command_runs.validation_status='failed'`
   - `events.step_finished.success=false`
   - any downstream step after the fatal step is absent from `step_started`

### Expected
- `fatal_provider_error_marks_run_failed_even_with_zero_exit_code` passes:
  - validation result is `failed`
  - error reason is provider rate limit related
- `effective_exit_code_maps_validation_failure_to_nonzero` passes:
  - `validation_status='failed'` with outer exit `0` is remapped to a non-zero persisted exit code
- Runtime regression shows:
  - fatal step is stored with a non-zero exit code
  - `step_finished.success` is `false`
  - downstream steps are not started after the fatal execution-level failure
  - the task does not settle as a false-positive success
- No regression in the broader unit suite.

### Expected Data State
For the runtime regression:
- `command_runs.exit_code != 0` for the fatal step
- `command_runs.validation_status = 'failed'`
- the fatal step's `step_finished.success = false`
- downstream step count in `events` remains `0`
- `tasks.status = 'failed'` (or the owning item remains unresolved)

---

## Checklist

| # | Scenario | Status | Date | Tester | Notes |
|---|----------|--------|------|--------|-------|
| 1 | Fatal Provider Error Overrides Outer Exit Code 0 | ☐ | | | |
