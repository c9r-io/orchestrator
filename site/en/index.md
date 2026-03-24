---
layout: home

hero:
  name: Agent Orchestrator
  text: AI-native SDLC Automation
  tagline: Declarative workflow and agent orchestration for the entire software development lifecycle — from requirements to deployment.
  actions:
    - theme: brand
      text: Get Started
      link: /en/guide/quickstart
    - theme: alt
      text: Why Orchestrator?
      link: /en/why
    - theme: alt
      text: GitHub
      link: https://github.com/c9r-io/orchestrator

features:
  - icon: "\U0001F4CB"
    title: Declarative Workflows
    details: Define multi-step workflows in YAML with loop control, guard steps, DAG execution, and dynamic step pools.
  - icon: "\U0001F916"
    title: Agent Orchestration
    details: Capability-driven agent selection with health scoring, rotation, and metrics-based load balancing.
  - icon: "\u26A1"
    title: CEL Prehooks
    details: Dynamic control flow via CEL expressions — Run, Skip, Branch, DynamicAdd, and Transform decisions at runtime.
  - icon: "\U0001F512"
    title: Built-in Security
    details: mTLS authentication, RBAC authorization, sandbox enforcement (macOS Seatbelt / Linux namespaces), and output redaction.
  - icon: "\U0001F680"
    title: Client/Server Architecture
    details: Lightweight CLI client communicates with the daemon over gRPC/UDS. Embedded workers, SQLite persistence, hot-reloadable config.
  - icon: "\U0001F310"
    title: Multi-platform
    details: "Install via curl | sh, brew install, or cargo install. Supports macOS (ARM64) and Linux (x86_64, ARM64)."
---

## Quick Start

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
