# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-03-24

Initial release of the Agent Orchestrator platform.

### Added

#### Core Engine
- DAG execution engine with topological sort, cycle detection, and conditional edges
- CEL (Common Expression Language) prehook decisions: Run, Skip, Branch, DynamicAdd, Transform
- Capability-driven agent selection with health scoring and load balancing
- Dynamic step pools with runtime step selection based on context and priority
- Pipeline variables with CEL expression interpolation

#### Architecture
- Client/server model: `orchestratord` daemon + `orchestrator` CLI over gRPC/UDS
- Configurable worker pool (`--workers N`) for concurrent task execution
- Proper daemonization with PID file, log rotation, and crash recovery
- Fixed data directory at `~/.orchestratord/` with database-level project isolation

#### Workflow Engine
- Declarative YAML manifests (v2 resource model: `orchestrator.dev/v2`)
- Loop control: `once` / `infinite` modes with `max_cycles` limits
- Guard steps for workflow termination (`loop_guard`, convergence expressions)
- Repeatable steps with per-cycle execution control
- Step templates for reusable step definitions
- Item-scoped git worktree isolation for parallel execution

#### Resource Model
- 11 built-in resource kinds: Workspace, Agent, Workflow, StepTemplate, ExecutionProfile, SecretStore, EnvStore, WorkflowStore, Trigger, RuntimePolicy, CustomResourceDefinition
- Custom Resource Definitions (CRD) with JSON Schema + CEL validation
- Resource versioning and audit trail

#### Security
- mTLS control plane with auto-generated PKI (CA, server, client certificates)
- RBAC authorization (read_only, operator, admin roles)
- SecretStore encryption (AES-256-GCM-SIV) with key rotation support
- Control plane audit logging
- Sandbox enforcement: resource limits, network isolation, writable paths
- Daemon PID guard against subprocess kill attempts

#### Triggers
- Cron-based scheduled task creation
- Event-driven task creation (workflow completion, step events)

#### Observability
- Structured logging with JSON and pretty formats
- Event system with TTL cleanup and JSONL archival
- Agent health metrics, success rates, and latency tracking
- Task execution metrics sampling

#### CLI
- kubectl-style interface with aliases (`t` for `task`, `g` for `get`)
- Output formats: table, JSON, YAML
- Shell completion support (via `clap_complete`)
- Daemon lifecycle commands: stop, status, maintenance mode

#### GUI (Alpha)
- Tauri 2.x desktop application with gRPC client
- Wish pool UI with real-time progress observation
- Theme toggle, i18n framework, responsive layout

#### Distribution
- Multi-platform binaries: Linux (x86_64, aarch64) + macOS (x86_64, aarch64)
- Automated release pipeline with SHA256 checksums
- One-line installer: `curl -fsSL .../install.sh | sh`

#### Documentation
- 7-chapter user guide (English + Simplified Chinese)
- Architecture reference documentation
- 70+ design documents with QA verification
