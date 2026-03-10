# Agent Orchestrator Core

Pure Rust library implementing the orchestrator engine — scheduling, agent selection, workflow execution, and state management.

## Build

```bash
# Build entire workspace (core + daemon + cli + proto)
cargo build --workspace --release

# Build core library only
cargo build -p agent-orchestrator --release
```

## Binaries

| Binary | Crate | Purpose |
|--------|-------|---------|
| `orchestratord` | `crates/daemon` | Daemon — gRPC server + embedded workers |
| `orchestrator` | `crates/cli` | CLI client — lightweight gRPC client |

> **Note:** `core` is a library crate only — it has no binary. The `orchestratord` daemon embeds the core engine, and the `orchestrator` CLI communicates with it over gRPC.

## Run

```bash
# Start daemon (foreground)
orchestratord --foreground --workers 2

# Use CLI client (with daemon running)
orchestrator <command>
```

## Test

```bash
cargo test --workspace
```

## Lint

```bash
cargo clippy --workspace --all-targets -- -D clippy::unwrap_used -D clippy::panic
```

## Linux x86 Check

To validate the Linux GNU `setrlimit` ABI used by [runner.rs](/Volumes/Yotta/ai_native_sdlc/core/src/runner.rs), run:

```bash
./scripts/check-linux-x86-rlimit.sh
```

This installs `i686-unknown-linux-gnu` if needed and type-checks the `RLIMIT_*` to `setrlimit` conversion without requiring the full cross-compilation toolchain.

For a full crate check on `i686-unknown-linux-gnu`, you also need an `i686` C compiler because `rusqlite` builds bundled SQLite C code:

```bash
cargo check -p agent-orchestrator --target i686-unknown-linux-gnu
```

## Config And Data

When run from repo root, runtime paths are:

- DB: `data/agent_orchestrator.db`
- Logs: `data/logs/`
- Daemon socket: `data/orchestrator.sock` (C/S mode)
- Daemon PID: `data/daemon.pid` (C/S mode)

Use `orchestrator apply -f <path>` to initialize config in SQLite.
