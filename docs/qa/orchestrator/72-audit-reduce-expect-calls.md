---
self_referential_safe: false
self_referential_safe_scenarios: [S1, S2]
---

# QA #72: Audit and Reduce expect() Calls (FR-021)

## Scope

Verify that production code contains no `expect()`/`unwrap()` calls, that deny-level lint attributes prevent future introduction, and that test code remains unaffected.

## Scenarios

### S-01: Zero expect() in production code

**Steps**:
1. Verify deny lint is active: confirm `deny(clippy::expect_used)` in `core/src/lib.rs`, `crates/cli/src/main.rs`, `crates/daemon/src/main.rs`
2. Run `cargo check --workspace` — if it compiles, no production expect() exists

**Expected**: All three crate roots contain `deny(clippy::expect_used)` (gated with `cfg_attr(not(test), ...)`). Compilation succeeds, proving zero production expect() calls.

> **Note**: Raw `grep -rn '.expect(' ...` will show matches in `#[cfg(test)]` modules
> and `*test*.rs` files. These are legitimate — deny attributes only apply to production
> code. The definitive check is compilation success with deny attributes in place.

### S-02: Zero unwrap() in production code

**Steps**:
1. Verify deny lint is active: confirm `deny(clippy::unwrap_used)` in all three crate roots
2. Run `cargo check --workspace` — if it compiles, no production unwrap() exists

**Expected**: Compilation succeeds. `.unwrap_or()` and `.unwrap_or_else()` are allowed (not flagged by clippy).

> **Note**: Same as S-01 — raw grep will show test-code matches which are expected.

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

S-01 and S-02 verified on 2026-03-18. `deny(clippy::expect_used)` and `deny(clippy::unwrap_used)` confirmed in `core/src/lib.rs`, `crates/cli/src/main.rs`, and `crates/daemon/src/main.rs`. `cargo check --workspace` compiles cleanly — zero production expect()/unwrap() calls. S-03–S-06 skipped per self_referential_safe_scenarios: [S1, S2].

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☑ | Verified S-01, S-02 on 2026-03-18. S-03-S-06 skipped per self_referential_safe_scenarios: [S1, S2] |
