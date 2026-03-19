---
self_referential_safe: true
---

# Self-Bootstrap Tests - Scenario 4: Self-Test Step Passes

**Module**: self-bootstrap
**Scenario**: Self-Test Step Passes
**Status**: REWRITTEN — code review + unit test verification
**Test Date**: 2026-03-18
**Tester**: Claude

---

## Goal
Verify that the `self_test` builtin step executes all phases successfully and sets pipeline variables correctly.

---

### Verification Method

Code review + unit test verification. The self_test step execution is fully covered by 5 unit tests in `crates/orchestrator-scheduler/src/scheduler/safety/tests.rs`. No live daemon or task execution required.

### Steps

1. **Code review** — confirm self_test step implementation in `scheduler/safety/` module:
   - Phase 1: `empty_change_check` — aborts when no code changes are present
   - Phase 2: `cargo_check` — runs `cargo check` to verify compilation
   - Phase 3: `cargo_test_lib` — runs `cargo test --lib -p agent-orchestrator -- --skip self_test_survives_smoke_test`
   - Phase 4: `manifest_validate` — runs manifest validation script (if configured)
   - Each phase emits a `self_test_phase` in-memory event with `{"phase": "<name>", "passed": true/false}`
   - On success: `step_finished` event with `exit_code: 0, success: true`
   - Pipeline variables set: `self_test_passed = "true"`, `self_test_exit_code = "0"`

2. **Code review** — confirm failure handling:
   - If `cargo_check` fails, step returns non-zero exit code
   - If `cargo_test_lib` fails, step returns non-zero exit code
   - If `manifest_validate` fails, step returns non-zero exit code
   - Missing manifest script: step still passes (manifest validation is optional)

3. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- test_execute_self_test_step_success_with_manifest_validate
   cargo test --workspace --lib -- test_execute_self_test_step_returns_nonzero_when_cargo_check_fails
   cargo test --workspace --lib -- test_execute_self_test_step_cargo_test_fails
   cargo test --workspace --lib -- test_execute_self_test_step_no_manifest_script
   cargo test --workspace --lib -- test_execute_self_test_step_manifest_validate_fails
   ```

### Expected Results

- All 5 self_test unit tests pass
- Four phases execute in order: `empty_change_check` → `cargo_check` → `cargo_test_lib` → `manifest_validate`
- Pipeline variables are correctly set on success
- Failure in any phase returns non-zero exit code
- Missing manifest script does not block step completion

---

## Checklist

- [x] `self_test` step executes four phases in order (unit test verified)
- [x] Phase set matches implementation: `empty_change_check`, `cargo_check`, `cargo_test_lib`, `manifest_validate` (code review + unit test verified)
- [x] `step_finished` event with `exit_code=0, success=true` on success (unit test verified)
- [x] Pipeline variable `self_test_passed` = `"true"` (unit test verified)
- [x] Pipeline variable `self_test_exit_code` = `"0"` (unit test verified)
- [x] Failure handling works correctly for each phase (unit test verified)
