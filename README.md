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

Resources are declared as YAML manifests with `apiVersion: orchestrator.dev/v2` and applied via `orchestrator apply -f`. Multiple resources can be combined in a single file separated by `---`.

```yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: default
spec:
  root_path: "."
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
---
apiVersion: orchestrator.dev/v2
kind: StepTemplate
metadata:
  name: qa_testing
spec:
  description: "Execute QA scenarios"
  prompt: >-
    /qa-testing {rel_path}
    Read the QA document at {rel_path}, execute each scenario step by step.
---
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: tester
  description: "tester agent — QA scenario execution"
spec:
  capabilities:
    - qa_testing
  command: claude -p "{prompt}" --dangerously-skip-permissions --verbose --output-format stream-json
  env:
    - fromRef: claude-sonnet
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: my-workflow
spec:
  max_parallel: 2
  steps:
    - id: qa_testing
      scope: item
      required_capability: qa_testing
      template: qa_testing
      enabled: true
      repeatable: true

    - id: loop_guard
      builtin: loop_guard
      enabled: true
      repeatable: true
      is_guard: true

  loop:
    mode: fixed
    max_cycles: 1
    enabled: true
    stop_when_no_unresolved: false

  safety:
    max_consecutive_failures: 3
    auto_rollback: true
    checkpoint_strategy: git_tag
```

See `docs/workflow/` for complete production manifests including `SecretStore`, `ExecutionProfile`, `WorkflowStore`, and `Trigger` resources.

## CLI Commands

```bash
# Start daemon
orchestratord --foreground --workers 2

# Core workflow
orchestrator init
orchestrator apply -f manifest.yaml
orchestrator task create --goal "QA run"
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

## CI and Security Automation

The repository includes GitHub Actions workflows for baseline quality gates:

- `CI`: runs `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`
- `Security`: runs `cargo audit` on pushes, pull requests, and a weekly schedule
- `Dependabot`: opens weekly update PRs for Cargo dependencies and GitHub Actions
- `Release`: builds tagged releases for Linux and macOS, packages `orchestrator` and `orchestratord`, and publishes checksum files to GitHub Releases

These workflows live under `.github/workflows/`, and dependency update policy is defined in `.github/dependabot.yml`.

## Installation

Install the latest GitHub Release with:

```bash
curl -fsSL https://raw.githubusercontent.com/gpgkd906/ai_native_sdlc/main/install.sh | sh
```

Install a specific version:

```bash
curl -fsSL https://raw.githubusercontent.com/gpgkd906/ai_native_sdlc/main/install.sh | INSTALL_ORCHESTRATOR_VERSION=v0.1.0 sh
```

Useful environment variables:

- `INSTALL_ORCHESTRATOR_VERSION`: release tag, defaults to `latest`
- `INSTALL_ORCHESTRATOR_BIN_DIR`: installation directory, defaults to `/usr/local/bin`
- `INSTALL_ORCHESTRATOR_REPO`: GitHub repository in `owner/name` format, defaults to `gpgkd906/ai_native_sdlc`

## Release Process

Push a tag in the form `vX.Y.Z` to trigger the release workflow:

```bash
git tag v0.1.0
git push origin v0.1.0
```

The workflow publishes tarballs for supported targets plus a `sha256sums` manifest. The install script uses those release assets directly.

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

### Prerequisites

The build requires `protoc` (Protocol Buffers compiler). If `protoc` is not installed, the build automatically compiles it from source via `protobuf-src` — no manual setup is needed. For faster builds, you can optionally install `protoc` and set the `PROTOC` environment variable:

```bash
# macOS
brew install protobuf
export PROTOC=$(which protoc)

# Ubuntu/Debian
sudo apt-get install -y protobuf-compiler
export PROTOC=/usr/bin/protoc
```

### Build

```bash
cargo build --workspace --release
```

```bash
orchestratord --foreground --workers 2 &
orchestrator init
orchestrator apply -f fixtures/capability-test.yaml
orchestrator task create --goal "My first QA run"
orchestrator task list
```

## Documentation

- [AGENTS.md](./AGENTS.md) - Agent configuration and orchestration details
- [SKILLS.md](./SKILLS.md) - Complete skills reference
- `docs/qa/` - QA test documents
- `docs/ticket/` - QA failure tickets
- `docs/architecture.md` - Architecture reference
- `docs/design-system.md` - Design system constraints
