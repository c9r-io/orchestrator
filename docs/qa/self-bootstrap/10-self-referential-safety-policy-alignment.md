---
self_referential_safe: true
---

# Self-Bootstrap - Self-Referential Safety Policy Alignment

**Module**: self-bootstrap
**Scope**: Verify the unified self-referential safety contract across config validation, runtime rejection, and policy audit
**Scenarios**: 5
**Priority**: High

---

## Background

FR-003 aligned self-referential safety behavior so every entry point uses the same policy:

- Required: `checkpoint_strategy != none`
- Required: `auto_rollback == true`
- Required: at least one enabled builtin `self_test`
- Recommended-only: `binary_snapshot == true`
- Probe add-on: `self_referential_probe` requires a self-referential workspace and strict probe-only workflow shape

This document validates the shared evaluator through code review and unit tests in `core/src/config_load/validate/tests.rs`.

---

## Scenario 1: `binary_snapshot` Is Warning-Only (Not Blocking)

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm in `core/src/self_referential_policy.rs` that `binary_snapshot: false` produces a diagnostic with `severity: "warning"` and `blocking: false`.

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- validate_self_referential_safety_passes_with_self_test
   cargo test --workspace --lib -- validate_self_referential_safety_passes_with_git_stash
   ```

### Expected

- `binary_snapshot` missing/false produces a warning, not an error
- The summary shows `errors == 0` when only `binary_snapshot` is missing
- Validation passes (returns Ok) despite the warning

---

## Scenario 2: Self-Referential Task Fails When `checkpoint_strategy` Is `none`

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm in `core/src/config_load/validate/` that `checkpoint_strategy: none` with a self-referential workspace triggers a blocking validation error with rule `self_ref.checkpoint_strategy_required`.

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- validate_self_referential_safety_errors_without_checkpoint_strategy
   ```

### Expected

- Validation returns an error for `checkpoint_strategy: none`
- Error includes rule `self_ref.checkpoint_strategy_required`
- The invalid workflow is rejected before persisting to config

---

## Scenario 3: Self-Referential Task Fails When `auto_rollback` Is Disabled

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm `auto_rollback: false` is rejected as a blocking error, not downgraded to a warning.

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- validate_self_referential_safety_errors_disabled_auto_rollback
   ```

### Expected

- Validation returns an error for `auto_rollback: false`
- Error includes rule `self_ref.auto_rollback_required`
- There is no warning-only continuation path for this rule

---

## Scenario 4: Self-Referential Task Fails When Builtin `self_test` Is Missing

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm missing builtin `self_test` step is enforced as a blocking rule.

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- validate_self_referential_safety_errors_missing_self_test
   ```

### Expected

- Validation returns an error when no `self_test` step is present
- Error includes rule `self_ref.self_test_required`
- The invalid workflow is rejected before task creation

---

## Scenario 5: Probe Workflow Rejects Non-Self-Referential Workspace Binding

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm `self_referential_probe` profile requires a self-referential workspace in `core/src/config_load/validate/`.

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- validate_self_referential_safety_rejects_probe_on_non_self_referential_workspace
   cargo test --workspace --lib -- validate_workflow_config_rejects_probe_without_git_tag_checkpoint
   cargo test --workspace --lib -- validate_workflow_config_rejects_probe_without_auto_rollback
   cargo test --workspace --lib -- validate_workflow_config_rejects_probe_with_item_scoped_steps
   cargo test --workspace --lib -- validate_workflow_config_rejects_probe_with_agent_steps
   cargo test --workspace --lib -- validate_workflow_config_rejects_probe_with_strict_phase
   ```

### Expected

- Probe workflow validation rejects non-self-referential workspace binding
- Error includes `self_ref.probe_requires_self_referential_workspace`
- Probe structural constraints (no item-scoped steps, no agent steps) are enforced
- All 6 probe validation tests pass

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | `binary_snapshot` Is Warning-Only | ☑ | 2026-03-18 | Claude | Unit test verified |
| 2 | Fails When `checkpoint_strategy` Is `none` | ☑ | 2026-03-18 | Claude | Unit test verified |
| 3 | Fails When `auto_rollback` Is Disabled | ☑ | 2026-03-18 | Claude | Unit test verified |
| 4 | Fails When Builtin `self_test` Is Missing | ☑ | 2026-03-18 | Claude | Unit test verified |
| 5 | Probe Rejects Non-Self-Referential Workspace | ☑ | 2026-03-18 | Claude | Unit test verified |
