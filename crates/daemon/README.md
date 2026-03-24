# orchestratord

Daemon process for the [Agent Orchestrator](https://github.com/c9r-io/orchestrator) — hosts the gRPC control plane, embedded workers, task scheduling, and agent process management.

## Install

```bash
cargo install orchestratord
```

## Quick Start

```bash
# Start in foreground with 2 workers
orchestratord --foreground --workers 2

# Or run as a background daemon
orchestratord start
```

## Part of the Agent Orchestrator

| Crate | Description |
|-------|-------------|
| [`agent-orchestrator`](https://crates.io/crates/agent-orchestrator) | Core library — scheduling, runner, persistence |
| [`orchestrator-config`](https://crates.io/crates/orchestrator-config) | Configuration models and YAML loading |
| [`orchestrator-proto`](https://crates.io/crates/orchestrator-proto) | gRPC/protobuf definitions |
| [`orchestrator-scheduler`](https://crates.io/crates/orchestrator-scheduler) | Scheduler, runner, and prehook engine |
| [`orchestrator-cli`](https://crates.io/crates/orchestrator-cli) | CLI client (`orchestrator` binary) |
| **`orchestratord`** | Daemon (`orchestratord` binary) |

## License

MIT
