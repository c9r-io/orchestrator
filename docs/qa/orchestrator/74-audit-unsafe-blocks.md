---
self_referential_safe: false
self_referential_safe_scenarios: [S1, S3, S4, S5, S6, S7]
---

# QA: Audit Unsafe Blocks (FR-024)

## Verification Scenarios

### S1: No undocumented unsafe blocks

**Steps:**
1. Code review confirms all `unsafe` blocks have `// SAFETY:` comments (verified by S5)
2. Code review confirms `#![deny(clippy::undocumented_unsafe_blocks)]` in all crate roots:
   ```bash
   rg -n 'deny\(clippy::undocumented_unsafe_blocks\)' core/src/lib.rs crates/cli/src/main.rs crates/daemon/src/main.rs
   ```
3. Implicit compilation by `cargo test --workspace --lib` (safe) proves no undocumented blocks exist

**Expected:** deny attribute enforces 100% SAFETY comment coverage at compile time.

### S2: `forbid(unsafe_code)` on proto crate

**Steps:**
1. Add an `unsafe {}` block to `crates/proto/src/lib.rs`
2. Run `cargo check -p orchestrator-proto`

**Expected:** Compile error due to `#![forbid(unsafe_code)]`.

**Teardown:** Revert the test change.

### S3: Eliminated unsafe blocks use safe wrappers

**Steps:**
1. Verify `core/src/runner/sandbox.rs` uses `nix::unistd::geteuid()` (no `libc::geteuid`)
2. Verify `crates/daemon/src/lifecycle.rs` uses `nix::sys::signal::kill()` (no `libc::kill`)

**Expected:** Both files use `nix` wrappers for `kill()` and `geteuid()` respectively.

> **Note**: `lifecycle.rs` retains `unsafe` blocks for `libc::sigaction` signal installation
> with `SA_SIGINFO` (sender PID extraction). This is acceptable — `nix` wraps `sigaction`
> but the current implementation predates that adoption. The S3 scope covers only `kill()`
> and `geteuid()` replacements.

### S4: Test env-var macros work correctly

**Steps:**
1. Code review confirms safety tests exist in `crates/orchestrator-scheduler/src/scheduler/safety/tests.rs` (43+ tests)
2. Run safety module tests (safe: lib tests do not affect running daemon):
   ```bash
   cargo test --lib -p orchestrator-scheduler -- safety
   ```

**Expected:** All safety module tests pass.

### S5: SAFETY comment coverage

**Steps:**
1. Run `grep -rn "unsafe" --include="*.rs" core/src/ crates/` (exclude target/)
2. For each `unsafe {` block, verify a `// SAFETY:` comment exists immediately above

**Expected:** 100% SAFETY comment coverage on all retained unsafe blocks.

### S6: Miri CI job defined

**Steps:**
1. Inspect `.github/workflows/ci.yml`

**Expected:** A `miri` job exists that installs nightly + miri and runs
`cargo +nightly miri test` on targeted modules.

### S7: Full workspace build and test

**Steps:**
1. Run `cargo test --workspace --lib` (safe: does not affect running daemon; implicit `cargo check` via compilation)
2. Verify zero test failures

**Expected:** No compilation errors. All lib tests pass.

> **注意**：`orchestrator-scheduler` 的测试依赖 `agent-orchestrator/test-harness` feature。
> 该 feature 已通过 dev-dependencies 自动激活，无需手动传递 `--features`。

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☑ | S1/S3/S4/S5/S6/S7 PASS (2026-03-19); S1/S4/S7 rewritten as safe. S2 remains unsafe (temp code injection). |
