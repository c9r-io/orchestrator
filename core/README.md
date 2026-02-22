# Agent Orchestrator Core (CLI)

Pure Rust CLI implementation for workflow and agent orchestration.

## Build

```bash
cd core
cargo build --release
```

## Run

```bash
# direct
./target/release/agent-orchestrator <command>

# from repo root
./core/target/release/agent-orchestrator <command>

# wrapper script
./scripts/orchestrator.sh <command>
```

## Test

```bash
cd core
cargo test --lib --bins
```

## Config And Data

When run from repo root, runtime paths are:

- DB: `data/agent_orchestrator.db`
- Logs: `data/logs/`

Use `orchestrator config bootstrap --from <path>` to initialize config in SQLite.
