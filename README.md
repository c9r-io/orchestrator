# Agent Orchestrator

[![CI](https://github.com/c9r-io/orchestrator/actions/workflows/ci.yml/badge.svg)](https://github.com/c9r-io/orchestrator/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/orchestrator-cli.svg)](https://crates.io/crates/orchestrator-cli)
[![docs](https://img.shields.io/badge/docs-docs.c9r.io-blue)](https://docs.c9r.io)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Agent Orchestrator is a **Harness Engineering control plane** for agent-first software delivery. It turns shell-native coding agents such as Claude Code, OpenCode, Codex, and Gemini CLI into governed execution units that can participate in long-running, multi-step engineering workflows through Kubernetes-style YAML manifests.

OpenAI recently described this shift as [Harness Engineering](https://openai.com/index/harness-engineering/): humans spend less time hand-writing code and more time designing environments, specifying intent, and building feedback loops that let agents do reliable work. This project adopts that framing directly. Instead of treating agents as isolated chat tools, it gives them a shared control plane with workflows, triggers, guard steps, secrets, observability, and recovery paths.

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

Run these commands directly, or let your AI coding agent handle it — the CLI is designed for both.

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

## Why This Exists

- **Harness, not wrapper** — The goal is not to invoke an agent once, but to define the environment, workflow, policy, and feedback loop around agent execution.
- **Humans steer, agents execute** — Developers specify goals, constraints, and acceptance criteria; the control plane coordinates execution.
- **Repository-local system of record** — YAML manifests, docs, skills, and QA artifacts become versioned workflow assets that agents can discover and follow.
- **Long-running loops** — The runtime is designed for plan -> implement -> test -> review -> fix cycles that can continue for hours or days, not just one-shot generations.

## Harness Engineering Control Plane

- **CLI** — Machine-parseable output (`-o json`), structured error codes, no interactive prompts
- **Manifests** — Declarative YAML, no imperative code; agents read and apply without interpretation
- **Skills** — `.claude/skills/` provide structured execution plans that agents follow autonomously
- **Showcases** — `docs/showcases/` contain end-to-end execution plans designed for agents to read and execute
- **Any shell agent** — Any tool that accepts a prompt and runs shell commands can be an orchestrator agent under the same control plane

## Integrations

Pre-configured webhook trigger packages for external platforms: **[orchestrator-integrations](https://github.com/c9r-io/orchestrator-integrations)**

Slack, GitHub, LINE — each with per-trigger signature verification, CEL payload filtering, and setup guides. Add new integrations via the `/integration-authoring` skill.

## Key Features

- **Declarative control plane** — YAML manifests with loop control, guard steps, DAG execution, and reusable resource definitions
- **Agent orchestration** — capability matching, health scoring, rotation, and load balancing across heterogeneous shell agents
- **CEL prehooks** — conditional step execution via CEL bool expressions
- **Built-in policy and security** — mTLS, RBAC, sandbox (macOS Seatbelt / Linux namespaces), output redaction, and secret lifecycle management
- **Long-running automation** — task persistence, event streams, trigger-based task creation, and guarded workflow loops
- **Rust workspace** — embedded SQLite, no external runtime dependencies

## Documentation

Full documentation: **[docs.c9r.io](https://docs.c9r.io)** (EN / ZH)

- [Vision](docs/guide/00-vision.md)
- [Quick Start](docs/guide/01-quickstart.md)
- [Resource Model](docs/guide/02-resource-model.md)
- [Workflow Configuration](docs/guide/03-workflow-configuration.md)
- [CEL Prehooks](docs/guide/04-cel-prehooks.md)
- [Advanced Features](docs/guide/05-advanced-features.md)
- [Self-Bootstrap](docs/guide/06-self-bootstrap.md)
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
