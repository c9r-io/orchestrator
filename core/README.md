# Agent Orchestrator Core

Pure Rust library implementing the orchestrator engine — scheduling, agent selection, workflow execution, and state management.

## Build

```bash
# Build entire workspace (core + daemon + cli + proto)
cargo build --workspace --release

# Build core only
cargo build -p agent-orchestrator --release
```

## Binaries

| Binary | Crate | Purpose |
|--------|-------|---------|
| `agent-orchestrator` | `core` | Standalone CLI (legacy) |
| `orchestratord` | `crates/daemon` | Daemon — gRPC server + embedded workers |
| `orchestrator` | `crates/cli` | CLI client — lightweight gRPC client |

## Run

### Standalone (legacy)

```bash
./scripts/orchestrator.sh <command>
```

### Client/Server

```bash
# Start daemon
./target/release/orchestratord --foreground --workers 2

# Use CLI client
./target/release/orchestrator <command>
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
