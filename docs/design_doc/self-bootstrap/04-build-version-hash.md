# Self-Bootstrap - Build Version Hash

**Module**: self-bootstrap
**Status**: Approved
**Related Plan**: Compile-time build info (git hash, timestamp) embedded in binary via `build.rs`, `version` CLI subcommand, enriched restart event payloads
**Related QA**: `docs/qa/self-bootstrap/08-build-version-hash.md`
**Created**: 2026-03-05
**Last Updated**: 2026-03-05

## Background

After implementing `self_restart` with SHA256 binary verification, there was no quick way to check from the CLI **which build** is running — the version, git commit, or build time. Operators debugging self-bootstrap failures need to confirm the running binary matches expectations without comparing SHA256 hashes manually.

## Goals
- Embed compile-time metadata (git hash, build timestamp) into the binary via Cargo build script
- Provide a `version` subcommand with plain-text and JSON output
- Enrich `--version` flag to include git hash
- Add `build_git_hash` and `build_timestamp` to `self_restart_ready` and `binary_verification` events for traceability

## Non-goals
- Runtime version negotiation or compatibility checking
- Semver enforcement or release tagging automation
- Version-gated feature flags

## Scope
- In scope: build.rs script, CLI `version` subcommand, `--version` enrichment, event payload enrichment
- Out of scope: CI/CD release pipeline changes, version bump automation

## Key Design

1. **Build script (`core/build.rs`)**: Captures `BUILD_TIMESTAMP` (UTC ISO 8601 via `date -u`) and `BUILD_GIT_HASH` (`git rev-parse --short HEAD` + `-dirty` suffix) at compile time using `cargo:rustc-env`. Falls back to `"unknown"` on command failure.

2. **Rerun triggers**: `cargo:rerun-if-changed=.git/HEAD` and `cargo:rerun-if-changed=src/` ensure the build script reruns when the commit changes or source files are modified.

3. **`--version` enrichment**: Clap `version` attribute uses `concat!(env!("CARGO_PKG_VERSION"), " (", env!("BUILD_GIT_HASH"), ")")` to produce output like `0.1.0 (abc1234-dirty)`.

4. **`version` subcommand**: Handled as a preflight command (before DB init) so it works without an initialized workspace. Supports `--json` flag for machine-readable output.

5. **Event payload enrichment**: `self_restart_ready` and `binary_verification` events in `safety.rs` now include `build_git_hash` and `build_timestamp` fields, enabling post-mortem correlation of which build was involved in restart cycles.

## Alternatives And Tradeoffs
- **Option A**: Use `built` crate for build metadata — heavier dependency, captures more info than needed
- **Option B**: Manual build.rs with `date` and `git` commands — minimal, no extra dependency, sufficient for our needs
- Why we chose Option B: zero external crate overhead; git and date are universally available in our build environment

## Risks And Mitigations
- Risk: `date` or `git` unavailable in build environment
  - Mitigation: `unwrap_or_else` falls back to `"unknown"` — binary still compiles
- Risk: `-dirty` suffix may vary across developer machines
  - Mitigation: this is informational only; SHA256 verification remains the authoritative binary identity check

## Observability

- Logs: `version` subcommand output (text or JSON) includes version, git_hash, build_time
- Events: `self_restart_ready` payload now includes `build_git_hash`, `build_timestamp`
- Events: `binary_verification` payload now includes `build_git_hash`, `build_timestamp`

## Operations / Release
- Config: No new env vars; build info is compile-time only
- Migration / rollback: No migration needed; build.rs is additive
- Compatibility: Fully backward compatible; old binaries without build info continue to work

## Test Plan
- Unit tests: All 1188 existing `cargo test --lib` pass (no regressions)
- CLI tests: `--version` shows git hash, `version` and `version --json` produce correct output
- Build verification: `cargo build --release` runs build.rs successfully

## QA Docs
- `docs/qa/self-bootstrap/08-build-version-hash.md`

## Acceptance Criteria
- `cargo build --release` — build.rs runs without error
- `./target/release/orchestrator --version` → `0.1.0 (abc1234)`
- `./target/release/orchestrator version` → version, git hash, build time in plain text
- `./target/release/orchestrator version --json` → JSON with version, git_hash, build_time keys
- `cargo test --lib` — all tests pass
