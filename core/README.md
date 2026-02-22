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
```

## Test

```bash
cd core
cargo test --lib --bins
```

## Config And Data

When run from repo root, default paths are:

- Config: `orchestrator/config/default.yaml`
- DB: `orchestrator/data/agent_orchestrator.db`
- Logs: `orchestrator/data/logs/`

Use `--config <path>` to override the config file.
