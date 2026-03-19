---
self_referential_safe: true
---

# Error Semantics Governance

**Scope**: Verify FR-014 boundary error classification and gRPC status mapping for task, resource, store, system, and secret-key critical paths.

## Self-Referential Safety

This document is safe for self-referential full-QA runs. All scenarios use Rust test, clippy,
or code-review gates only; no scenario starts, stops, or mutates a live daemon.

## Scenarios

1. Run the focused classifier regression:

   ```bash
   cargo test -p agent-orchestrator error::tests -- --nocapture
   ```

   Expected:

   - missing project lookup maps to `NotFound`
   - missing resumable task maps to `InvalidState`
   - invalid target-file input maps to `UserInput`
   - manifest policy failure maps to `ConfigValidation`
   - secret rotation without active key maps to `InvalidState`

2. Run daemon/server mapping regressions:

   ```bash
   cargo test -p orchestratord server::tests -- --nocapture
   ```

   Expected:

   - `NotFound` maps to gRPC `NOT_FOUND`
   - `InvalidState` maps to gRPC `FAILED_PRECONDITION`
   - `UserInput` maps to gRPC `INVALID_ARGUMENT`

3. Run CLI formatting regressions:

   ```bash
   cargo test -p orchestrator-cli commands::common::tests -- --nocapture
   ```

   Expected:

   - `FailedPrecondition` force-confirmation messages retain the CLI hint
   - `NotFound` messages are not rewritten into generic internal failures

4. Run workspace verification:

   ```bash
   cargo test --workspace --lib
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   ```

   Expected:

   - all tests pass
   - clippy reports no warnings

## Failure Notes

- If a representative error starts mapping to `INTERNAL`, inspect `core/src/error.rs`
- If CLI hint text disappears, inspect `crates/cli/src/commands/common.rs`
- If daemon handlers reintroduce ad-hoc `Status::internal(...)`, inspect `crates/daemon/src/server/`

## Checklist

| # | Scenario | Status | Notes |
|---|----------|--------|-------|
| 1 | Core classifier regression | ✅ | `error::tests` covers task/resource/system/secret category mapping |
| 2 | Daemon gRPC status mapping regression | ✅ | `server::tests` covers `NOT_FOUND`, `FAILED_PRECONDITION`, `INVALID_ARGUMENT` |
| 3 | CLI error rendering regression | ✅ | `commands::common::tests` preserves force hint and not-found message |
| 4 | Workspace verification | ✅ | `cargo test --workspace` and `cargo clippy --workspace --all-targets --all-features -- -D warnings` |
