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
- `core/build.rs` — Build script capturing git hash and timestamp
- `core/src/cli.rs` — `Version` subcommand and enriched `--version` string
- `core/src/main.rs` — Preflight handler for `version` (no DB required)
- `core/src/scheduler/safety.rs` — Enriched event payloads

---

## Scenario 1: --version Flag Shows Git Hash

### Preconditions
- Binary built with `cargo build --release` from `core/`

### Goal
Verify that `--version` includes the package version and git hash in parentheses.

### Steps
1. Build the release binary:
   ```bash
   cd /Volumes/Yotta/ai_native_sdlc/core
   cargo build --release 2>&1 | tail -5
   ```
2. Run with `--version`:
   ```bash
   ./target/release/agent-orchestrator --version
   ```

### Expected
- Output matches pattern: `orchestrator {VERSION} ({GIT_HASH})`
- Example: `orchestrator 0.1.0 (b74eeaa-dirty)`
- Git hash is 7+ chars, optionally suffixed with `-dirty`

---

## Scenario 2: version Subcommand Plain Text Output

### Preconditions
- Release binary available

### Goal
Verify that `version` subcommand prints version, git hash, and build time in human-readable format, and works without an initialized workspace (preflight).

### Steps
1. Run the version subcommand:
   ```bash
   cd /Volumes/Yotta/ai_native_sdlc/core
   ./target/release/agent-orchestrator version
   ```

### Expected
- Output contains three lines:
  - `Version:    {SEMVER}` (e.g., `0.1.0`)
  - `Git Hash:   {HASH}` (e.g., `b74eeaa-dirty`)
  - `Build Time: {ISO8601}` (e.g., `2026-03-05T07:03:08Z`)
- Build time is valid ISO 8601 UTC timestamp
- No database errors (runs as preflight before DB init)

---

## Scenario 3: version --json Machine-Readable Output

### Preconditions
- Release binary available

### Goal
Verify that `version --json` produces valid JSON with all three fields.

### Steps
1. Run with `--json` flag:
   ```bash
   cd /Volumes/Yotta/ai_native_sdlc/core
   ./target/release/agent-orchestrator version --json 2>/dev/null
   ```
2. Validate JSON structure:
   ```bash
   ./target/release/agent-orchestrator version --json 2>/dev/null | python3 -c "
   import sys, json
   d = json.load(sys.stdin)
   assert 'version' in d, 'missing version key'
   assert 'git_hash' in d, 'missing git_hash key'
   assert 'build_time' in d, 'missing build_time key'
   assert len(d['git_hash']) >= 7, 'git_hash too short'
   assert 'T' in d['build_time'], 'build_time not ISO 8601'
   print('JSON structure valid')
   "
   ```

### Expected
- Output is valid JSON with keys: `version`, `git_hash`, `build_time`
- `version` matches `CARGO_PKG_VERSION` (e.g., `0.1.0`)
- `git_hash` is 7+ chars with optional `-dirty`
- `build_time` is ISO 8601 UTC timestamp ending in `Z`

---

## Scenario 4: Restart Event Payloads Include Build Info

### Preconditions
- Unit test environment available

### Goal
Verify that the `self_restart_ready` and `binary_verification` events in `safety.rs` include `build_git_hash` and `build_timestamp` fields by running the existing unit tests and inspecting the source.

### Steps
1. Verify `self_restart_ready` event payload contains build info:
   ```bash
   cd /Volumes/Yotta/ai_native_sdlc/core
   grep -A8 '"self_restart_ready"' src/scheduler/safety.rs | grep -E 'build_git_hash|build_timestamp'
   ```
2. Verify `binary_verification` event payload contains build info:
   ```bash
   grep -A4 '"binary_verification"' src/scheduler/safety.rs | grep -E 'build_git_hash|build_timestamp'
   ```
3. Run the self_restart and binary verification unit tests to confirm no regressions:
   ```bash
   cargo test --lib -- --test-threads=1 test_execute_self_restart_step_success 2>&1 | tail -5
   cargo test --lib -- --test-threads=1 test_verify_post_restart_binary 2>&1 | tail -10
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
   cd /Volumes/Yotta/ai_native_sdlc/core
   grep 'unwrap_or_else' build.rs | wc -l
   grep '"unknown"' build.rs | wc -l
   ```
2. Verify rerun-if-changed triggers:
   ```bash
   grep 'rerun-if-changed' build.rs
   ```
3. Full test suite passes with build.rs active:
   ```bash
   cargo test --lib 2>&1 | tail -5
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
