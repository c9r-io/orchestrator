# Design Doc: Audit Unsafe Blocks (FR-024)

## Overview

Audited all `unsafe` blocks in the project codebase, eliminated unnecessary ones,
documented retained ones with `// SAFETY:` comments, and established CI guardrails
to prevent future undocumented unsafe code.

## Audit Results

### Inventory

20 `unsafe` blocks found across the workspace (FR estimated ~35; delta due to
counting at block granularity rather than line granularity).

| Category | Count | Disposition |
|----------|-------|-------------|
| Eliminated (safe replacement) | 2 | Replaced with `nix` crate safe wrappers |
| FFI — retained with SAFETY | 6 | libc syscalls (signal, kill, setrlimit, pre_exec) |
| Test — retained with macros | 12 | `std::env::set_var`/`remove_var` via `test_set_env!`/`test_remove_env!` |

### Eliminated Blocks

1. **`core/src/runner/sandbox.rs`** — `libc::geteuid()` replaced with `nix::unistd::geteuid().as_raw()`.
2. **`crates/daemon/src/lifecycle.rs`** — `libc::kill(pid, 0)` replaced with `nix::sys::signal::kill(Pid, None).is_ok()`.

### Retained Production Blocks (6)

All retained blocks are libc FFI with no safe alternative:

- `crates/cli/src/main.rs` — `libc::signal(SIGPIPE, SIG_DFL)` (POSIX signal restore)
- `crates/cli/src/commands/debug.rs` — `Vec::set_len()` (memory probe diagnostic)
- `core/src/runner/resource_limits.rs:20` — `cmd.pre_exec()` (inherently unsafe tokio API)
- `core/src/runner/resource_limits.rs:67` — `libc::setrlimit()` (FFI)
- `core/src/runner/spawn.rs:191` — `libc::kill(-pid, SIGKILL)` (process group kill)
- `core/src/scheduler/runtime.rs:195` — `libc::kill(-pid, SIGKILL)` (process group kill)

### Test Blocks (12)

All 12 in `core/src/scheduler/safety/tests.rs`. Consolidated into `test_set_env!` /
`test_remove_env!` macros with centralized `// SAFETY:` documentation. Tests hold
`ENV_LOCK` (tokio Mutex) for exclusive environment access.

## CI Guardrails

### `clippy::undocumented_unsafe_blocks` (deny)

Added `#![deny(clippy::undocumented_unsafe_blocks)]` to all three main crates
(`core`, `cli`, `daemon`). Any future `unsafe` block without a `// SAFETY:` comment
is a compile error under `cargo clippy -- -D warnings`.

### `#![forbid(unsafe_code)]`

Added to `crates/proto` — a pure protobuf codegen crate that should never contain
unsafe code. Compile-time enforced.

### Miri CI Job

Added `miri` job to `.github/workflows/ci.yml` targeting the `resource_limits`
module in the core crate. Scope is limited because most unsafe blocks call libc
FFI functions that Miri cannot execute.

## Design Decisions

### `nix` crate for safe wrappers

Used `nix` (no-default-features, minimal feature flags) to eliminate `geteuid()` and
`kill(pid, 0)`. The `nix` crate is a well-established, idiomatic Rust wrapper around
POSIX APIs. Feature-gated under `cfg(unix)` like the existing `libc` dependency.

### Macro consolidation for test env vars

Rather than adding the `serial_test` crate, used macros with the existing `ENV_LOCK`
pattern. This avoids a new dependency while centralizing the SAFETY argument.

### Miri scope limitations

Miri cannot execute foreign function calls (libc syscalls), which constitute all
production unsafe blocks. Miri also flags `std::env::set_var` as potential UB in
multi-threaded tokio test contexts. The practical Miri coverage targets pure-Rust
logic adjacent to unsafe code. FFI correctness is validated by the existing
integration and unit test suites.
