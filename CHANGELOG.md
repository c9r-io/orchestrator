# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.2.3] - 2026-03-28

### Changed
- **Core crate decomposition** — extracted 3 leaf crates from the 60K-LOC monolithic `agent-orchestrator` core:
  - `orchestrator-collab` (1,935 LOC) — agent collaboration types, message bus, shared context, DAG primitives
  - `orchestrator-security` (1,895 LOC) — SecretStore encryption, key lifecycle, audit, secure file helpers
  - `orchestrator-runner` (2,305 LOC) — command runner, sandbox, output capture, network allowlist
- **TaskRepository sub-trait split** — decomposed the 38-method `TaskRepository` trait into 7 domain-aligned sub-traits (`TaskQueryRepository`, `TaskItemQueryRepository`, `TaskStateRepository`, `TaskItemMutRepository`, `CommandRunRepository`, `EventRepository`, `TaskGraphRepository`) with a blanket supertrait for backward compatibility
- All existing import paths preserved via re-export facades — zero downstream breakage

## [0.2.2] - 2026-03-26

### Added
- Filesystem trigger — `event.source: filesystem` for native file system change detection (macOS FSEvents / Linux inotify via `notify` crate)
- Lazy watcher lifecycle — zero filesystem triggers = zero overhead; watcher created/released on demand
- Filesystem event payload — `payload_path`, `payload_filename`, `payload_dir`, `payload_event_type`, `payload_timestamp` available in CEL filter
- Path safety constraints — watched paths must be within workspace `root_path`; `.git/` and daemon data dir auto-excluded
- Workflow template library — 5 progressive templates (hello-world, qa-loop, plan-execute, scheduled-scan, fr-watch) with echo agents for zero-cost tryout
- Doc site "Templates" section — 5 beginner-friendly entries in EN/ZH Showcases sidebar
- Agent `command_rules` — CEL conditional command selection per agent; first matching rule overrides default `command`
- Step `step_vars` — per-step temporary pipeline variable overlay (isolated from other steps)
- `command_rule_index` audit column in `command_runs` table for rule traceability
- Skill template packaging — 17 skills distributed as templates (generic/framework/sdlc-patterns)
- `scripts/package-skill-templates.sh` — sanitizes and packages skills for release
- `install.sh` installs templates to `~/.orchestratord/skill-templates/`
- Skill setup showcase — agent-driven project analysis and skill specialization
- `integration-authoring` skill for managing companion integrations repo

## [0.2.1] - 2026-03-26

### Added
- Per-trigger webhook authentication — `webhook.secret.fromRef` resolves signing keys from SecretStore with multi-key rotation support
- Custom signature header per trigger — `webhook.signatureHeader` (default: `X-Webhook-Signature`)
- CEL payload filtering — `filter.condition` evaluates CEL expressions against webhook JSON body
- Integration manifest packages — companion repo `c9r-io/orchestrator-integrations` with Slack, GitHub, LINE pre-configured triggers
- `integration-authoring` skill for creating new integration packages
- Secret rotation showcase (`docs/showcases/secret-rotation-workflow.md`)

### Changed
- Webhook auth fallback chain: per-trigger secret → global `--webhook-secret` → no verification

## [0.2.0] - 2026-03-25

### Added
- HTTP webhook endpoint — `--webhook-bind <ADDR>` runs axum HTTP server alongside gRPC
- Webhook trigger source — `event.source: webhook` for external event ingestion
- HMAC-SHA256 signature verification — `--webhook-secret` with `X-Webhook-Signature` header
- `orchestrator trigger fire --payload` — simulate webhook payloads via CLI
- `orchestrator task items <task_id>` — list task item status
- `orchestrator event list --task <task_id>` — list task events with type filter
- `orchestrator db vacuum` — reclaim SQLite disk space
- `orchestrator db cleanup --older-than N` — manual log file cleanup
- `orchestrator db status` — shows DB, logs, and archive sizes
- Automatic log file TTL cleanup — `--log-retention-days 30` (default enabled)
- Optional task auto-cleanup — `--task-retention-days N` (default disabled)

### Changed
- Webhook payload included in trigger goal for context
- `db status` output now includes disk usage information

## [0.1.6] - 2026-03-25

### Changed
- Dependencies upgraded: clap 4.6, nix 0.31, cron 0.15, arc-swap 1.9, tracing-subscriber 0.3.23, clap_complete 4.6
- Fix nix 0.31 breaking change: `dup2()` API migration to `AsFd` + `OwnedFd`
- CI clippy and fmt fixes

## [0.1.5] - 2026-03-25

### Changed
- Documentation site launched at docs.c9r.io (VitePress + Cloudflare Pages)
- 9 showcase execution plans with EN/ZH translations
- Multi-model benchmark showcase for comparing LLM shells and models
- README slimmed from 371 to 74 lines with agent-first vision
- Project identity: "Built for agents, by agents"

## [0.1.3] - 2026-03-25

### Fixed
- Supply chain: rustls-webpki 0.103.9 → 0.103.10 (RUSTSEC-2026-0049)
- Supply chain: migrate serde_yml → serde_yaml (RUSTSEC-2025-0067/0068)

## [0.1.2] - 2026-03-24

### Fixed
- `orchestrator get` returns empty results instead of error for missing projects
- Full CLI/daemon documentation alignment (20+ stale references fixed)

### Changed
- Showcases sanitized with developer-friendly placeholders
- sqlite workarounds replaced with CLI commands

## [0.1.1] - 2026-03-24

### Added
- Homebrew tap: `brew install c9r-io/tap/orchestrator`
- crates.io publishing with Trusted Publishers (OIDC)
- crate READMEs for crates.io display

### Changed
- Release workflow: Homebrew formula auto-push + crates.io auto-publish

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
