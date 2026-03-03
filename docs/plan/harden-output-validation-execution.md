# Implementation Plan: Harden output_validation.rs

## Overview

This plan addresses four hardening tasks for `core/src/output_validation.rs`:
1. Remove dead branch in `parse_build_errors_from_text`
2. Add unit test for warning-level parsing
3. Expand `is_strict_phase` to include additional SDLC phases
4. Add unit tests for new strict phases

---

## Files to Change

### `core/src/output_validation.rs`

| Location | Change Type | Description |
|----------|-------------|-------------|
| Lines 164-170 | Refactor | Remove redundant inner if/else; set `BuildErrorLevel::Error` directly |
| Line 30-32 | Extend | Add 5 new phases to `is_strict_phase` match pattern |
| Tests section | Add | Add `build_phase_parses_warnings` test |
| Tests section | Add | Add tests for new strict phases (`qa_testing`, `qa_doc_gen`, `ticket_fix`, `align_tests`, `doc_governance`) |

---

## Approach

### Task 1: Fix Dead Branch in `parse_build_errors_from_text`

**Current code (lines 164-170):**
```rust
if line.starts_with("error") {
    let message = line.to_string();
    let level = if line.starts_with("error") {  // <-- ALWAYS TRUE
        BuildErrorLevel::Error
    } else {
        BuildErrorLevel::Warning                // <-- UNREACHABLE
    };
```

**Analysis:** The outer condition `line.starts_with("error")` is already satisfied, so the inner `if line.starts_with("error")` is always true. The `else` branch setting `BuildErrorLevel::Warning` is dead code.

**Fix:** Simplify to:
```rust
if line.starts_with("error") {
    errors.push(BuildError {
        file: None,
        line: None,
        column: None,
        message: line.to_string(),
        level: BuildErrorLevel::Error,
    });
```

**Blast radius:** None. This is a pure code cleanup with no behavioral change — error lines already produce `BuildErrorLevel::Error`.

### Task 2: Add Warning-Level Parsing Test

Add a test verifying that lines starting with `warning:` produce `BuildErrorLevel::Warning`.

**Test structure:**
```rust
#[test]
fn build_phase_parses_warnings() {
    let stderr = "warning: unused variable `x`\n --> src/lib.rs:5:13";
    let outcome = validate_phase_output("build", Uuid::new_v4(), "agent", 0, "", stderr)
        .expect("validation should return outcome");
    assert_eq!(outcome.output.build_errors.len(), 1);
    assert_eq!(outcome.output.build_errors[0].level, BuildErrorLevel::Warning);
    assert_eq!(outcome.output.build_errors[0].file.as_deref(), Some("src/lib.rs"));
    assert_eq!(outcome.output.build_errors[0].line, Some(5));
}
```

### Task 3: Expand `is_strict_phase`

**Current code (line 30-32):**
```rust
fn is_strict_phase(phase: &str) -> bool {
    matches!(phase, "qa" | "fix" | "retest" | "guard")
}
```

**New code:**
```rust
fn is_strict_phase(phase: &str) -> bool {
    matches!(
        phase,
        "qa" | "fix" | "retest" | "guard"
            | "qa_testing" | "qa_doc_gen" | "ticket_fix" | "align_tests" | "doc_governance"
    )
}
```

**Blast radius:** Low. These phases will now require JSON stdout when executed. Agents for these phases already produce JSON output per architecture spec. The change enforces what was already expected.

### Task 4: Add Unit Tests for New Strict Phases

Add parameterized tests verifying each new strict phase rejects non-JSON output:

```rust
#[test]
fn new_strict_phases_require_json() {
    let new_phases = ["qa_testing", "qa_doc_gen", "ticket_fix", "align_tests", "doc_governance"];
    for phase in new_phases {
        let outcome = validate_phase_output(phase, Uuid::new_v4(), "agent", 0, "plain-text", "")
            .expect("validation should return outcome");
        assert_eq!(outcome.status, "failed", "phase {} should require JSON", phase);
        assert!(outcome.error.is_some());
    }
}

#[test]
fn new_strict_phases_accept_json() {
    let new_phases = ["qa_testing", "qa_doc_gen", "ticket_fix", "align_tests", "doc_governance"];
    let json_output = r#"{"confidence":0.9}"#;
    for phase in new_phases {
        let outcome = validate_phase_output(phase, Uuid::new_v4(), "agent", 0, json_output, "")
            .expect("validation should return outcome");
        assert_eq!(outcome.status, "passed", "phase {} should accept JSON", phase);
    }
}
```

---

## Scope Boundary

### IN Scope

- Remove dead branch in `parse_build_errors_from_text` (lines 164-170)
- Add test `build_phase_parses_warnings`
- Add `qa_testing`, `qa_doc_gen`, `ticket_fix`, `align_tests`, `doc_governance` to `is_strict_phase`
- Add tests for new strict phases
- Preserve existing `is_build_phase` and `is_test_phase` logic unchanged
- Preserve existing strict phases (`qa`, `fix`, `retest`, `guard`)
- Maintain current error-line parsing behavior (error lines produce `BuildErrorLevel::Error`)

### OUT of Scope

- Changes to `is_build_phase` or `is_test_phase`
- Changes to `parse_build_errors_from_text` beyond removing the dead inner if/else
- Changes to `parse_test_failures_from_text`
- Changes to `detect_fatal_agent_error`
- Changes to `ValidationOutcome` struct
- Changes to `validate_phase_output` main logic flow
- Adding new phases to `is_build_phase` or `is_test_phase`
- Integration tests or E2E tests
- Documentation updates
- Any behavioral changes to error parsing beyond the dead code removal

---

## Test Strategy

### Unit Tests Required

| Test Name | Purpose |
|-----------|---------|
| `build_phase_parses_warnings` | Verify warning lines produce `BuildErrorLevel::Warning` |
| `new_strict_phases_require_json` | Verify new strict phases reject non-JSON stdout |
| `new_strict_phases_accept_json` | Verify new strict phases accept valid JSON stdout |

### Existing Tests to Verify Still Pass

| Test Name | Purpose |
|-----------|---------|
| `strict_phase_requires_json` | Existing strict phase behavior |
| `strict_phase_accepts_json` | Existing strict phase behavior |
| `build_phase_parses_errors` | Error parsing behavior unchanged |
| `test_phase_parses_failures` | Test failure parsing unchanged |
| `non_build_phase_has_no_build_errors` | Non-build phase behavior unchanged |
| `fatal_provider_error_marks_run_failed_even_with_zero_exit_code` | Fatal error detection unchanged |
| `fatal_provider_auth_error_marks_run_failed` | Auth error detection unchanged |

### Test Commands

```bash
# Run all tests in the module
cargo test -p agent-orchestrator output_validation

# Run with verbose output
cargo test -p agent-orchestrator output_validation -- --nocapture
```

---

## QA Strategy

**Task Classification:** REFACTORING

This is a refactoring task with these characteristics:
- Removing dead code (unreachable branch)
- Extending an existing validation pattern to additional phases
- No new features or behavior changes

**QA Approach:** Unit tests are sufficient. Behavioral equivalence is verified through:
1. Existing tests confirm unchanged behavior for error/warning parsing
2. New tests confirm warning parsing works correctly
3. New tests confirm extended strict phase enforcement works

**QA Documentation:** NOT required. This is a pure refactoring task with no user-facing behavior changes. The changes are internal code quality improvements that are fully covered by unit tests.

---

## Execution Checklist

- [ ] Remove dead inner if/else in `parse_build_errors_from_text`
- [ ] Add `build_phase_parses_warnings` test
- [ ] Extend `is_strict_phase` with 5 new phases
- [ ] Add `new_strict_phases_require_json` test
- [ ] Add `new_strict_phases_accept_json` test
- [ ] Run `cargo test -p agent-orchestrator output_validation`
- [ ] Verify all tests pass
