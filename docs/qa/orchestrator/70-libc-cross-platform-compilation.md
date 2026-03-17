---
self_referential_safe: false
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
   cargo test -p agent-orchestrator phase_runner::tests::detect_sandbox_violation_detects_cpu_signal -- --nocapture
   ```

   Expected:

   - Test passes on unix platforms
   - Test is excluded from compilation on non-unix targets (verified via cross-compile check)

4. Local workspace verification:

   ```bash
   cargo check --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

   Expected:

   - All three commands pass with zero errors and zero warnings

5. CI cross-compile matrix verification:

   ```bash
   grep -A3 'cross-compile' .github/workflows/ci.yml | head -5
   grep 'target:' .github/workflows/ci.yml
   ```

   Expected:

   - `ci.yml` contains a `cross-compile` job
   - Matrix includes all 5 targets: `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`
   - Each target uses `cargo check --workspace --target <triple>`

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
