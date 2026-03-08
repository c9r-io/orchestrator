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
# Start daemon (foreground with built-in restart loop)
orchestrator daemon start -f --workers 2

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

## Config And Data

When run from repo root, runtime paths are:

- DB: `data/agent_orchestrator.db`
- Logs: `data/logs/`
- Daemon socket: `data/orchestrator.sock` (C/S mode)
- Daemon PID: `data/daemon.pid` (C/S mode)

Use `orchestrator apply -f <path>` to initialize config in SQLite.
