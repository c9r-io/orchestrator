# AI Dev Platform

A comprehensive **Agent Orchestrator** platform built with Rust that provides unified **workflow orchestration** and **agent orchestration** capabilities. This platform automates the entire software development lifecycle from requirements to deployment.

> **Note**: This repo contains both the **orchestrator core** (Rust CLI) and the **AI development skills** (`.claude/skills/`) that drive an end-to-end development workflow.

## What It Does

The orchestrator combines workflow engines with agent coordination to enable:
- **Capability-driven orchestration**: Steps declare required capabilities, agents declare supported capabilities
- **Dynamic agent selection**: Intelligent routing based on capability matching and health scoring
- **Declarative workflow definitions**: Built-in steps (`init_once`, `ticket_scan`, `loop_guard`) or capability-based
- **Repeatable steps**: Control whether steps execute in every loop cycle
- **Guard steps**: Terminate workflows based on runtime output
- **Dynamic Orchestration**: PrehookDecision (Run/Skip/Branch/DynamicAdd/Transform) for runtime step control
- **DAG Execution Engine**: Topological sort, cycle detection, conditional edges
- **Dynamic Step Pool**: Runtime step selection based on context and priority
- **Built-in observability**: Real-time logs, event tracking, metrics collection

## Architecture

The orchestrator uses a **client/server** architecture (daemon + gRPC client):

```
orchestrator (CLI) ──gRPC/UDS──> orchestratord (daemon)
                                    ├── gRPC server (tonic)
                                    ├── Embedded workers (N configurable)
                                    ├── Engine + DB
                                    └── Lifecycle (PID, socket, signals)
```

### Internal Components

```
┌─────────────────────────────────────────────────────────────────┐
│              CLI / gRPC Client (kubectl-style)                   │
│  task create/start/pause/resume | apply | get | store | daemon  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                Service Layer (core/src/service/)                 │
│  Pure business logic: task, resource, store, system, bootstrap  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Scheduler / Runner                            │
│  - Task lifecycle management                                     │
│  - Phase execution (init_once → qa → fix → retest → guard)    │
│  - Agent rotation with health scoring                           │
│  - Prehook evaluation (CEL)                                     │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Selection Engine                            │
│  - Capability matching                                          │
│  - Health-aware agent selection                                  │
│  - Metrics-based scoring                                        │
│  - Load balancing                                               │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      SQLite Database                             │
│  - Tasks, TaskItems, CommandRuns, Events, Metrics             │
└─────────────────────────────────────────────────────────────────┘
```

## Core Capabilities

### Workflow Orchestration
| Feature | Description |
|---------|-------------|
| Multi-step workflows | `init_once`, `qa`, `ticket_scan`, `fix`, `retest`, `loop_guard` |
| Loop control | `once` / `infinite` modes with `max_cycles` limits |
| Guard steps | Built-in `loop_guard` + custom guard steps |
| Repeatable steps | Per-cycle execution control |
| DAG execution | Topological sort, cycle detection, conditional edges |

### Agent Orchestration
| Feature | Description |
|---------|-------------|
| Capability matching | Steps declare `required_capability`, agents declare `capabilities` |
| Agent rotation | Top-3 scoring with random selection |
| Health management | Consecutive errors → "diseased" → 5h recovery |
| Metrics collection | Success rate, latency, load, API calls |
| Scoring strategies | `CapabilityAware`, `Performance`, `Quality`, `Balance` |

### Dynamic Orchestration (Prehook 2.0)
```rust
PrehookDecision::Run        // Execute step
PrehookDecision::Skip      // Skip step
PrehookDecision::Branch    // Jump to step
PrehookDecision::DynamicAdd // Add steps dynamically
PrehookDecision::Transform // Transform template
```

### Security & Governance
- Shell allowlist policy
- Environment variable allowlist
- Output redaction (token/password/secret)
- CEL expression validation

## Config Format

```yaml
runner:
  shell: /bin/bash
  shell_arg: -lc

resume:
  auto: false

defaults:
  workspace: default
  workflow: my_workflow

workspaces:
  default:
    root_path: .
    qa_targets:
      - docs/qa
    ticket_dir: docs/ticket

agents:
  opencode:
    metadata:
      name: opencode
      cost: 50
    capabilities:
      - qa
      - fix
    templates:
      qa: "opencode run {rel_path}"
      fix: "opencode run {ticket_paths}"
      loop_guard: "echo '{\"continue\":false,\"should_stop\":true}'"

workflows:
  my_workflow:
    steps:
      - id: run_qa
        required_capability: qa
        enabled: true
        repeatable: true

      - id: run_fix
        required_capability: fix
        enabled: true
        repeatable: true

      - id: check_stop
        builtin: loop_guard
        enabled: true
        repeatable: true
        is_guard: true

    loop:
      mode: infinite
      guard:
        enabled: true
        stop_when_no_unresolved: true

## CLI Commands

```bash
# Start daemon
orchestratord --foreground --workers 2

# Core workflow
orchestrator init
orchestrator apply -f manifest.yaml
orchestrator task create --goal "QA run" --detach
orchestrator task list
orchestrator task logs <task_id>
orchestrator get workspaces -o json
orchestrator store put mystore key '{"value":1}'
```

## Project Structure

```
.
├── Cargo.toml               # Workspace root
├── core/                    # Rust orchestrator engine (library)
│   ├── src/
│   │   ├── service/        # Pure business logic layer
│   │   ├── scheduler.rs    # Task scheduling & loop execution
│   │   ├── runner.rs      # Command execution
│   │   ├── selection.rs   # Agent selection engine
│   │   ├── prehook.rs     # CEL-based prehook evaluation
│   │   └── ...
│   └── Cargo.toml
│
├── crates/
│   ├── proto/              # gRPC codegen (tonic + prost)
│   ├── daemon/             # orchestratord (gRPC server + workers)
│   └── cli/                # orchestrator (lightweight gRPC client)
│
├── proto/                   # Protocol buffer definitions
│   └── orchestrator.proto
│
├── .claude/skills/        # AI development skills
│   ├── project-bootstrap/ # Initialize full-stack projects
│   ├── qa-testing/        # Execute QA scenarios
│   ├── qa-doc-gen/       # Generate QA docs
│   ├── ticket-fix/        # Fix QA tickets
│   ├── test-coverage/     # Run tests & coverage
│   ├── e2e-testing/      # Playwright E2E tests
│   ├── security-test-doc-gen/  # ASVS 5.0 security tests
│   ├── uiux-test-doc-gen/      # UI/UX tests
│   ├── deploy-gh-k8s/    # GitHub → K8s deployment
│   ├── ops/              # Docker/K8s troubleshooting
│   └── ...
│
├── docs/
│   ├── qa/               # QA test documents
│   ├── ticket/           # QA failure tickets
│   ├── security/         # Security test docs
│   ├── uiux/             # UI/UX test docs
│   └── design_doc/       # Design documents
│
├── fixtures/             # Sample configs & manifests
└── scripts/              # Utility scripts
```

## AI Development Workflow

This platform supports a complete AI-first development loop:

```
┌─────────────────────────────────────────────────────────────────┐
│  1. bootstrap    → Create project skeleton (Rust + React)     │
│  2. plan         → Explicit scope, acceptance criteria         │
│  3. implement    → Write feature code                          │
│  4. qa-doc-gen  → Generate QA test docs                       │
│  5. qa-testing  → Execute QA scenarios                         │
│  6. ticket-fix   → Fix failed tickets                          │
│  7. align-tests  → Fix broken tests after refactor            │
│  8. test-coverage→ Check test coverage                        │
│  9. security    → Generate security tests (ASVS 5.0)         │
│  10. uiux       → Generate UI/UX tests                        │
│  11. readiness  → Pre-release checks                           │
│  12. deploy     → Deploy to Kubernetes                         │
└─────────────────────────────────────────────────────────────────┘
```

## Skills Index

See [SKILLS.md](./SKILLS.md) for the complete list of available skills and how to use them.

### Quick Reference

| Skill | Purpose |
|-------|---------|
| `project-bootstrap` | Initialize new full-stack project |
| `qa-testing` | Execute QA test scenarios |
| `qa-doc-gen` | Generate QA test documents |
| `ticket-fix` | Fix QA failure tickets |
| `test-authoring` | Write unit/E2E tests |
| `test-coverage` | Measure test coverage |
| `e2e-testing` | Playwright E2E tests |
| `performance-testing` | Load testing with hey |
| `deploy-gh-k8s` | GitHub → K8s deployment |
| `ops` | Docker/K8s troubleshooting |
| `security-test-doc-gen` | OWASP ASVS 5.0 security tests |
| `uiux-test-doc-gen` | UI/UX consistency tests |

## Tech Stack

- **Backend**: Rust (2021 edition)
- **Async**: Tokio
- **Database**: SQLite (bundled)
- **CLI**: Clap + Clap Complete
- **RPC**: tonic + prost (gRPC over UDS/TCP)
- **Condition Engine**: cel-interpreter
- **Serialization**: Serde (JSON/YAML)

## Getting Started

### Build

```bash
cargo build --workspace --release
```

```bash
orchestratord --foreground --workers 2 &
orchestrator init
orchestrator apply -f fixtures/capability-test.yaml
orchestrator task create --goal "My first QA run" --detach
orchestrator task list
```

## Documentation

- [AGENTS.md](./AGENTS.md) - Agent configuration and orchestration details
- [SKILLS.md](./SKILLS.md) - Complete skills reference
- `docs/qa/` - QA test documents
- `docs/ticket/` - QA failure tickets
- `docs/architecture.md` - Architecture reference
- `docs/design-system.md` - Design system constraints
