# Design Doc #33: Audit and Reduce expect() Calls (FR-021)

## Status

Implemented (extended to full workspace coverage)

## Context

Rust `expect()` and `unwrap()` calls trigger panics on `None`/`Err`, crashing the thread or process. For a long-running daemon like the orchestrator, a panic in a request handler or state-machine transition means unrecoverable service interruption. FR-021 requested an audit of all such call sites, replacement of high-risk ones with proper error handling, and lint rules to prevent future regressions.

## Decision

Use `deny`-level crate-root lint attributes gated by `cfg(not(test))` to prevent `expect()`, `unwrap()`, and `panic!()` in production code at compile time, while allowing test code to use them freely.

### Key Design Choices

1. **`deny` over `warn`**: The original FR proposed `warn`-level clippy lints. We chose `deny` instead because it provides a compile-time guarantee — no `expect()`/`unwrap()` can be introduced in production code without an explicit `#[allow]` annotation, which serves as a natural review gate.

2. **Crate-root `cfg_attr` over `.clippy.toml`**: Placing the lint attributes directly in each crate root (`lib.rs`, `main.rs`) makes enforcement visible in the source code and avoids toolchain-version sensitivity. A `.clippy.toml` approach would be less discoverable and harder to gate on `cfg(test)`.

3. **`cfg(not(test))` gating**: Test code legitimately uses `expect()`/`unwrap()` for concise assertions. Gating the deny attributes behind `not(test)` avoids verbose `#[allow]` annotations throughout the test suite while still enforcing strict production safety.

4. **No `// SAFETY:` comments needed**: The audit found zero `expect()`/`unwrap()` calls in production code. The only related patterns are safe `.unwrap_or()` calls with fallback defaults, which do not panic and do not require safety annotations.

## Changes

| File | Change |
|------|--------|
| `core/src/lib.rs:15-18` | `#![cfg_attr(not(test), deny(clippy::panic, clippy::unwrap_used, clippy::expect_used))]` |
| `crates/cli/src/main.rs:4-7` | Same deny attribute |
| `crates/daemon/src/main.rs:4-7` | Same deny attribute |
| `crates/orchestrator-config/src/lib.rs` | Same deny attribute (with `test-harness` feature gate) |
| `crates/orchestrator-scheduler/src/lib.rs` | Same deny attribute (with `test-harness` feature gate) |
| `crates/orchestrator-runner/src/lib.rs` | Same deny attribute |
| `crates/orchestrator-security/src/lib.rs` | Same deny attribute |
| `crates/orchestrator-collab/src/lib.rs` | Same deny attribute |
| `crates/orchestrator-client/src/lib.rs` | Same deny attribute |
| `crates/proto/src/lib.rs` | Same deny attribute |
| `crates/gui/src/lib.rs` | Same deny attribute (with `#[allow]` on Tauri `run()`) |

## Audit Summary

| Category | Count | Details |
|----------|-------|---------|
| Production `.expect()` | 0 | None found |
| Production `.unwrap()` | 0 | None found |
| Production `.unwrap_or()` | 3 | `step_pool.rs:37`, `dag.rs:243`, `adaptive.rs:164` — all with safe fallback defaults |
| Test `.expect()` / `.unwrap()` | Many | Allowed by `cfg(not(test))` gating |

## Trade-offs

- **Strictness vs flexibility**: `deny` means any future need for `expect()` in production code requires an explicit `#[allow(clippy::expect_used)]` with justification. This is intentional — it creates a mandatory review checkpoint.
- **Three-lint bundle**: `clippy::panic`, `clippy::unwrap_used`, and `clippy::expect_used` are denied together, providing comprehensive panic-freedom in production paths.
