# orchestrator-proto

Protocol Buffers definitions and generated Rust code for the [Agent Orchestrator](https://github.com/c9r-io/orchestrator) gRPC API.

This crate uses `tonic-prost-build` with a vendored `protoc` binary (`protoc-bin-vendored`), so no external protobuf compiler is required.

## Usage

```toml
[dependencies]
orchestrator-proto = "0.1"
```

## Part of the Agent Orchestrator

| Crate | Description |
|-------|-------------|
| [`agent-orchestrator`](https://crates.io/crates/agent-orchestrator) | Core library — scheduling, runner, persistence |
| [`orchestrator-config`](https://crates.io/crates/orchestrator-config) | Configuration models and YAML loading |
| **`orchestrator-proto`** | gRPC/protobuf definitions |
| [`orchestrator-scheduler`](https://crates.io/crates/orchestrator-scheduler) | Scheduler, runner, and prehook engine |
| [`orchestrator-cli`](https://crates.io/crates/orchestrator-cli) | CLI client (`orchestrator` binary) |
| [`orchestratord`](https://crates.io/crates/orchestratord) | Daemon (`orchestratord` binary) |

## License

MIT
