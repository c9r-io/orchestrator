# Agent Orchestrator User Guide

This guide helps you understand and use the Agent Orchestrator — a CLI tool for automating AI-native development lifecycles through intelligent agent orchestration.

## Who Is This For?

- **New users** wanting to get started quickly
- **Workflow authors** designing custom SDLC pipelines
- **Advanced users** exploring CRDs, persistent stores, and self-bootstrap

## Guide Structure

| Chapter | Topic | Difficulty |
|---------|-------|------------|
| [01 - Quick Start](01-quickstart.md) | Run your first workflow in 5 minutes | Beginner |
| [02 - Resource Model](02-resource-model.md) | Workspace, Agent, Workflow, StepTemplate | Beginner |
| [03 - Workflow Configuration](03-workflow-configuration.md) | Steps, scopes, loops, finalize rules | Intermediate |
| [04 - CEL Prehooks](04-cel-prehooks.md) | Dynamic step gating with CEL expressions | Intermediate |
| [05 - Advanced Features](05-advanced-features.md) | CRDs, Persistent Store, Task Spawning, Invariants | Advanced |
| [06 - Self-Bootstrap](06-self-bootstrap.md) | Self-modifying workflows and survival mechanisms | Advanced |
| [07 - CLI Reference](07-cli-reference.md) | Command quick-reference table | Reference |

## Complementary Resources

- **Architecture**: `docs/architecture.md` — system design and internals
- **QA Test Docs**: `docs/qa/` — scenario-based validation (51+ documents)
- **Design Docs**: `docs/design_doc/` — detailed feature designs
- **Fixture Manifests**: `fixtures/manifests/bundles/` — real YAML examples

## Entry Point

All commands use the wrapper script:

```bash
./scripts/orchestrator.sh <command>
```

## Chinese Version / 中文版

See [zh/README.md](zh/README.md) for the full Chinese translation of this guide.

## Binary Entry Point

Or the binary directly:

```bash
./core/target/release/agent-orchestrator <command>
```
