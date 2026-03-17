---
self_referential_safe: false
---

# QA #72: Audit and Reduce expect() Calls (FR-021)

## Scope

Verify that production code contains no `expect()`/`unwrap()` calls, that deny-level lint attributes prevent future introduction, and that test code remains unaffected.

## Scenarios

### S-01: Zero expect() in production code

**Steps**:
1. Run `grep -rn '\.expect(' core/src/ crates/cli/src/ crates/daemon/src/` excluding test files
2. Filter out `#[cfg(test)]` modules and `#[test]` functions

**Expected**: No matches in production (non-test) code.

### S-02: Zero unwrap() in production code

**Steps**:
1. Run `grep -rn '\.unwrap()' core/src/ crates/cli/src/ crates/daemon/src/` excluding test files
2. Filter out `#[cfg(test)]` modules and `#[test]` functions

**Expected**: No matches in production code. `.unwrap_or()` and `.unwrap_or_else()` are allowed.

### S-03: Deny attribute blocks new expect()

**Steps**:
1. Temporarily add `.expect("test")` to a production function in `core/src/`
2. Run `cargo check -p agent-orchestrator`

**Expected**: Compilation fails with `error: used expect() on a ... value` from `clippy::expect_used`.

### S-04: Test code can still use expect()

**Steps**:
1. Confirm test modules use `.expect()` and `.unwrap()` freely
2. Run `cargo test -p agent-orchestrator`

**Expected**: Tests compile and run. The `cfg(not(test))` gating exempts test code.

### S-05: Workspace tests pass

**Steps**:
1. Run `cargo test --workspace`

**Expected**: All tests pass (excluding pre-existing doctest issues unrelated to this FR).

### S-06: Clippy clean

**Steps**:
1. Run `cargo clippy --workspace --all-targets -- -D warnings`

**Expected**: No warnings or errors related to expect/unwrap usage.

## Result

All scenarios verified on 2026-03-12. Lint enforcement confirmed in all 3 crate roots.

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
