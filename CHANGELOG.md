# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- **Agent Process Console v1** (FR-095 through FR-105) — deterministic process timelines and evidence, cross-task Attention Inbox, immutable handoff briefings and reviewed safe resume, governed Session re-entry/control, provider-neutral source bindings with a Slack adapter, canonical mutation audit, and local privacy-safe operational metrics
- **Agent Process Console UI** (FR-100) — Attention-first navigation, integrated Process Workspace, global Session re-entry, stable hash deep links, keyboard triage, role-sensitive actions, responsive/reduced-transparency fallbacks, and request-ID error correlation
- Console release acceptance with populated schema-26 upgrade coverage, nine independently owned slice gates, a real Tauri-to-gRPC recovery flow, release performance fixtures, and the [operator runbook](docs/guide/agent-process-console-v1-operations.md)
- Frontend Vitest and Playwright coverage for route migration, Attention reconciliation, semantic evidence, read-only gates, narrow navigation, accessibility, and visual fallbacks
- **Slack Reaction Skill Automation** (FR-107 through FR-113) — authenticated `reaction_added` ingestion, versioned Skill task templates, exact badge bindings, Slack permalink resolution, canonical task creation, durable retries/Attention replay, and Sources → Automations management
- Slack automation release acceptance with two badges selecting distinct Skill/workflow tasks, concurrent identity convergence, rate-limit restart recovery, real Tauri provenance, populated migration, compatible previous-binary rollback, and the [setup/operations guide](docs/guide/slack-reaction-skill-automation.md)
- **Managed Slack Connections** (FR-114) — one-consent installation of an official shared Orchestrator Slack App, independent internet-facing OAuth/Events Gateway, project-scoped SourceConnection lifecycle, outbound durable delivery, bounded permalink proxy, target-side two-phase ownership transfer, and Sources → Connections management with a [deployment and user guide](docs/guide/slack-managed-connections.md)
- **Dedicated Slack App Provisioning** (FR-115) — an advanced per-workspace private App path with a fixed reviewed manifest, local-only short-lived Configuration Token custody, one-time receipt-gated credential import, per-connection encrypted App identity, exact-App OAuth/events, provisioning Attention recovery, reviewed shared↔dedicated migration, semantic manifest upgrade with suspension/reauthorization, separate typed App deletion, CLI stdin, and a [dedicated setup guide](docs/guide/slack-dedicated-app-provisioning.md)
- **Agent Driver Abstraction** (FR-116) — per-Agent `shell/cli`, `claude/cli`, and `codex/cli` adapters; typed provider-neutral options and workflow requirements; structured apply diagnostics; direct event-stream folding; complete tool/usage/permission event projection; opaque in-memory session attachment; and run-scoped private MCP configuration. See the [driver guide](docs/guide/agent-driver-model.md).
- **Non-code Workspaces and Global File Sharing** (FR-117) — `task` workspaces with an optional persistent `work_dir`, one implicit process item, provider-neutral convergence, private per-task HOME/cwd allocation, operator-owned `fileSharing` ceilings, read-only global Skills, Process Console task semantics, and a reproducible Slack inventory pilot. See the [user guide](docs/guide/non-code-workspace.md).

### Changed
- Wish Pool and Progress Observer are now presented as New Process and Processes; resource administration remains reachable through System and raw diagnostics through Process Expert
- Session read and control rollout is globally authoritative from the `_system` RuntimePolicy; ordinary project policies cannot override the fail-closed control gate
- Process Console mutations support `action_audit_mode=compatibility|enforced`; rollout begins in compatibility mode and moves to enforced only after clients send canonical action context
- Explicit driver phases use `setup → start → consume → fold → record`; the deprecated global streaming executor is now a provider-owned compatibility bridge while legacy manifests migrate
- Codex CLI cross-step session attachment is certified against `codex-cli 0.144.5` with same-thread context continuity, a sanitized recorded JSONL fixture, an offline replay gate, and an isolated live recertification script
- Workspace manifests serialize the canonical `work_dir` field while continuing to accept legacy `root_path`; existing omitted `kind` manifests retain `code_repo` behavior

### Fixed
- Slack source automation now permits different reviewed badge bindings on the same message to create distinct tasks, while preserving one route/task for retries of the same message/reaction/binding identity
- Task-scoped driver completion and `mark_done` events now participate in implicit-item convergence, and successful low-confidence Slack replies create Attention records without converting the step into a failure

### Security
- Global Skill directories now fail closed unless owned by the daemon effective user, free of group/world write bits, and disjoint from every task Workspace and writable ExecutionProfile path; unsupported platforms reject configured global Skills with `FILE_SHARING_GLOBAL_SKILL_UNTRUSTED`

### Compatibility And Migrations
- Migrations 27-32 add Attention/change feeds, handoff/resume state, Session control fencing, source events/bindings, canonical action audit, and Process Console metric observations/rollups. They are additive, forward-only, restart-safe, and preserve existing task and Session identity.
- Migrations 33-34 add durable source automation routes, frozen template/binding generations, optimistic route versions, bounded retry leases, attempt/change history, and Attention correlation. They are additive and forward-only; normal binary rollback keeps their tables and disables reaction writers before using a verified compatible binary.
- Daemon migration 35 adds SourceConnection, OAuth intent, and monotonic connection-change persistence. Slack Gateway schema versions 1-2 independently add encrypted app/install credentials, normalized delivery/audit state, and target-side transfer handoffs. Both stores are additive, forward-only, and backed up/restored with their own encryption keys.
- Daemon migration 36 adds safe dedicated App projections and provisioning checkpoints. Slack Gateway schema 3 adds per-App encryption contexts, one-time import capabilities, signed receipt metadata, dedicated OAuth identity, and exact event endpoint mapping; populated shared/manual state remains compatible.
- Daemon migration 37 adds reviewed dedicated-provisioning migration targets. Slack Gateway schema 4 adds installation/version/source-mode OAuth fences so stale or unreviewed App-mode callbacks fail closed; older binaries reject newer stores rather than guessing rollback compatibility.
- Existing task, trace, log, watch, CLI, and additive gRPC clients remain compatible. No persisted `Task` rename or destructive schema conversion is included.
- No database migration is required for task workspaces. Existing Workspace manifests remain compatible through the default `code_repo` kind and `root_path` input alias; exported manifests use `work_dir`.
- Normal rollback disables source/session/resume writers and optional projectors before deploying the previous binaries; it retains migrations 27-32 and all Console tables. Database restore is reserved for migration failure or corruption.

### Slack Permissions, Secrets, And Privacy
- Slack Events API configuration requires `reaction_added` delivery and the `reactions:read` scope. Inbound requests use Slack Signing Secret verification; outbound `chat.getPermalink` uses a separately referenced installation token from SecretStore.
- Slack message bodies, attachments, and thread transcripts are not ingested. Tasks contain the configured Skill invocation plus the protected message permalink; safe source/route/UI projections omit credentials, raw payloads, rendered goals, and permalinks unless an Operator explicitly opens the protected route.
- Managed shared mode keeps official app and installation tokens encrypted only in the Slack Gateway. The daemon holds an encrypted installation-scoped pairing; OAuth state/code, tokens, raw Slack bodies, private workspace names, and provider URLs are excluded from safe connection projections, browser storage, tasks, metrics, and routine logs.
- Dedicated mode keeps the Configuration Token only in zeroizing daemon memory and clears it before UI review completes. Newly created App credentials move once into connection-context encrypted Gateway storage; safe state exposes only digests, manifest version, and stable provisioning errors.

### Known Non-goals
- Desktop application packaging/distribution (FR-076), hosted multi-tenant SaaS, down migrations, arbitrary checkpoint rollback, and unreviewed non-idempotent replay are not part of Console v1.
- Marketplace distribution, Enterprise Grid/GovSlack, outbound Slack progress messages, `reaction_removed` task cancellation, message-body ingestion, production Slack release testing, in-place Slack Signing/Client Secret rotation, and destructive automation down migrations are not included. FR-114/FR-115 live certification is limited to a controlled non-production Slack sandbox.

## [0.3.1] - 2026-04-06

### Security
- **UDS trust boundary hardening** — fix RPC role map, enrich audit metadata, add daemon startup checks
- **Least-privilege UDS default** — default UDS max role changed from Admin to Operator

### Added
- **Benchmark evaluation 6-dimension scoring** — upgraded from simple pass/fail to 0-60 composite score

### Fixed
- **Trigger firing chain** (P1) — eliminate duplicate tasks, bypass, and cross-project leakage in unified fire path
- **Sandbox capability matrix** — Linux does not support non-inherit `fs_mode`; corrected capability reporting
- **Loop-guard builtin step** — skip agent capability check when builtin step is present

## [0.3.0] - 2026-04-05

### Added
- **Self-describing CLI reference** — `orchestrator guide` command for built-in documentation

### Changed
- **Core module decomposition** — split oversized dispatch, resource service, and workflow convert modules for maintainability

## [0.2.8] - 2026-04-04

### Added
- **Lightweight step run** (FR-090) — `orchestrator run` command for ad-hoc single-step execution without full workflow scaffolding
- **Design-first workflow skills** — `design-brief-gen` and `design-governance` skills for structured design-first development
- 195 new unit tests — coverage improved from 80.9% to 82.3%

### Fixed
- **CRD plugin process-group isolation** (P1) — plugin child processes now run in dedicated process groups with correct async execution semantics
- **Cross-platform sandbox capability gaps** (P2) — sandbox capability mismatches are now surfaced at manifest validate time rather than failing silently at runtime
- **Log read-path per-project secret redaction** (P2) — defense-in-depth redaction now resolves the task's actual project_id instead of hardcoding the default project; prevents cross-project secret leakage on fallback
- Documentation drift in README and architecture reference
- Replaced 'operator' terminology with 'user' in plugin policy docs

## [0.2.7] - 2026-04-02

### Added
- **Plugin policy governance** (P0-SEC) — layered defense against CRD plugin privilege escalation:
  - `PluginPolicy` with three modes: `deny`, `allowlist` (default), `audit`
  - Command allowlist with prefix matching; built-in denied patterns (curl, wget, nc, eval, base64, /dev/tcp)
  - Timeout cap enforcement (default 30s max per plugin)
  - Hook command policy enforcement (`enforce_on_hooks: true` by default)
  - Admin role elevation for CRDs containing plugins or hooks (`ApplyPluginCrd` RPC)
  - `plugin_audit` SQLite table for immutable audit trail (migration m0022)
  - Audit logging on CRD apply (allowed/denied) and plugin execution
  - Policy loaded from `{data_dir}/plugin-policy.yaml`; absent file = Allowlist with empty allowlist (secure-by-default)
- QA doc 137: plugin policy governance verification (5 scenarios)
- Integration tests for plugin policy enforcement (6 tests)

## [0.2.6] - 2026-04-01

### Added
- **CRD plugin system** (FR-083) — generic custom resource definition plugin framework with three plugin types: interceptor, transformer, cron; `webhook.authenticate`/`webhook.transform` extension points; `crdRef` trigger association; built-in orchestrator tool library
- **QA doctor CLI** (FR-088) — `orchestrator qa doctor` command exposing task execution metrics for observability
- **SecretStore emergency recovery** (FR-089) — `secret key bootstrap` command for encryption key emergency recovery
- **Health policy CLI fixtures** (FR-087) — automated QA script for verifying custom health policy display via `orchestrator check`
- **Dependabot governance skill** — dependency PR lifecycle management

### Fixed
- Key rotation crash safety — prevent data loss during SecretStore key rotation
- Mark QA-64/135 as self-referential unsafe
- Clippy errors — unused gid field and redundant i32 cast
- SecretStore write-blocked error message when encryption keys revoked
- Resolved 30+ QA tickets — doc drift, triage, test alignment, feature gap routing

### Changed
- **Dependency upgrades** — sha2 0.10→0.11, hmac 0.12→0.13, notify 7→8.2, notify-debouncer-full 0.4→0.7, cron 0.15→0.16, picomatch 4.0.3→4.0.4 (CVE fix)

## [0.2.5] - 2026-03-29

### Fixed
- **SafetySpec derived Default** stored zeros instead of proper defaults — now correctly initializes all safety fields
- **Block-style YAML arrays** in frontmatter parser — suppressed false `orphan_command` warnings for multi-line list syntax
- **FR-086 daemon config hot reload** confirmed already implemented via ArcSwap — closed as no-op
- **FR-086 agent selection threshold** closed via Option 2 (unit-test verification) — added `test_diseased_agent_with_passing_capability_threshold_is_selected` integration test proving diseased agents with custom `capability_success_threshold` remain selectable
- **QA-106 inflight wait test fixture** — 3 integration tests verify heartbeat reset (S1), timeout reap (S2), and diagnostic events (S4)
- Resolved all 18 QA tickets — fmt drift, doc date corrections, lint fixes, and feature gap FRs

### Changed
- Removed unused `MessageBus` mechanism (dead code cleanup)
- Added scenario-level self-referential safety annotations to QA docs

## [0.2.4] - 2026-03-28

### Changed
- Extended panic-safety deny lints (`clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`) to all production crates
- Resolved clippy errors and formatting drift across core crates after crate decomposition

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
- CEL (Common Expression Language) prehooks: conditional step execution via bool expressions
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
