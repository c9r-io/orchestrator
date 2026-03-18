---
self_referential_safe: true
---

# Orchestrator - Invariant Constraints (WP04)

**Module**: orchestrator
**Scope**: InvariantConfig, invariant evaluation at checkpoints, protected file detection, on_violation actions
**Scenarios**: 5
**Priority**: High

---

## Background

WP04 adds tamper-proof safety assertions enforced by the engine. Invariants are defined in the `safety.invariants` section of workflow YAML and pinned immutably at task start via `TaskRuntimeContext.pinned_invariants: Arc<Vec<InvariantConfig>>`.

Each invariant can:
- Run a **command** and check its exit code against `expect_exit` (default: 0)
- Evaluate a simple **assertion expression** (`exit_code == N` / `exit_code != N`)
- Detect modifications to **protected files** via `git diff --name-only HEAD`
- Specify **when** to check: `before_cycle`, `after_implement`, `before_restart`, `before_complete`
- Specify **what to do** on violation: `halt` (default), `rollback`, or `warn`

---

## Scenario 1: Invariant Passes — Command Exit Code Check

### Preconditions
- A workflow has an invariant configured:
  ```yaml
  safety:
    invariants:
      - name: "cargo_check"
        description: "Code must compile"
        command: "cargo check --quiet"
        expect_exit: 0
        check_at: [after_implement]
        on_violation: halt
  ```
- The workspace code compiles successfully

### Goal
Verify that a passing invariant does not block execution.

### Steps
1. Configure the invariant as above
2. Run the task through the `after_implement` checkpoint
3. Evaluate invariants at the checkpoint

### Expected
- `InvariantResult.passed = true`
- `InvariantResult.message` is empty
- Execution continues normally past the checkpoint
- No `invariant_violated` status is set

---

## Scenario 2: Invariant Fails — Command Returns Wrong Exit Code

### Preconditions
- A workflow has an invariant:
  ```yaml
  safety:
    invariants:
      - name: "no_todo"
        description: "No TODO comments allowed"
        command: "grep -r TODO src/"
        expect_exit: 1
        check_at: [after_implement]
        on_violation: halt
  ```
- Source code contains TODO comments (grep exits 0, meaning matches found)

### Goal
Verify that an invariant failure with `on_violation: halt` blocks execution.

### Steps
1. Add a TODO comment to source code
2. Run the task through the checkpoint
3. Evaluate invariants

### Expected
- `InvariantResult.passed = false`
- `InvariantResult.message` contains "command exited with 0 (expected 1)"
- `has_halting_violation()` returns `true`
- Task execution should be halted at this checkpoint

---

## Scenario 3: Protected File Modification Detection

### Preconditions
- An invariant has `protected_files: ["Cargo.toml", "src/main.rs"]`
- The workspace is a git repository
- An agent has modified `Cargo.toml` (uncommitted change visible in `git diff`)

### Goal
Verify that protected file modification is detected before the command even runs.

### Steps
1. Configure invariant:
   ```yaml
   safety:
     invariants:
       - name: "protect_cargo"
         description: "Cargo.toml must not be modified"
         protected_files: ["Cargo.toml", "src/main.rs"]
         check_at: [before_complete]
         on_violation: halt
   ```
2. Modify `Cargo.toml` in the workspace (do not commit)
3. Evaluate invariants at `before_complete` checkpoint

### Expected
- `InvariantResult.passed = false`
- `InvariantResult.message` contains "protected file 'Cargo.toml' was modified (pattern: 'Cargo.toml')"
- The command (if any) is NOT executed — protected file check short-circuits
- `has_halting_violation()` returns `true`

---

## Scenario 4: Invariant with Warn-Only Violation

### Preconditions
- An invariant has `on_violation: warn`
- The invariant command fails

### Goal
Verify that `warn` violations are reported but do not halt execution.

### Steps
1. Configure invariant:
   ```yaml
   safety:
     invariants:
       - name: "lint_check"
         description: "Linting warnings"
         command: "false"
         on_violation: warn
         check_at: [after_implement]
   ```
2. Evaluate invariants at the checkpoint

### Expected
- `InvariantResult.passed = false`
- `InvariantResult.on_violation = Warn`
- `has_halting_violation()` returns `false` (warn does not halt)
- `has_rollback_violation()` returns `false`
- Execution continues normally past the checkpoint
- The violation is logged for observability

---

## Scenario 5: Checkpoint Filtering — Invariants Run Only at Configured Points

### Preconditions
- Two invariants configured:
  - `inv_before_cycle`: `check_at: [before_cycle]`
  - `inv_after_implement`: `check_at: [after_implement]`

### Goal
Verify that `evaluate_invariants()` only runs invariants matching the current checkpoint.

### Steps
1. Configure both invariants with passing commands (`true`)
2. Call `evaluate_invariants(invariants, BeforeCycle, workspace_root)`
3. Call `evaluate_invariants(invariants, AfterImplement, workspace_root)`

### Expected
- At `BeforeCycle`: only `inv_before_cycle` is evaluated (1 result)
- At `AfterImplement`: only `inv_after_implement` is evaluated (1 result)
- Invariants with non-matching checkpoints are skipped entirely
- An invariant with `check_at: [before_cycle, after_implement]` would run at both

---

## General Scenario: Invariant Immutability (Pinned at Task Start)

### Steps
1. Task starts — `TaskRuntimeContext.pinned_invariants` is set from SafetyConfig
2. During execution, an agent modifies the workflow YAML to remove an invariant
3. The task continues executing

### Expected
- The removed invariant is still enforced because `pinned_invariants` is an `Arc<Vec<InvariantConfig>>` snapshot
- Changes to workflow YAML do not affect running tasks
- Only newly created tasks pick up invariant changes

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Invariant passes — command exit code check | ✅ | 2026-03-07 | claude | Code path verified: invariant.rs:37-112. Tests: test_passing_invariant, test_invariant_with_expected_exit |
| 2 | Invariant fails — command returns wrong exit code | ✅ | 2026-03-07 | claude | Code path verified: invariant.rs:66-78. Tests: test_failing_invariant, test_has_halting_violation |
| 3 | Protected file modification detection | ✅ | 2026-03-07 | claude | Code path verified: invariant.rs:41-51 (short-circuit), invariant.rs:115-142 (git diff). Tests: file_matches_pattern_exact/prefix_glob/suffix_glob |
| 4 | Invariant with warn-only violation | ✅ | 2026-03-07 | claude | Code path verified: invariant.rs:24-28, loop_engine.rs:724-731. Tests: test_no_halting_violation_when_warn |
| 5 | Checkpoint filtering — invariants run only at configured points | ✅ | 2026-03-07 | claude | Code path verified: invariant.rs:13-15. Tests: test_evaluate_invariants_filters_by_checkpoint |
| G | Invariant immutability (pinned at task start) | ✅ | 2026-03-07 | claude | Code path verified: runtime.rs:293 pins Arc<Vec<InvariantConfig>> at task start. All usage is read-only |
