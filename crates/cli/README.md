# orchestrator-cli

CLI client for the [Agent Orchestrator](https://github.com/c9r-io/orchestrator) daemon.

Provides a kubectl-style interface to manage workflows, tasks, agents, and resources via gRPC.

## Install

```bash
cargo install orchestrator-cli
```

## Quick Start

```bash
# Start the daemon first
orchestratord --foreground --workers 2

# Then use the CLI
orchestrator init
orchestrator apply -f manifest.yaml
orchestrator task create --goal "My first QA run"
orchestrator task list
orchestrator task logs <task_id>
```

## Part of the Agent Orchestrator

| Crate | Description |
|-------|-------------|
| [`agent-orchestrator`](https://crates.io/crates/agent-orchestrator) | Core library — scheduling, runner, persistence |
| [`orchestrator-config`](https://crates.io/crates/orchestrator-config) | Configuration models and YAML loading |
| [`orchestrator-proto`](https://crates.io/crates/orchestrator-proto) | gRPC/protobuf definitions |
| [`orchestrator-scheduler`](https://crates.io/crates/orchestrator-scheduler) | Scheduler, runner, and prehook engine |
| **`orchestrator-cli`** | CLI client (`orchestrator` binary) |
| [`orchestratord`](https://crates.io/crates/orchestratord) | Daemon (`orchestratord` binary) |

## License

MIT
