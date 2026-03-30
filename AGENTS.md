# Agent Orchestrator — AI Dev Platform Index

## Project Overview

This project is an **Agent Orchestrator** — a Harness Engineering control plane for agent-first software delivery. It turns shell-native coding agents (Claude Code, OpenCode, Codex, Gemini CLI, etc.) into governed execution units that participate in long-running, multi-step engineering workflows through Kubernetes-style YAML manifests.

### Core Capabilities

- **Workflow Orchestration**: Declarative multi-step workflows with loop control, guard steps, DAG execution, and CEL-based prehooks
- **Agent Orchestration**: Capability matching, health scoring, rotation, and load balancing across heterogeneous shell agents
- **Long-running Automation**: Task persistence, event streams, trigger-based task creation (cron/event/webhook), and guarded workflow loops

### Architecture

```
orchestrator (CLI) ──gRPC/UDS──> orchestratord (daemon)
                                    ├── gRPC server (tonic)
                                    ├── Embedded workers
                                    ├── SQLite persistence
                                    └── Sandbox enforcement
```

Key design principles:
- **Capability-driven orchestration**: Steps declare required capabilities, agents declare supported capabilities
- **Dynamic agent selection**: Strategies include CapabilityAware, CostBased, SuccessRateWeighted, PerformanceFirst, Adaptive, LoadBalanced
- **Declarative workflow definitions**: Steps can be builtin (`init_once`, `ticket_scan`, `self_test`, `self_restart`, `item_select`, `loop_guard`) or capability-based
- **CEL prehooks**: Conditional step execution via CEL boolean expressions
- **Guard steps**: Steps that can terminate the workflow loop based on their output
- **Dynamic step pools**: Runtime step selection based on context and priority
- **DAG execution engine**: Topological sort, cycle detection, conditional edges
- Built-in observability (structured logs, event streams, task lifecycle tracking)

### Manifest Format

All resources use the Kubernetes-style envelope with `apiVersion: orchestrator.dev/v2`:

```yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: default
spec:
  root_path: "."
  qa_targets: [docs/qa]
  ticket_dir: docs/ticket
---
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: coder
spec:
  capabilities: [implement, fix]
  command: 'claude -p "{prompt}" --output-format stream-json'
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: qa-loop
spec:
  steps:
    - id: init
      builtin: init_once
      repeatable: false
    - id: qa_test
      required_capability: qa
      repeatable: true
    - id: check_done
      builtin: loop_guard
      is_guard: true
      repeatable: true
  loop:
    mode: infinite
    max_cycles: 10
    stop_when_no_unresolved: true
```

#### Resource Kinds

| Kind | Purpose |
|------|---------|
| **Workspace** | File system context — `root_path`, `qa_targets`, `ticket_dir`, `health_policy` |
| **Agent** | Execution unit — `command` (with `{prompt}` placeholder), `capabilities`, `selection`, `env`, `command_rules` |
| **Workflow** | Process flow — `steps`, `loop`, `finalize`, `safety`, `dynamic_steps` |
| **StepTemplate** | Reusable prompt templates with pipeline variables (`{goal}`, `{source_tree}`, `{diff}`, etc.) |
| **ExecutionProfile** | Sandbox/host execution boundary — `fs_mode`, `network_mode`, resource limits |
| **EnvStore** | Non-sensitive environment variable sets |
| **SecretStore** | Sensitive secrets (auto-redacted from logs) |
| **Trigger** | Automatic task creation via cron schedule, filesystem events, or webhooks |
| **Project** | Isolation domain for resources |
| **RuntimePolicy** | Runner behavior, resume strategy, observability |

See `docs/workflow/` for production-ready manifest templates: `hello-world.yaml`, `qa-loop.yaml`, `scheduled-scan.yaml`, `command-rules.yaml`, `execution-profiles.yaml`, `self-bootstrap.yaml`, `fr-watch.yaml`, etc.

### Tech Stack

- **Backend**: Rust (Cargo workspace)
- **Database**: SQLite for task/item lifecycle tracking
- **CLI**: kubectl-style interface with machine-parseable output (`-o json`)
- **RPC**: gRPC (tonic + prost) for client/server mode
- **Transport**: Unix Domain Socket (default) or TCP
- **Security**: Sandbox (macOS Seatbelt / Linux namespaces), output redaction, secret lifecycle management

### Execution Mode

- **Client/Server**: `orchestratord` (daemon) + `orchestrator` (CLI client) — daemon holds state, CLI communicates via gRPC over UDS

---

This repo is an AI-first development scaffold. When a task touches architecture or UI design language, consult the corresponding docs before making decisions or changes:

- Architecture reference: `docs/architecture.md`
- Design system reference: `docs/design-system.md`

Recommended workflow:
1. Use `project-bootstrap` to generate a new project skeleton.
2. Create an explicit plan (scope, acceptance criteria, test plan).
3. Implement.
4. Generate reproducible QA docs under `docs/qa/` via `qa-doc-gen`.
5. Execute QA via `qa-testing`; file failures under `docs/ticket/`.
6. Fix end-to-end via `ticket-fix` and re-verify.
