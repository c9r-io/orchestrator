# QA: Audit Unsafe Blocks (FR-024)

## Verification Scenarios

### S1: No undocumented unsafe blocks

**Steps:**
1. Run `cargo clippy --workspace --all-targets -- -A warnings -D clippy::undocumented_unsafe_blocks`

**Expected:** Zero errors. All `unsafe` blocks have `// SAFETY:` comments.

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

**Expected:** No `unsafe` blocks in either location.

### S4: Test env-var macros work correctly

**Steps:**
1. Run `cargo test -p agent-orchestrator -- safety`

**Expected:** All safety module tests pass (65 tests).

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
1. Run `cargo check --workspace`
2. Run `cargo test --workspace`

**Expected:** No compilation errors. All tests pass.
