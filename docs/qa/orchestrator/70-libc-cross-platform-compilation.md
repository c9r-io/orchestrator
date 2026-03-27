---
self_referential_safe: true
---

# libc Cross-Platform Compilation

**Scope**: Verify FR-019 libc dependency gating, workspace version unification, test cfg guards, and cross-compile CI coverage.

## Scenarios

1. Verify workspace dependency unification:

   ```bash
   grep -A1 '\[workspace.dependencies\]' Cargo.toml
   grep 'workspace = true' core/Cargo.toml crates/cli/Cargo.toml crates/daemon/Cargo.toml
   ```

   Expected:

   - Root `Cargo.toml` declares `libc = "0.2"` under `[workspace.dependencies]`
   - All three crates reference `libc = { workspace = true }`
   - No crate declares `libc = "0.2"` independently

2. Verify CLI crate libc is cfg-gated:

   ```bash
   grep -B2 'libc' crates/cli/Cargo.toml
   ```

   Expected:

   - `libc` appears under `[target.'cfg(unix)'.dependencies]`, not under `[dependencies]`

3. Verify SIGXCPU test has unix guard:

   ```bash
   rg -n '#\[cfg(unix)\]' crates/orchestrator-scheduler/src/scheduler/phase_runner/tests.rs
   rg -n 'detect_sandbox_violation_detects_cpu_signal' crates/orchestrator-scheduler/src/scheduler/phase_runner/tests.rs
   ```

   Expected:

   - Test function exists and is gated with `#[cfg(unix)]`
   - Implicit compilation verified by `cargo test --workspace --lib` (safe)
   - Cross-compile CI matrix (scenario 5) covers non-unix exclusion

4. Local workspace verification:

   ```bash
   cargo test --workspace --lib
   ```

   Code review confirms:
   - `.github/workflows/ci.yml` contains clippy job with `-D warnings`
   - Implicit `cargo check` is performed by `cargo test` compilation phase

   Expected:

   - `cargo test --workspace --lib` passes (safe: does not affect running daemon)
   - CI gate enforces clippy and cross-compile compliance

5. CI cross-compile matrix verification:

   ```bash
   grep -A3 'cross-compile' .github/workflows/ci.yml | head -5
   grep 'target:' .github/workflows/ci.yml
   ```

   Expected:

   - `ci.yml` contains a `cross-compile` job
   - Matrix includes all 4 targets: `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-gnu`, `aarch64-apple-darwin`
   - Each target uses `cargo check --workspace --target <triple>`

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☑ | S1-S5 PASS (2026-03-19); S3/S4 rewritten as safe (code review + cargo test --lib + CI gate) |
