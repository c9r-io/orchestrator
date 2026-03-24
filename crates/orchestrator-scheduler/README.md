# orchestrator-scheduler

Scheduler, runner, and prehook engine for the [Agent Orchestrator](https://github.com/c9r-io/orchestrator).

Manages task lifecycle, phase execution (init, qa, fix, retest, guard), agent process spawning, CEL prehook evaluation, and sandbox enforcement.

## Usage

```toml
[dependencies]
orchestrator-scheduler = "0.1"
```

## Part of the Agent Orchestrator

| Crate | Description |
|-------|-------------|
| [`agent-orchestrator`](https://crates.io/crates/agent-orchestrator) | Core library — scheduling, runner, persistence |
| [`orchestrator-config`](https://crates.io/crates/orchestrator-config) | Configuration models and YAML loading |
| [`orchestrator-proto`](https://crates.io/crates/orchestrator-proto) | gRPC/protobuf definitions |
| **`orchestrator-scheduler`** | Scheduler, runner, and prehook engine |
| [`orchestrator-cli`](https://crates.io/crates/orchestrator-cli) | CLI client (`orchestrator` binary) |
| [`orchestratord`](https://crates.io/crates/orchestratord) | Daemon (`orchestratord` binary) |

## License

MIT
