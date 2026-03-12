# Design Doc: Public API Doc Comments (FR-022)

## Overview

Closed the remaining public rustdoc gaps across the orchestrator workspace. The change completes
public API coverage for the `core`, `orchestrator-cli`, and `orchestratord` crates, upgrades all
three crate roots from `warn(missing_docs)` to `deny(missing_docs)`, and removes the temporary
`#[allow(missing_docs)]` suppressions that previously masked CLI and daemon surfaces.

## Scope

- `core/`: completed rustdoc coverage for the remaining scheduling, persistence,
  dynamic-orchestration, CRD, selection, security, and service-facing helpers.
- `crates/cli/`: documented the clap command model down to public enum variants and public fields
  so generated docs and IDE hovers expose the CLI contract cleanly.
- `crates/daemon/`: documented control-plane security and traffic-protection public models,
  including mutual-TLS policy bundles and protection policy shapes.

## Design Decisions

### `deny(missing_docs)` as the steady state

Once the remaining public items were documented, the crate roots were tightened to
`#![deny(missing_docs)]` in:

- `core/src/lib.rs`
- `crates/cli/src/main.rs`
- `crates/daemon/src/main.rs`

This turns future public API drift into a build failure instead of a warning, which is the
actual closure mechanism for FR-022.

### Remove suppressions instead of preserving local escapes

The prior `#[allow(missing_docs)]` annotations in `crates/cli/src/cli.rs`,
`crates/daemon/src/control_plane.rs`, and `crates/daemon/src/protection.rs` were deleted rather
than moved or narrowed. That keeps the enforcement model simple: public API docs are mandatory
everywhere, with no lingering exemption islands.

### Keep rustdoc concise and behavioral

Most newly added comments intentionally stay short and behavioral:

- identify what the public item represents
- clarify field meaning or command/argument semantics
- avoid duplicating implementation details that are likely to drift

This keeps the maintenance burden reasonable while still making generated docs and IDE hovers useful.

## Verification

The closure was validated with the workspace-level gates required by FR-022:

- `cargo check --workspace --all-targets`
- `cargo doc --workspace --no-deps`
- `cargo test --doc --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`

All of the above now pass, and `cargo doc --workspace --no-deps` reports zero
`missing documentation` warnings.
