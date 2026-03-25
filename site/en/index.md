---
layout: home

hero:
  name: Agent Orchestrator
  text: Built for Agents, by Agents
  tagline: An orchestration platform that enables any shell-based AI agent to collaborate on complex, multi-step software development tasks through Kubernetes-style YAML manifests.
  actions:
    - theme: brand
      text: Get Started
      link: /en/guide/quickstart
    - theme: alt
      text: Showcases
      link: /en/showcases/benchmark-multi-model-execution
    - theme: alt
      text: GitHub
      link: https://github.com/c9r-io/orchestrator

features:
  - icon: "\U0001F916"
    title: Agent-First Design
    details: CLI outputs machine-parseable JSON, configuration is Kubernetes-style YAML, and skills provide structured plans that agents follow autonomously. Any shell-based agent can participate.
  - icon: "\U0001F91D"
    title: Multi-Agent Collaboration
    details: Agents with different capabilities (plan, implement, test, review) collaborate through the orchestrator. Capability matching, health scoring, and rotation are built in.
  - icon: "\U0001F4CB"
    title: Declarative Workflows
    details: Define multi-step workflows in YAML with loop control, guard steps, DAG execution, and dynamic step pools. No imperative code needed.
  - icon: "\u26A1"
    title: CEL Prehooks
    details: Dynamic control flow via CEL expressions — Run, Skip, Branch, DynamicAdd, and Transform decisions at runtime.
  - icon: "\U0001F512"
    title: Built-in Security
    details: mTLS authentication, RBAC authorization, sandbox enforcement (macOS Seatbelt / Linux namespaces), and output redaction.
  - icon: "\U0001F310"
    title: Any Agent, Any Model
    details: "Claude Code, OpenCode, Codex, Gemini CLI — any tool that accepts a prompt and runs shell commands can be an orchestrator agent. Switch models via environment variables."
---

## Quick Start

Run these commands directly, or let your AI coding agent handle it — the CLI is designed for both.

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
