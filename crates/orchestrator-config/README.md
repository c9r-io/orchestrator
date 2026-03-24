# orchestrator-config

Configuration models and YAML loading for the [Agent Orchestrator](https://github.com/c9r-io/orchestrator).

Handles parsing and validation of YAML manifests (`apiVersion: orchestrator.dev/v2`) including Workspace, Agent, Workflow, StepTemplate, ExecutionProfile, SecretStore, EnvStore, and Trigger resources.

## Usage

```toml
[dependencies]
orchestrator-config = "0.1"
```

## Part of the Agent Orchestrator

| Crate | Description |
|-------|-------------|
| [`agent-orchestrator`](https://crates.io/crates/agent-orchestrator) | Core library — scheduling, runner, persistence |
| **`orchestrator-config`** | Configuration models and YAML loading |
| [`orchestrator-proto`](https://crates.io/crates/orchestrator-proto) | gRPC/protobuf definitions |
| [`orchestrator-scheduler`](https://crates.io/crates/orchestrator-scheduler) | Scheduler, runner, and prehook engine |
| [`orchestrator-cli`](https://crates.io/crates/orchestrator-cli) | CLI client (`orchestrator` binary) |
| [`orchestratord`](https://crates.io/crates/orchestratord) | Daemon (`orchestratord` binary) |

## License

MIT
