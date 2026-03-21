---
self_referential_safe: true
---

# Self-Bootstrap - Build Version Hash

**Module**: self-bootstrap
**Scope**: Compile-time build info in binary (build.rs), `version` subcommand (text + JSON), `--version` enrichment, and restart event payload enrichment
**Scenarios**: 5
**Priority**: Medium

---

## Background

The build version hash feature embeds compile-time metadata into the binary via `core/build.rs`:
- `BUILD_TIMESTAMP` — UTC ISO 8601 build time
- `BUILD_GIT_HASH` — short git commit hash with `-dirty` suffix for uncommitted changes

This enables quick identification of which build is running via `--version` or the `version` subcommand, and enriches `self_restart_ready` / `binary_verification` events with build provenance.

Key files:
- `crates/cli/build.rs` — Build script capturing git hash and timestamp for the CLI binary
- `crates/cli/src/cli.rs` — `Version` subcommand and enriched `--version` string
- `crates/cli/src/main.rs` — Preflight handler for `version` (no daemon required)
- `core/src/scheduler/safety.rs` — Enriched event payloads

---

## Scenario 1: --version Flag Shows Git Hash

### Preconditions
- Rust toolchain available

### Goal
Verify that `--version` output is enriched with git hash via `build.rs` compile-time embedding.

### Steps
1. Code review — verify `build.rs` captures git hash and injects it via `env!()`:
   ```bash
   rg -n "BUILD_GIT_HASH|git.*rev-parse" crates/cli/build.rs
   ```
2. Code review — verify `clap` `version` string includes the git hash:
   ```bash
   rg -n "version.*BUILD_GIT_HASH|long_version" crates/cli/src/cli.rs | head -5
   ```
3. Implicit compilation by `cargo test --workspace --lib` proves `build.rs` env vars resolve correctly.

### Expected
- `build.rs` runs `git rev-parse --short HEAD` and sets `BUILD_GIT_HASH` env var
- `cli.rs` uses `env!("BUILD_GIT_HASH")` in the clap version string
- Output pattern: `orchestrator {VERSION} ({GIT_HASH})` where git hash is 7+ chars, optionally suffixed with `-dirty`

---

## Scenario 2: version Subcommand Plain Text Output

### Preconditions
- Rust toolchain available

### Goal
Verify that `version` subcommand formats output with Version/Git Hash/Build Time lines and runs as preflight (no daemon required).

### Steps
1. Code review — verify `Version` subcommand handler in main.rs runs before DB init:
   ```bash
   rg -n "Version|preflight|version" crates/cli/src/main.rs | head -10
   ```
2. Code review — verify plain text format includes three fields:
   ```bash
   rg -n "Version:|Git Hash:|Build Time:" crates/cli/src/cli.rs | head -5
   ```

### Expected
- `version` subcommand is handled as preflight (before daemon connection)
- Output contains three lines: `Version:`, `Git Hash:`, `Build Time:`
- Build time is ISO 8601 UTC from `BUILD_TIMESTAMP` env var

---

## Scenario 3: version --json Machine-Readable Output

### Preconditions
- Rust toolchain available

### Goal
Verify that `version --json` produces valid JSON with version, git_hash, build_time keys.

### Steps
1. Code review — verify JSON serialization in version handler:
   ```bash
   rg -n "json|serde_json|version.*git_hash.*build_time" crates/cli/src/cli.rs | head -10
   ```
2. Code review — verify `--json` flag on Version subcommand:
   ```bash
   rg -n "json.*bool|json.*flag" crates/cli/src/cli.rs | head -5
   ```

### Expected
- JSON output contains keys: `version`, `git_hash`, `build_time`
- `version` matches `CARGO_PKG_VERSION`
- `git_hash` from `BUILD_GIT_HASH` env var (7+ chars with optional `-dirty`)
- `build_time` from `BUILD_TIMESTAMP` env var (ISO 8601 UTC)

---

## Scenario 4: Restart Event Payloads Include Build Info

### Preconditions
- Unit test environment available

### Goal
Verify that the `self_restart_ready` and `binary_verification` events in `safety.rs` include `build_git_hash` and `build_timestamp` fields by running the existing unit tests and inspecting the source.

### Steps
1. Verify `self_restart_ready` event payload contains build info:
   ```bash
   rg -n 'build_git_hash|build_timestamp' crates/orchestrator-scheduler/src/scheduler/safety.rs | head -10
   ```
2. Verify `binary_verification` event payload contains build info:
   ```bash
   rg -A2 '"binary_verification"' crates/orchestrator-scheduler/src/scheduler/safety.rs | head -10
   ```
3. Run the self_restart and binary verification unit tests to confirm no regressions:
   ```bash
   cargo test -p orchestrator-scheduler --lib -- test_execute_self_restart_step_success test_verify_post_restart_binary 2>&1 | tail -10
   ```

### Expected
- `self_restart_ready` payload includes `"build_git_hash": env!("BUILD_GIT_HASH")` and `"build_timestamp": env!("BUILD_TIMESTAMP")`
- Both `binary_verification` payloads (verified=true and verified=false) include `build_git_hash` and `build_timestamp`
- All unit tests pass without regression

---

## Scenario 5: build.rs Fallback and Rerun Triggers

### Preconditions
- Repository with `core/build.rs` present

### Goal
Verify that build.rs has proper fallback handling and rerun-if-changed triggers.

### Steps
1. Verify fallback to "unknown" on command failure:
   ```bash
   rg -n 'unwrap_or_else|"unknown"' crates/cli/build.rs
   ```
2. Verify rerun-if-changed triggers:
   ```bash
   rg -n 'rerun-if-changed' crates/cli/build.rs
   ```
3. Full test suite passes with build.rs active:
   ```bash
   cargo test --workspace --lib 2>&1 | tail -5
   ```

### Expected
- Two `unwrap_or_else` calls (one for date, one for git)
- Two `"unknown"` fallback strings
- `rerun-if-changed` entries for `.git/HEAD` and `src/`
- All lib tests pass (1188+ tests, 0 failures)

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | --version Flag Shows Git Hash | PASS | 2026-03-05 | claude | `orchestrator 0.1.0 (b74eeaa-dirty)` |
| 2 | version Subcommand Plain Text Output | PASS | 2026-03-05 | claude | Three lines: Version, Git Hash, Build Time; ISO 8601 UTC |
| 3 | version --json Machine-Readable Output | PASS | 2026-03-05 | claude | Valid JSON with version, git_hash, build_time keys |
| 4 | Restart Event Payloads Include Build Info | PASS | 2026-03-05 | claude | build_git_hash + build_timestamp in all 3 event emissions; 4 unit tests pass |
| 5 | build.rs Fallback and Rerun Triggers | PASS | 2026-03-05 | claude | 2 fallbacks, 2 rerun triggers, 1188 tests pass |
