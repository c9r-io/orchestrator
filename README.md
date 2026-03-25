# Agent Orchestrator

AI-native SDLC automation — declarative workflow and agent orchestration for the entire software development lifecycle.

```
orchestrator (CLI) ──gRPC/UDS──> orchestratord (daemon)
                                    ├── gRPC server (tonic)
                                    ├── Embedded workers
                                    ├── SQLite persistence
                                    └── Sandbox enforcement
```

## Install

```bash
# Shell script (Linux/macOS)
curl -fsSL https://raw.githubusercontent.com/c9r-io/orchestrator/main/install.sh | sh

# Homebrew
brew install c9r-io/tap/orchestrator

# Cargo
cargo install orchestrator-cli orchestratord
```

## Quick Start

```bash
# Start daemon
orchestratord --foreground --workers 2 &

# Initialize and run
orchestrator init
orchestrator apply -f manifest.yaml
orchestrator task create --goal "My first QA run"
orchestrator task list
orchestrator task logs <task_id>
```

## Key Features

- **Declarative workflows** — YAML manifests with loop control, guard steps, DAG execution
- **Agent orchestration** — capability matching, health scoring, rotation, load balancing
- **CEL prehooks** — dynamic control flow (Run/Skip/Branch/DynamicAdd/Transform)
- **Built-in security** — mTLS, RBAC, sandbox (macOS Seatbelt / Linux namespaces), output redaction
- **Single binary** — Rust, embedded SQLite, no external dependencies

## Documentation

Full documentation: **[docs.c9r.io](https://docs.c9r.io)** (EN / ZH)

- [Quick Start](docs/guide/01-quickstart.md)
- [Resource Model](docs/guide/02-resource-model.md)
- [Workflow Configuration](docs/guide/03-workflow-configuration.md)
- [CEL Prehooks](docs/guide/04-cel-prehooks.md)
- [Advanced Features](docs/guide/05-advanced-features.md)
- [CLI Reference](docs/guide/07-cli-reference.md)
- [Architecture](docs/architecture.md)

## Build from Source

```bash
cargo build --workspace --release
```

Protobuf compiler is vendored — no manual `protoc` installation required.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Feature requests and bug reports via [GitHub Issues](https://github.com/c9r-io/orchestrator/issues).

## License

[MIT](LICENSE)
