# Implementation Plan: Expand Unit Test Coverage for validate.rs

**Target File:** `core/src/config_load/validate.rs`
**Current Tests:** 33
**Target Tests:** 45+ (add 12+ new tests)
**Constraint:** Only add tests in `#[cfg(test)] mod tests` section. No production code changes.

---

## Files to Change

### `core/src/config_load/validate.rs`

Add new tests within the existing `#[cfg(test)] mod tests` block (after line 1081). No other files require changes.

---

## Approach

The implementation follows a minimal blast radius approach:

1. **Reuse existing test helpers** from `crate::config_load::tests`:
   - `make_step(id, enabled)` - creates a basic WorkflowStepConfig
   - `make_builtin_step(id, builtin, enabled)` - creates a step with builtin field
   - `make_command_step(id, cmd)` - creates a command step
   - `make_workflow(steps)` - creates a WorkflowConfig with default safety
   - `make_config_with_agent(capability, template)` - creates OrchestratorConfig with agent

2. **Add tests in logical groups** matching the three functions being targeted:
   - Group 1: `ensure_within_root()` tests (4-5 new tests)
   - Group 2: `validate_probe_workflow_shape()` direct tests (5-6 new tests)
   - Group 3: `validate_self_referential_safety()` edge case tests (2-3 new tests)

3. **Follow existing patterns**:
   - Use `assert!(result.is_ok())` / `assert!(result.is_err())` for validation checks
   - Use `expect_err()` + `to_string().contains()` for error message validation
   - Use `std::env::temp_dir()` + `uuid::Uuid::new_v4()` for filesystem tests
   - Clean up temp directories in filesystem tests with `std::fs::remove_dir_all()`

---

## Scope Boundary

### IN Scope

- Add tests for `ensure_within_root()`:
  - Path outside root (e.g., temp dir vs project root)
  - Root equals target (edge case where paths are identical)
  - Nested deep child paths (multi-level subdirectory)
  - Symlink escaping (if symlink points outside root, should reject)

- Add direct tests for `validate_probe_workflow_shape()`:
  - Reject workflow with `chain_steps` populated
  - Reject workflow with `loop.mode != once` (e.g., `LoopMode::Fixed` or `LoopMode::Infinite`)
  - Reject each forbidden phase ID not yet tested: `qa`, `qa_testing`, `fix`, `ticket_fix`, `retest`, `guard`, `test`, `lint`, `self_test`, `smoke_chain`, `ticket_scan`, `init_once`, `loop_guard`
  - Accept valid probe workflow with custom phase names (e.g., `custom_probe_step`)

- Add tests for `validate_self_referential_safety()`:
  - Probe profile on self_referential workspace passes (explicit positive case)
  - Non-self-referential workspace with non-probe profile is NOT checked for checkpoint_strategy (should NOT error)

### OUT of Scope

- No changes to production code in `validate.rs` or any other file
- No changes to test helper functions in `mod.rs`
- No changes to configuration types or structs
- No new test helper abstractions (use existing helpers only)
- No performance or benchmark tests
- No integration tests (all tests remain unit tests)
- No changes to other test files (e.g., `normalize.rs`, `build.rs`)

---

## Test Strategy

### Group 1: `ensure_within_root()` Tests (Target: 4 new tests)

| Test Name | Description | Expected Outcome |
|-----------|-------------|------------------|
| `ensure_within_root_rejects_path_outside_root` | Target path is in /tmp while root is a subdirectory of /tmp | Error: "resolves outside workspace root" |
| `ensure_within_root_accepts_root_equals_target` | Target path is the same as root path | Ok(()) |
| `ensure_within_root_accepts_deeply_nested_child` | Target is 5+ levels deep inside root | Ok(()) |
| `ensure_within_root_rejects_symlink_escaping_root` | Symlink inside root points outside root | Error: "resolves outside workspace root" |

### Group 2: `validate_probe_workflow_shape()` Tests (Target: 6 new tests)

| Test Name | Description | Expected Outcome |
|-----------|-------------|------------------|
| `validate_probe_workflow_shape_rejects_chain_steps` | Step has non-empty `chain_steps` vec | Error: "does not allow chain steps" |
| `validate_probe_workflow_shape_rejects_fixed_loop_mode` | `loop.mode = Fixed` | Error: "requires loop.mode=once" |
| `validate_probe_workflow_shape_rejects_infinite_loop_mode` | `loop.mode = Infinite` | Error: "requires loop.mode=once" |
| `validate_probe_workflow_shape_rejects_forbidden_phase_qa_testing` | Step id = "qa_testing" | Error: "does not allow strict or builtin phases" |
| `validate_probe_workflow_shape_rejects_forbidden_phase_ticket_fix` | Step id = "ticket_fix" | Error: "does not allow strict or builtin phases" |
| `validate_probe_workflow_shape_rejects_forbidden_phase_loop_guard` | Step id = "loop_guard" | Error: "does not allow strict or builtin phases" |
| `validate_probe_workflow_shape_accepts_custom_phase_name` | Step id = "custom_probe_task", all other constraints met | Ok(()) |

### Group 3: `validate_self_referential_safety()` Tests (Target: 2 new tests)

| Test Name | Description | Expected Outcome |
|-----------|-------------|------------------|
| `validate_self_referential_safety_probe_on_self_ref_workspace_passes` | Profile = SelfReferentialProbe, workspace_is_self_referential = true | Ok(()) |
| `validate_self_referential_safety_non_self_ref_without_probe_skipped` | Profile = Standard, workspace_is_self_referential = false, checkpoint_strategy = None | Ok(()) (no validation performed) |

### Test Implementation Details

#### `ensure_within_root_rejects_path_outside_root`

```rust
#[test]
fn ensure_within_root_rejects_path_outside_root() {
    // Create a unique temp root directory
    let root = std::env::temp_dir().join(format!("test-root-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root directory");

    // Use a sibling directory (outside root) as target
    let outside = std::env::temp_dir().join(format!("test-outside-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&outside).expect("create outside directory");

    let result = ensure_within_root(&root, &outside, "test_field");
    assert!(result.is_err(), "should reject path outside root");
    let err = result.expect_err("operation should fail").to_string();
    assert!(err.contains("resolves outside workspace root"), "unexpected error: {}", err);

    std::fs::remove_dir_all(&root).ok();
    std::fs::remove_dir_all(&outside).ok();
}
```

#### `ensure_within_root_accepts_root_equals_target`

```rust
#[test]
fn ensure_within_root_accepts_root_equals_target() {
    let root = std::env::temp_dir().join(format!("test-root-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root directory");

    let result = ensure_within_root(&root, &root, "test_field");
    assert!(result.is_ok(), "root equals target should pass: {:?}", result.err());

    std::fs::remove_dir_all(&root).ok();
}
```

#### `ensure_within_root_accepts_deeply_nested_child`

```rust
#[test]
fn ensure_within_root_accepts_deeply_nested_child() {
    let root = std::env::temp_dir().join(format!("test-root-{}", uuid::Uuid::new_v4()));
    let deep_child = root.join("a").join("b").join("c").join("d").join("e");
    std::fs::create_dir_all(&deep_child).expect("create deep child directory");

    let result = ensure_within_root(&root, &deep_child, "test_field");
    assert!(result.is_ok(), "deeply nested child should pass: {:?}", result.err());

    std::fs::remove_dir_all(&root).ok();
}
```

#### `ensure_within_root_rejects_symlink_escaping_root`

```rust
#[test]
fn ensure_within_root_rejects_symlink_escaping_root() {
    // Create root and outside directories
    let root = std::env::temp_dir().join(format!("test-root-{}", uuid::Uuid::new_v4()));
    let outside = std::env::temp_dir().join(format!("test-outside-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root directory");
    std::fs::create_dir_all(&outside).expect("create outside directory");

    // Create symlink inside root pointing outside
    let symlink = root.join("escape_link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &symlink).expect("create symlink");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&outside, &symlink).expect("create symlink");

    let result = ensure_within_root(&root, &symlink, "test_field");
    assert!(result.is_err(), "symlink escaping root should be rejected");
    let err = result.expect_err("operation should fail").to_string();
    assert!(err.contains("resolves outside workspace root"), "unexpected error: {}", err);

    std::fs::remove_dir_all(&root).ok();
    std::fs::remove_dir_all(&outside).ok();
}
```

#### `validate_probe_workflow_shape_rejects_chain_steps`

```rust
#[test]
fn validate_probe_workflow_shape_rejects_chain_steps() {
    let mut step = make_command_step("implement", "echo probe");
    step.chain_steps = vec![make_command_step("sub", "echo sub")];

    let mut workflow = make_workflow(vec![step]);
    workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
    workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
    workflow.safety.auto_rollback = true;
    workflow.loop_policy.mode = LoopMode::Once;

    let result = validate_probe_workflow_shape(&workflow, "probe");
    assert!(result.is_err());
    let err = result.expect_err("operation should fail").to_string();
    assert!(err.contains("does not allow chain steps"), "unexpected error: {}", err);
}
```

#### `validate_probe_workflow_shape_rejects_fixed_loop_mode`

```rust
#[test]
fn validate_probe_workflow_shape_rejects_fixed_loop_mode() {
    let mut workflow = make_workflow(vec![make_command_step("implement", "echo probe")]);
    workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
    workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
    workflow.safety.auto_rollback = true;
    workflow.loop_policy.mode = LoopMode::Fixed;

    let result = validate_probe_workflow_shape(&workflow, "probe");
    assert!(result.is_err());
    let err = result.expect_err("operation should fail").to_string();
    assert!(err.contains("requires loop.mode=once"), "unexpected error: {}", err);
}
```

#### `validate_probe_workflow_shape_rejects_infinite_loop_mode`

```rust
#[test]
fn validate_probe_workflow_shape_rejects_infinite_loop_mode() {
    let mut workflow = make_workflow(vec![make_command_step("implement", "echo probe")]);
    workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
    workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
    workflow.safety.auto_rollback = true;
    workflow.loop_policy.mode = LoopMode::Infinite;

    let result = validate_probe_workflow_shape(&workflow, "probe");
    assert!(result.is_err());
    let err = result.expect_err("operation should fail").to_string();
    assert!(err.contains("requires loop.mode=once"), "unexpected error: {}", err);
}
```

#### `validate_probe_workflow_shape_rejects_forbidden_phase_qa_testing`

```rust
#[test]
fn validate_probe_workflow_shape_rejects_forbidden_phase_qa_testing() {
    let mut workflow = make_workflow(vec![make_command_step("qa_testing", "echo qa")]);
    workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
    workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
    workflow.safety.auto_rollback = true;

    let result = validate_probe_workflow_shape(&workflow, "probe");
    assert!(result.is_err());
    let err = result.expect_err("operation should fail").to_string();
    assert!(err.contains("does not allow strict or builtin phases"), "unexpected error: {}", err);
    assert!(err.contains("qa_testing"), "error should mention phase name");
}
```

#### `validate_probe_workflow_shape_rejects_forbidden_phase_ticket_fix`

```rust
#[test]
fn validate_probe_workflow_shape_rejects_forbidden_phase_ticket_fix() {
    let mut workflow = make_workflow(vec![make_command_step("ticket_fix", "echo fix")]);
    workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
    workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
    workflow.safety.auto_rollback = true;

    let result = validate_probe_workflow_shape(&workflow, "probe");
    assert!(result.is_err());
    let err = result.expect_err("operation should fail").to_string();
    assert!(err.contains("ticket_fix"), "error should mention phase name");
}
```

#### `validate_probe_workflow_shape_rejects_forbidden_phase_loop_guard`

```rust
#[test]
fn validate_probe_workflow_shape_rejects_forbidden_phase_loop_guard() {
    let mut workflow = make_workflow(vec![make_command_step("loop_guard", "echo guard")]);
    workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
    workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
    workflow.safety.auto_rollback = true;

    let result = validate_probe_workflow_shape(&workflow, "probe");
    assert!(result.is_err());
    let err = result.expect_err("operation should fail").to_string();
    assert!(err.contains("loop_guard"), "error should mention phase name");
}
```

#### `validate_probe_workflow_shape_accepts_custom_phase_name`

```rust
#[test]
fn validate_probe_workflow_shape_accepts_custom_phase_name() {
    let mut workflow = make_workflow(vec![make_command_step("custom_probe_task", "echo custom")]);
    workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
    workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
    workflow.safety.auto_rollback = true;
    workflow.loop_policy.mode = LoopMode::Once;

    let result = validate_probe_workflow_shape(&workflow, "probe");
    assert!(result.is_ok(), "custom phase name should pass: {:?}", result.err());
}
```

#### `validate_self_referential_safety_probe_on_self_ref_workspace_passes`

```rust
#[test]
fn validate_self_referential_safety_probe_on_self_ref_workspace_passes() {
    let mut workflow = make_workflow(vec![make_command_step("implement", "echo probe")]);
    workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
    workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
    workflow.safety.auto_rollback = true;

    let result = validate_self_referential_safety(&workflow, "probe", "self-ref-ws", true);
    assert!(result.is_ok(), "probe profile on self-referential workspace should pass: {:?}", result.err());
}
```

#### `validate_self_referential_safety_non_self_ref_without_probe_skipped`

```rust
#[test]
fn validate_self_referential_safety_non_self_ref_without_probe_skipped() {
    // Non-self-referential workspace with Standard profile should NOT be checked
    // for checkpoint_strategy requirements
    let mut workflow = make_workflow(vec![make_step("implement", true)]);
    workflow.safety.profile = WorkflowSafetyProfile::Standard;
    workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::None; // Would fail if checked

    let result = validate_self_referential_safety(&workflow, "standard-workflow", "plain-ws", false);
    assert!(result.is_ok(), "non-self-ref workspace with standard profile should skip checkpoint check: {:?}", result.err());
}
```

---

## QA Strategy

**Task Classification:** REFACTORING (test-only changes)

**QA Approach:**
- This task adds unit tests only; no production code changes.
- Behavioral equivalence is inherently preserved since no behavior is modified.
- **No new QA documents are needed.** The validation logic being tested is already covered by existing QA scenarios for workflow configuration.

**Verification Steps:**
1. Run `cargo test --package agent-orchestrator --lib config_load::validate::tests` to verify all tests pass
2. Run `cargo llvm-cov --package agent-orchestrator --lib -- config_load::validate` to confirm coverage increase
3. Verify test count: `grep -c '#\[test\]' core/src/config_load/validate.rs` should return 45+

---

## Summary

| Category | New Tests | Target Coverage |
|----------|-----------|-----------------|
| `ensure_within_root()` | 4 | Path escaping, edge cases, symlinks |
| `validate_probe_workflow_shape()` | 6 | Loop modes, chain steps, forbidden phases |
| `validate_self_referential_safety()` | 2 | Profile/workspace combination edge cases |
| **Total** | **12** | **45 tests total (from 33)** |
