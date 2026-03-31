---
self_referential_safe: true
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
- Rust toolchain available
- Unit tests available: `cargo test fatal_provider_error`, `cargo test effective_exit_code`

### Goal
Verify that a phase with outer shell exit code `0` but fatal provider stderr text is detected as a failed run via unit tests and code review.

### Steps
1. Run the targeted unit tests:
   ```bash
   cargo test --workspace --lib fatal_provider_error_marks_run_failed_even_with_zero_exit_code
   cargo test --workspace --lib fatal_provider_auth_error_marks_run_failed
   cargo test --workspace --lib effective_exit_code_maps_validation_failure_to_nonzero
   ```

2. Review the output validation implementation:
   ```bash
   rg -n "fn validate_output\b|fatal_provider_error|rate.limited|quota" core/src/output_validation.rs
   ```

3. Review effective exit code remapping logic:
   ```bash
   rg -n "effective_exit_code|validation_failure.*nonzero" core/src/output_validation.rs
   ```

### Expected
- `fatal_provider_error_marks_run_failed_even_with_zero_exit_code` passes:
  - validation result is `failed`
  - error reason is provider rate limit related
- `fatal_provider_auth_error_marks_run_failed` passes:
  - authentication failures are also detected
- `effective_exit_code_maps_validation_failure_to_nonzero` passes:
  - `validation_status='failed'` with outer exit `0` is remapped to a non-zero persisted exit code
- Code review confirms fatal provider error patterns are checked before recording exit code

---

## Checklist

| # | Scenario | Status | Date | Tester | Notes |
|---|----------|--------|------|--------|-------|
| 1 | Fatal Provider Error Overrides Outer Exit Code 0 | PASS | 2026-03-31 | QA | All 3 unit tests pass; code review confirms detect_fatal_agent_error called before other validation (line 88 vs 104) |
