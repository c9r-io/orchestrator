# Agent Orchestrator User Guide

This guide helps you understand and use Agent Orchestrator as a **Harness Engineering control plane** for agent-first software delivery.

## Who Is This For?

- **New users** wanting to get started quickly
- **Workflow authors** designing reusable agent harnesses and SDLC pipelines
- **Platform builders** operationalizing shell-based agents across teams
- **Advanced users** exploring CRDs, persistent stores, and self-bootstrap

## Guide Structure

| Chapter | Topic | Difficulty |
|---------|-------|------------|
| [00 - Vision](00-vision.md) | Understand the Harness Engineering control plane direction | Beginner |
| [01 - Quick Start](01-quickstart.md) | Run your first workflow in 5 minutes | Beginner |
| [02 - Resource Model](02-resource-model.md) | Workspace, Agent, Workflow, StepTemplate | Beginner |
| [Non-code Workspaces and Global Skills](non-code-workspace.md) | Task workspaces, isolated HOME, file-sharing ceiling, Slack pilot | Beginner to Operations |
| [Coordination Tools](coordination-tools.md) | Typed daemon-owned testing, item, ticket, and dynamic-item coordination | Intermediate to Advanced |
| [03 - Workflow Configuration](03-workflow-configuration.md) | Steps, scopes, loops, finalize rules | Intermediate |
| [04 - CEL Prehooks](04-cel-prehooks.md) | Dynamic step gating with CEL expressions | Intermediate |
| [05 - Advanced Features](05-advanced-features.md) | CRDs, Persistent Store, Task Spawning, Invariants | Advanced |
| [06 - Self-Bootstrap](06-self-bootstrap.md) | Self-modifying workflows and survival mechanisms | Advanced |
| [07 - CLI Reference](07-cli-reference.md) | Command quick-reference for the C/S runtime | Reference |
| [Error Codes](error-codes.md) | Bracketed machine error codes: meaning, trigger, remedy | Reference |
| [Agent Process Console 用户指南（中文）](zh/08-agent-process-console.md) | Daily Attention triage, process recovery, session takeover, sources, and operations | Beginner to Advanced |
| [Agent Process Console v1 Operations](agent-process-console-v1-operations.md) | Upgrade, rollout, stop-loss, rollback, and disaster restore | Operations |
| [用 Slack Reaction 创建 Skill 任务](slack-reaction-skill-automation.md) | Slack app、SecretStore、双 badge 自动化、诊断、升级与回滚 | Beginner to Operations |
| [使用官方 Orchestrator Slack App 一键连接 Workspace](slack-managed-connections.md) | Managed Connection、OAuth、Gateway、转移、诊断与运维 | Beginner to Operations |
| [为每个 Slack Workspace 创建独立的 Orchestrator App](slack-dedicated-app-provisioning.md) | Dedicated App、Configuration Token、manifest 审批、恢复、迁移与 sandbox 实测 | Advanced to Operations |
| [受控 Slack Sandbox 持续认证 Runbook](slack-managed-sandbox-certification-runbook.md) | shared/dedicated/组合 live OAuth、checkpoint、双 badge、恢复、清理与证据保鲜 | Operations |

## Complementary Resources

- **Architecture**: `docs/architecture.md` — system design and internals
- **QA Test Docs**: `docs/qa/` — scenario-based validation (120+ documents)
- **Design Docs**: `docs/design_doc/` — detailed feature designs
- **Fixture Manifests**: `fixtures/manifests/bundles/` — real YAML examples
- **OpenAI framing**: [Harness Engineering](https://openai.com/index/harness-engineering/) — the engineering model this project is explicitly leaning into

## Entry Points

The orchestrator uses a C/S architecture where a long-running daemon acts as the local control plane, holds all state, and the CLI is a lightweight gRPC client:

```bash
# Start daemon (background workers auto-consume enqueued tasks)
./target/release/orchestratord --foreground --workers 2

# Use CLI client (connects to daemon via Unix socket)
./target/release/orchestrator <command>

# Stop daemon
orchestrator daemon stop
```

See [07 - CLI Reference](07-cli-reference.md) for the complete C/S command surface.

## Claude Code Skill (for AI Agents)

A condensed, agent-optimized version of this guide is available as a Claude Code Skill. Install it to give Claude deep knowledge of orchestrator configuration and commands:

```bash
/install-skill .claude/skills/orchestrator-guide
```

Once installed, Claude will automatically reference the guide when working with orchestrator manifests, workflows, CEL expressions, or CLI commands.

## Chinese Version / 中文版

See [zh/README.md](zh/README.md) for the full Chinese translation of this guide.
