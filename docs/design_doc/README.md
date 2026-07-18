# Design Docs

This directory contains design documents captured from confirmed plans (plan mode output). They preserve context after implementation (goals, scope, tradeoffs, risks, observability, acceptance criteria) to reduce future iteration overhead.

Generation entry point:
- Before generating `docs/qa/**`, `qa-doc-gen` generates the corresponding `docs/design_doc/**` design docs (same module-based structure).

**Featured narrative:** [Streaming Runner Pivot — Overview](orchestrator/streaming-runner-pivot-overview.md) — a one-page tour of the 101→102→103 arc (replace the shell black-box agent contract with a structured, tool-calling one) and the live demo where a workflow converges on `'mark_done' in tools_called`.

## Suggested Directory Structure

```
docs/design_doc/
├── README.md
├── <module>/
│   ├── 01-<topic>.md
│   └── 02-<topic>.md
└── ...
```

## Document Rules (Strict)

- Write everything in English. Keep technical details (API paths, SQL, field names, metric names) as-is.
- Each design doc must include:
  - Background and goals (including non-goals)
  - Scope (in/out)
  - Interfaces/data changes (if applicable)
  - Key design and tradeoffs
  - Risks and mitigations
  - Observability and operations (include at least default recommendations)
  - Testing and acceptance (must point to the related QA doc path)

## Index (Recommended)

| Module | Doc | Related QA | Notes |
|--------|-----|------------|-------|
| example | `docs/design_doc/example/01-sample.md` | `docs/qa/example/01-sample.md` | skeleton |
| orchestrator | `docs/design_doc/orchestrator/01-cli-agent-orchestration.md` | `docs/qa/orchestrator/01-cli-agent-orchestration.md` | CLI testing with mock agents |
| orchestrator | `docs/design_doc/orchestrator/08-project-namespace.md` | `docs/qa/orchestrator/08-project-namespace.md` | Project namespace for resource isolation |
| orchestrator | `docs/design_doc/orchestrator/09-scheduler-repository-refactor.md` | `docs/qa/orchestrator/19-scheduler-repository-refactor-regression.md` | P0/P1 scheduler data-layer refactor and error observability |
| orchestrator | `docs/design_doc/orchestrator/10-structured-output-worker-scheduler.md` | `docs/qa/orchestrator/20-structured-output-worker-scheduler.md` | Structured output scheduler mainline + queue-only daemon worker model |
| orchestrator | `docs/design_doc/orchestrator/11-performance-io-queue-optimizations.md` | `docs/qa/orchestrator/22-performance-io-queue-optimizations.md` | Single-write command runs, bounded IO reads, true tail, and atomic multi-worker claim |
| self-bootstrap | `docs/design_doc/self-bootstrap/01-survival-mechanism.md` | `docs/qa/self-bootstrap/01-survival-binary-checkpoint-self-test.md`, `docs/qa/self-bootstrap/02-survival-enforcement-watchdog.md` | 4-layer survival mechanism: binary checkpoint, self-test gate, self-referential enforcement, watchdog |
| orchestrator | `docs/design_doc/orchestrator/12-step-scope-segment-execution.md` | `docs/qa/orchestrator/29-step-scope-segment-execution.md` | StepScope enum + segment-based execution: task-scoped once, item-scoped fan out |
| orchestrator | `docs/design_doc/orchestrator/13-unified-step-execution-model.md` | `docs/qa/orchestrator/30-unified-step-execution-model.md` | Unified step execution: WorkflowStepType deletion, StepBehavior data types, StepExecutionAccumulator |
| orchestrator | `docs/design_doc/orchestrator/14-check-command.md` | `docs/qa/orchestrator/31-check-command.md` | New check CLI command: workspace/agent/config/all subcommands with output formats |
| self-bootstrap | `docs/design_doc/self-bootstrap/02-binary-snapshot-verification.md` | `docs/qa/self-bootstrap/06-survival-smoke-binary-snapshot-verification.md` | Binary verification function with MD5 checksum comparison |
| self-bootstrap | `docs/design_doc/self-bootstrap/03-self-restart-capability.md` | `docs/qa/self-bootstrap/07-self-restart-process-continuity.md` | Self-restart: rebuild binary, exec() hot reload (fallback: exit 75 restart loop), restart_pending resumption |
| self-bootstrap | `docs/design_doc/self-bootstrap/04-build-version-hash.md` | `docs/qa/self-bootstrap/08-build-version-hash.md` | Build version hash: compile-time git hash/timestamp, version subcommand, restart event enrichment |
| self-bootstrap | `docs/design_doc/self-bootstrap/05-self-referential-safety-policy-alignment.md` | `docs/qa/self-bootstrap/10-self-referential-safety-policy-alignment.md` | FR-003 safety alignment: unified policy evaluator, structured diagnostics, required self_test/rollback/checkpoint rules |
| orchestrator | `docs/design_doc/orchestrator/15-task-trace.md` | `docs/qa/orchestrator/32-task-trace.md` | Post-mortem diagnostics: execution timeline reconstruction and 9-rule anomaly detection |
| orchestrator | `docs/design_doc/orchestrator/16-structured-logging.md` | `docs/qa/orchestrator/36-structured-logging.md` | Structured logging bootstrap, CLI log overrides, stderr/stdout separation, and rolling system log files |
| orchestrator | `docs/design_doc/orchestrator/17-envstore-secretstore-agent-env.md` | `docs/qa/orchestrator/37-envstore-secretstore-resources.md`, `docs/qa/orchestrator/38-agent-env-resolution.md` | EnvStore/SecretStore resources and agent env configuration with runtime resolution and secret redaction |
| orchestrator | `docs/design_doc/orchestrator/18-prompt-delivery-abstraction.md` | `docs/qa/orchestrator/39-prompt-delivery.md` | PromptDelivery abstraction: stdin/file/env/arg modes to decouple prompt content from shell commands |
| orchestrator | `docs/design_doc/orchestrator/19-parallel-item-execution.md` | `docs/qa/orchestrator/44-parallel-item-execution.md` | Parallel item execution: max_parallel config, semaphore-gated JoinSet, RunningTask::fork(), pool size 20 |
| orchestrator | `docs/design_doc/orchestrator/20-workflow-primitives-wp02-wp03-wp04.md` | `docs/qa/orchestrator/47-task-spawning.md`, `docs/qa/orchestrator/48-dynamic-items-selection.md`, `docs/qa/orchestrator/49-invariant-constraints.md` | WP02/WP03/WP04 workflow primitives: task spawning, dynamic items + selection, invariant constraints |
| orchestrator | `docs/design_doc/orchestrator/21-sandbox-resource-network-enforcement.md` | `docs/qa/orchestrator/56-sandbox-resource-network-enforcement.md` | Step execution isolation closure: Unix resource limits, structured sandbox resource/network events, unsupported allowlist gating |
| orchestrator | `docs/design_doc/orchestrator/22-control-plane-security.md` | `docs/qa/orchestrator/58-control-plane-security.md` | Secure TCP control plane: mTLS bootstrap, host-user client config, role policy, and audit persistence |
| orchestrator | `docs/design_doc/orchestrator/23-dynamic-dag-mainline-execution.md` | `docs/qa/orchestrator/59-dynamic-dag-mainline-execution.md`, `docs/qa/orchestrator/32-task-trace.md` | FR-004 closure: task-level graph persistence, task info graph bundles, graph-run identifiers, and DAG debug view |
| orchestrator | `docs/design_doc/orchestrator/24-daemon-lifecycle-runtime-metrics.md` | `docs/qa/orchestrator/60-daemon-lifecycle-runtime-metrics.md`, `docs/qa/orchestrator/53-client-server-architecture.md` | FR-005 daemon runtime snapshot, graceful drain, additive Ping/WorkerStatus fields, and CLI daemon status view |
| orchestrator | `docs/design_doc/orchestrator/25-database-persistence-bootstrap-repositories.md` | `docs/qa/orchestrator/62-database-persistence-bootstrap-repositories.md` | FR-009 Phase 1: persistence bootstrap ownership, public schema-patch removal, and repository-backed session/store seams |
| orchestrator | `docs/design_doc/orchestrator/26-database-migration-kernel-and-repository-governance.md` | `docs/qa/orchestrator/63-database-migration-kernel-and-repository-governance.md` | FR-009 follow-up: migration kernel split, repository expansion policy, and DB operations governance |
| orchestrator | `docs/design_doc/orchestrator/27-grpc-control-plane-protection.md` | `docs/qa/orchestrator/65-grpc-control-plane-protection.md` | FR-013 closure: tower-composed control-plane protection, subject/global budgets, stream occupancy guards, audit fields, and pressure validation |
| orchestrator | `docs/design_doc/orchestrator/28-error-semantics-governance.md` | `docs/qa/orchestrator/66-error-semantics-governance.md` | FR-014 boundary error taxonomy, shared gRPC mapping, and critical-path diagnostics governance |
| orchestrator | `docs/design_doc/orchestrator/29-clone-reduction-and-shared-ownership.md` | `docs/qa/orchestrator/67-clone-reduction-and-shared-ownership.md`, `docs/qa/orchestrator/68-clone-reduction-follow-up.md` | FR-015 closure: shared runtime context fields, trace/graph borrow-first cleanup, scheduler follow-up hot paths, and persistence/export ownership tightening |
| orchestrator | `docs/design_doc/orchestrator/30-async-lock-model-alignment.md` | `docs/qa/orchestrator/69-async-lock-model-alignment.md` | FR-016 closure: config snapshots via `ArcSwap`, async telemetry locks, documented synchronous exceptions, and the governance gate |
| orchestrator | `docs/design_doc/orchestrator/43-step-variable-expansion-governance.md` | `docs/qa/orchestrator/82-step-variable-expansion-completeness.md` | Variable expansion completeness: coverage model for renderer helpers, runtime propagation, step-family mapping, and anomaly backstop |
| orchestrator | `docs/design_doc/orchestrator/92-scheduler-port-trait-inversion.md` | `docs/qa/orchestrator/102-core-crate-split-scheduler.md`, `docs/qa/orchestrator/78-worker-notify-wakeup.md` | scheduler_service.rs decomposition: TaskEnqueuer trait port, scheduling primitives to scheduler crate, worker helpers to service/system.rs |
| orchestrator | `docs/design_doc/orchestrator/96-crd-plugin-system.md` | `docs/qa/orchestrator/136-crd-plugin-system.md` | CRD plugin system: interceptor/transformer/cron plugins, policy governance, built-in tools |
| orchestrator | `docs/design_doc/orchestrator/99-linux-sandbox-filesystem-isolation.md` | `docs/qa/orchestrator/139-linux-sandbox-filesystem-isolation.md` | FR-091: Linux sandbox filesystem isolation via mount namespaces |
| orchestrator | `docs/design_doc/orchestrator/100-plugin-sandbox-isolation.md` | `docs/qa/orchestrator/140-plugin-sandbox-isolation.md` | Plugin sandbox isolation: TOCTOU defense, execution_profile, env sanitization, audit enhancement |
| orchestrator | `docs/design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md` | TBD | Decision record: replace one-shot shell-text agent contract with bidirectional stream-json + orchestrator-owned MCP tools; collapse coordination out of YAML/CEL; `StreamingAgentRunner` behind existing `RunnerExecutor` seam |
| orchestrator | `docs/design_doc/orchestrator/102-stream-json-event-ingestion.md` | TBD | Parse stream-json output into structured records: project `tool_use`/`tool_result`/`result` into the `events` table and onto `AgentOutput`; parse in validation, project in `record_phase_results`; additive, no schema change |
| orchestrator | `docs/design_doc/orchestrator/103-cel-stream-run-signals.md` | TBD | Surface streaming-run signals (`tools_called`, `tool_error_count`, `run_cost_usd`, …) to prehook/convergence/finalize CEL via a unified `bind_pipeline_vars`; coordination driven by what the agent did, not regex-scraped stdout |
| orchestrator | `docs/design_doc/orchestrator/105-process-timeline-read-model.md` | `docs/qa/orchestrator/142-process-timeline-read-model.md` | FR-095 semantic timeline projection, stable cursors, evidence references, and live operator UI |
| orchestrator | `docs/design_doc/orchestrator/106-attention-inbox.md` | `docs/qa/orchestrator/143-attention-inbox.md` | FR-096 durable human-action queue, governed actions, and keyboard-first default GUI |
| orchestrator | `docs/design_doc/orchestrator/107-handoff-and-safe-resume.md` | `docs/qa/orchestrator/144-handoff-and-safe-resume.md` | FR-097 deterministic handoffs, two-stage logical resume, fail-closed replay, and provider-neutral session reuse |
| orchestrator | `docs/design_doc/orchestrator/108-agent-session-control-plane.md` | `docs/qa/orchestrator/145-agent-session-control-plane.md` | FR-098 daemon-authoritative session observation, fenced writer control, restart reconciliation, and TaskDetail handoff |
| orchestrator | `docs/design_doc/orchestrator/109-source-events-and-slack-binding.md` | `docs/qa/orchestrator/146-source-events-and-slack-binding.md` | FR-099 durable provider-neutral ingestion, deterministic correlation, Slack pilot, audited actions, and Sources UI |
| orchestrator | `docs/design_doc/orchestrator/110-process-console-information-architecture.md` | `docs/qa/orchestrator/147-process-console-ui.md` | FR-100 Attention-first navigation, integrated process workspace, session re-entry, responsive accessibility, and rollout controls |
| orchestrator | `docs/design_doc/orchestrator/111-control-plane-action-audit-envelope.md` | `docs/qa/orchestrator/148-control-plane-action-audit-envelope.md` | FR-101 canonical mutation evidence, request-ID joins, retry safety, redaction, and compatibility enforcement |
| orchestrator | `docs/design_doc/orchestrator/112-agent-session-control-plane-hardening.md` | `docs/qa/orchestrator/149-agent-session-control-plane-hardening.md` | FR-102 restart reconciliation, bounded streams, fenced atomic input, PID identity, UI re-entry, and executable closure |
| orchestrator | `docs/design_doc/orchestrator/113-process-console-recovery-notifications-e2e.md` | `docs/qa/orchestrator/150-process-console-recovery-notifications-e2e.md` | FR-103 canonical reviewed recovery, actor-aware Attention notifications, real Tauri/gRPC vertical proof, and accessibility closure |
| orchestrator | `docs/design_doc/orchestrator/114-process-console-operational-metrics.md` | `docs/qa/orchestrator/151-process-console-operational-metrics.md` | FR-104 exact local product metrics, bounded read model, projector health, Operations dashboard, and performance closure |
| orchestrator | `docs/design_doc/orchestrator/115-session-runtime-policy-authority.md` | `docs/qa/orchestrator/152-session-runtime-policy-authority.md` | FR-105 deterministic `_system` RuntimePolicy authority, hot apply, restart persistence, and fail-closed Session gates |
| orchestrator | `docs/design_doc/orchestrator/116-process-console-release-acceptance.md` | `docs/qa/orchestrator/153-process-console-release-acceptance.md` | FR-106 clean-tree aggregate gate, migration identity, populated upgrade, release operations, and forward-only rollback |
| orchestrator | `docs/design_doc/orchestrator/117-process-console-qa-coverage-expansion.md` | `docs/qa/orchestrator/154-process-console-functional-ui-regression.md` | Risk-based unit and browser UI coverage expansion with honest source-wide measurement |
| orchestrator | `docs/design_doc/orchestrator/118-slack-reaction-source-event-contract.md` | `docs/qa/orchestrator/155-slack-reaction-source-event-contract.md` | FR-107 provider-neutral reaction contract, Slack normalization, non-mutating routing gate, and bounded Sources provenance |
| orchestrator | `docs/design_doc/orchestrator/119-source-task-template-skill-invocation.md` | `docs/qa/orchestrator/156-source-task-template-skill-invocation.md` | FR-108 native source-to-task recipe, deterministic safe preview, hot reload, and reference-governed deletion |
| orchestrator | `docs/design_doc/orchestrator/120-source-task-binding-badge-matching.md` | `docs/qa/orchestrator/157-source-task-binding-badge-matching.md` | FR-109 exact authenticated badge policy, conflict rejection, shared simulation/live matcher, audited lifecycle, and safe deletion |
| orchestrator | `docs/design_doc/orchestrator/121-slack-permalink-canonical-task-routing.md` | `docs/qa/orchestrator/158-slack-permalink-canonical-task-routing.md` | FR-110 bounded Slack permalink resolution, durable automation identity, canonical task/audit provenance, restart convergence, and role-aware deep links |
| orchestrator | `docs/design_doc/orchestrator/122-source-automation-reliability-operations.md` | `docs/qa/orchestrator/159-source-automation-reliability-operations.md` | FR-111 bounded route leases/retries, Attention recovery, safe operations, suspension, metrics, retention, and compatibility |
| orchestrator | `docs/design_doc/orchestrator/123-process-console-source-automation-ui.md` | `docs/qa/orchestrator/160-process-console-source-automation-ui.md` | FR-112 Sources → Automations templates, badge bindings, daemon draft preview/simulation, route diagnosis, audited CAS mutations, privacy, and accessibility |
| orchestrator | `docs/design_doc/orchestrator/124-slack-reaction-skill-automation-release.md` | `docs/qa/orchestrator/161-slack-reaction-skill-automation-release.md` | FR-113 clean-tree Slack Skill automation release gate, two-badge signed vertical flow, recovery, migration/compatible rollback, Tauri/UI, and operator guide |
| orchestrator | `docs/design_doc/orchestrator/125-managed-slack-connection-shared-oauth.md` | `docs/qa/orchestrator/162-managed-slack-connection-shared-oauth.md` | FR-114 official shared Slack App OAuth, independent Gateway, SourceConnection lifecycle, durable delivery, safe ownership transfer, and Connections UI |
| orchestrator | `docs/design_doc/orchestrator/126-dedicated-slack-app-auto-provisioning.md` | `docs/qa/orchestrator/163-dedicated-slack-app-auto-provisioning.md` | FR-115 per-workspace private Slack App manifest provisioning, local token custody, receipt-gated credential import, exact-App OAuth/events, and recovery Attention |
| orchestrator | `docs/design_doc/orchestrator/streaming-runner-pivot-overview.md` | — | Narrative overview tying 101→102→103 + the [showcase](../showcases/streaming-mark-done-convergence.md) into a one-page interview-ready tour |
