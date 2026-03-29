---
layout: home

hero:
  name: Agent Orchestrator
  text: "Harness Engineering Control Plane"
  tagline: "A local-first control plane that turns Claude Code, Codex, OpenCode, Gemini CLI, and shell tooling into a governed engineering harness for long-running software delivery loops."
  actions:
    - theme: brand
      text: Read the Vision
      link: /en/guide/vision
    - theme: alt
      text: Get Started
      link: /en/guide/quickstart
    - theme: alt
      text: GitHub
      link: https://github.com/c9r-io/orchestrator

features:
  - icon: "\U0001F916"
    title: "Harness Engineering Native"
    details: "Built around a simple idea: humans define intent and constraints, while agents execute inside a reusable harness of workflows, skills, triggers, and guardrails."
  - icon: "\U0001F91D"
    title: "Unified Agent Runtime"
    details: "Claude Code, Codex, OpenCode, Gemini CLI, and other shell-based tools can be abstracted into the same runtime through capabilities and command templates."
  - icon: "\U0001F4CB"
    title: "Declarative Control Plane"
    details: "Workspaces, agents, workflows, triggers, secrets, and policies are defined as versioned YAML resources instead of scattered scripts and ad hoc prompts."
  - icon: "\u26A1"
    title: "Long-Running Workflow Loops"
    details: "Plan, implement, test, review, and fix can run as guarded, stateful loops with repeatable steps, DAG execution, dynamic step pools, and trigger-based task creation."
  - icon: "\U0001F512"
    title: "Policy and Recovery Rails"
    details: "mTLS, RBAC, sandboxing, secret lifecycle management, loop guards, and persistent task state give agent execution stronger operational boundaries."
  - icon: "\U0001F310"
    title: "Observable by Default"
    details: "Structured events, execution logs, task state, and machine-readable outputs make long-running agent workflows auditable and operable."
---

## What This Project Is

Agent Orchestrator is not just an agent wrapper. It is a harness layer for software engineering systems.

OpenAI recently described this shift as [Harness Engineering](https://openai.com/index/harness-engineering/): engineers spend more effort designing environments, feedback loops, and control systems so agents can do reliable work. This project applies that idea to shell-native coding agents and local-first software delivery.

## What This Enables

- Institutionalize ad hoc agent usage into reusable manifests, skills, and workflow assets
- Run durable plan -> implement -> test -> review -> fix loops instead of one-off agent calls
- Keep one control plane while models, shells, and prompts change underneath

## Quick Start

Run these commands directly, or let your AI coding agent handle it. You are booting a local control plane, not just invoking a CLI once.

```bash
# Install
curl -fsSL https://raw.githubusercontent.com/c9r-io/orchestrator/main/install.sh | sh

# Or via Homebrew
brew install c9r-io/tap/orchestrator

# Or via Cargo
cargo install orchestrator-cli orchestratord
```

```bash
# Start daemon and run your first task
orchestratord --foreground --workers 2 &
orchestrator init
orchestrator apply -f manifest.yaml
orchestrator task create --goal "My first QA run"
orchestrator task list
```
