# Design Docs

This directory contains design documents captured from confirmed plans (plan mode output). They preserve context after implementation (goals, scope, tradeoffs, risks, observability, acceptance criteria) to reduce future iteration overhead.

Generation entry point:
- Before generating `docs/qa/**`, `qa-doc-gen` generates the corresponding `docs/design_doc/**` design docs (same module-based structure).

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
| orchestrator | `docs/design_doc/orchestrator/27-grpc-control-plane-protection.md` | `docs/qa/orchestrator/65-grpc-control-plane-protection.md` | FR-013 phase 1: request classification, subject/global budgets, stream occupancy guards, and protection audit fields |
