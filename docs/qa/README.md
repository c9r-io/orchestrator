# QA Docs

This directory contains reproducible, verifiable QA test documents.

## Source of Truth

- Runtime state source: SQLite (`data/agent_orchestrator.db`)
- YAML role: import/export/edit artifact (`apply`, `manifest export`, `edit export`)
- QA docs must not assume a mandatory `default.yaml` file is auto-generated.

## QA Contract

- Canonical CLI contract: `docs/qa/orchestrator/00-command-contract.md`
- Preferred entry point: `orchestrator <command>` (auto-builds + calls CLI client)
- Daemon: `./target/release/orchestratord --foreground --workers 2`
- CLI client: `./target/release/orchestrator <command>`
- Repository root is the default execution directory for all QA steps.

## Document Rules (Strict)

1. Keep each document to at most **5 scenarios**.
2. Every scenario must include Preconditions, Steps, and Expected Result.
3. Commands must align with the actual CLI surface in `crates/cli/src/cli.rs`.
4. Use `workspace info <workspace-id>` positional argument (no `--workspace-id`).
5. Do not use removed path assumptions like `cd orchestrator`.

## Test Scripts

Advanced scenarios use scripts in `docs/qa/script/`:

| Script | Purpose | Usage |
|--------|---------|-------|
| `test-task-pause-resume.sh` | Task pause/resume | `./docs/qa/script/test-task-pause-resume.sh [--workspace <id>] [--project <id>] [--json]` |
| `test-task-retry.sh` | Task item retry flow | `./docs/qa/script/test-task-retry.sh [--workspace <id>] [--project <id>] [--json]` |
| `test-three-phase-workflow.sh` | QA + Fix + Retest path | `./docs/qa/script/test-three-phase-workflow.sh [--workspace <id>] [--project <id>] [--json]` |

Concurrency policy for QA scripts:
- Prefer one unique `project` per scenario run.
- Do not delete `data/agent_orchestrator.db` during routine QA execution.
- Recreate per-project scaffolding via `orchestrator delete project/<project> --force`, remove `workspace/<project>`, then run `orchestrator apply -f <fixture> --project <project>`.

Project isolation requirements for QA execution:
- QA setup must treat `project` as the primary isolation boundary. Do not rely on global DB resets to obtain a clean environment.
- Before each isolated QA run, recreate the target project with the current CLI: run `orchestrator delete project/<project> --force`, remove `workspace/<project>`, then run `orchestrator apply -f <fixture> --project <project>`.
- All QA task creation, task execution, and follow-up inspection must explicitly bind to the intended project. Do not rely on ambient defaults when a project-scoped command is available.
- Fixture manifests used by QA must be applied only to support that QA run's project/workflow setup. Do not use QA fixtures to overwrite or replace the active orchestrator control-plane state for unrelated tasks.
- Do not run `orchestrator db reset --force`, `orchestrator db reset --include-config`, `orchestrator db reset --force --include-config`, `orchestrator --unsafe db reset`, or any variant of `db reset` as a QA scenario setup/cleanup step. The `--unsafe` flag bypasses force gates and is equally destructive. Use `delete project/<project> --force` for project-scoped isolation instead.
- Do not change `Defaults` to point the whole runtime at a QA-only workflow as part of scenario setup. QA fixtures must not hijack the default workspace/workflow used by unrelated runs such as `self-bootstrap`.

## Regression Runner

Unified CLI probe regression runner for automated scenario-group execution:

| Entry Point | Usage |
|-------------|-------|
| `./scripts/regression/run-cli-probes.sh` | Run all probe groups |
| `./scripts/regression/run-cli-probes.sh --group <name>` | Run a single group |
| `./scripts/regression/run-cli-probes.sh --list` | List available groups |
| `./scripts/regression/run-cli-probes.sh --json` | JSON output |

Available groups:

| Group | Scenario Script | Coverage |
|-------|----------------|----------|
| `task-create` | `probe-task-create.sh` | Task-scoped, item-scoped, and empty-workspace target resolution |
| `runtime-control` | `probe-runtime-control.sh` | Pause / resume lifecycle |
| `trace` | `probe-trace.sh` | Normal trace output and low-output anomaly detection |
| `low-output` | `probe-low-output.sh` | Low-output detection and active-output false-positive guard |

QA docs that reference the regression runner:

- `docs/qa/orchestrator/02-cli-task-lifecycle.md` — `--group task-create`, `--group runtime-control`, `--group low-output`
- `docs/qa/orchestrator/32-task-trace.md` — `--group trace`

## Lint Guard

Run:

```bash
./scripts/qa-doc-lint.sh
```

This checks:
- banned stale patterns (`cd orchestrator`, `--workspace-id`, `orchestrator agent health`, `orchestrator/config/default.yaml`, `config bootstrap --from`, `--config <file>`)
- workflow ID cross-reference: `--workflow <id>` in orchestrator QA docs must exist in fixture YAMLs
- edit subcommand structure: bare `edit <resource>` is banned (must use `edit export` or `edit open`)
- scenario count limit (<=5)
- orchestrator QA docs are indexed in this README

## Index

| Module | Doc | Scenarios | Notes |
|--------|-----|-----------|-------|
| orchestrator | `docs/qa/orchestrator/00-command-contract.md` | 4 | Canonical CLI command contract |
| orchestrator | `docs/qa/orchestrator/01-cli-agent-orchestration.md` | 5 | CLI lifecycle and apply dry-run |
| orchestrator | `docs/qa/orchestrator/02-cli-task-lifecycle.md` | 5 | Start/pause/resume/logs/retry |
| orchestrator | `docs/qa/orchestrator/03-cli-edit-export.md` | 4 | Edit and export commands |
| orchestrator | `docs/qa/orchestrator/04-cli-config-db.md` | 4 | Manifest apply and DB reset |
| orchestrator | `docs/qa/orchestrator/05-workflow-execution.md` | 5 | Workflow execution core scenarios |
| orchestrator | `docs/qa/orchestrator/06-cli-output-formats.md` | 5 | JSON/YAML output validation |
| orchestrator | `docs/qa/orchestrator/07-capability-orchestration.md` | 5 | Capability-driven orchestration core |
| orchestrator | `docs/qa/orchestrator/08-project-namespace.md` | 5 | Project namespace behavior |
| orchestrator | `docs/qa/orchestrator/09-agent-selection-strategy.md` | 5 | Multi-factor selection strategy |
| orchestrator | `docs/qa/orchestrator/10-agent-collaboration.md` | 5 | AgentOutput and MessageBus |
| orchestrator | `docs/qa/orchestrator/10-config-error-handling.md` | 4 | Config error paths |
| orchestrator | `docs/qa/orchestrator/11-config-creation-flow.md` | 4 | Apply-based resource creation |
| orchestrator | `docs/qa/orchestrator/12-config-validation.md` | 4 | Manifest validate command |
| orchestrator | `docs/qa/orchestrator/13-dynamic-orchestration.md` | 5 | Dynamic orchestration unit-level validation |
| orchestrator | `docs/qa/orchestrator/14-config-validation-enhanced.md` | 5 | Enhanced config validation |
| orchestrator | `docs/qa/orchestrator/15-workflow-multi-target-files.md` | 1 | Split from doc 05 |
| orchestrator | `docs/qa/orchestrator/16-capability-config-view-fields.md` | 1 | Split from doc 07 |
| orchestrator | `docs/qa/orchestrator/17-dynamic-yaml-integration.md` | 1 | Split from doc 13 |
| orchestrator | `docs/qa/orchestrator/18-kubectl-style-extensions.md` | 4 | Get list / create / stdin apply / label selector |
| orchestrator | `docs/qa/orchestrator/19-scheduler-repository-refactor-regression.md` | 5 | P0/P1 scheduler repository refactor regression and observability checks |
| orchestrator | `docs/qa/orchestrator/20-structured-output-worker-scheduler.md` | 5 | Structured output validation + queue-only daemon worker scheduling mainline |
| orchestrator | `docs/qa/orchestrator/21-runner-security-observability.md` | 5 | Runner allowlist boundary, redaction, and task execution metrics observability |
| orchestrator | `docs/qa/orchestrator/22-performance-io-queue-optimizations.md` | 5 | Transactional phase-result persistence, bounded output reads, true tail, and atomic multi-worker queue checks |
| orchestrator | `docs/qa/orchestrator/23-dynamic-plan-step-exec-tty.md` | 5 | Dynamic `plan` step insertion, step-level `tty`, and `exec` target contract |
| orchestrator | `docs/qa/orchestrator/24-exec-interactive-simulation.md` | 5 | Interactive execution simulation via stdin pipe/here-doc and reusable QA script |
| orchestrator | `docs/qa/orchestrator/25-session-attach-reattach.md` | 5 | Real session lifecycle: task session list/info/close, attach, re-attach, and close rejection checks |
| orchestrator | `docs/qa/orchestrator/26-self-bootstrap-workflow.md` | 5 | Self-bootstrap workflow: extended steps, pipeline variables, prehook-gated fix, checkpoint/rollback |
| orchestrator | `docs/qa/orchestrator/27-self-test-step.md` | 5 | Self-test builtin step: cargo check, test --lib, pipeline variables, self-referential safety |
| orchestrator | `docs/qa/orchestrator/28-self-bootstrap-pipeline.md` | 5 | Self-bootstrap pipeline: full SDLC, ticket fix chain, pipeline variables (Part 2) |
| orchestrator | `docs/qa/orchestrator/29-step-scope-segment-execution.md` | 5 | StepScope segment execution: task-scoped steps run once, item-scoped fan out per QA file |
| orchestrator | `docs/qa/orchestrator/30-unified-step-execution-model.md` | 5 | Unified step execution: WorkflowStepType removal, semantic resolution, StepBehavior alignment, and static-check parity |
| orchestrator | `docs/qa/orchestrator/31-runner-policy-defaults-compatibility.md` | 2 | Split from doc 21: unsafe/legacy policy compatibility checks |
| orchestrator | `docs/qa/orchestrator/32-task-trace.md` | 5 | Task trace: execution timeline reconstruction and anomaly detection |
| orchestrator | `docs/qa/orchestrator/33-fatal-agent-error-detection.md` | 1 | Regression: fatal provider stderr must override outer exit code 0 and mark runs failed |
| orchestrator | `docs/qa/orchestrator/34-config-heal-auditability.md` | 5 | Config self-heal audit log persistence, heal-log CLI, check enhancement |
| orchestrator | `docs/qa/orchestrator/35-legacy-observability-backfill.md` | 5 | Legacy event step_scope backfill, unknown→legacy display, backfill-events CLI |
| orchestrator | `docs/qa/orchestrator/36-structured-logging.md` | 5 | Structured logging bootstrap, CLI log overrides, stderr/stdout separation, and rolling file output |
| orchestrator | `docs/qa/orchestrator/37-envstore-secretstore-resources.md` | 5 | EnvStore/SecretStore resource apply, get, delete, export, and cross-kind isolation |
| orchestrator | `docs/qa/orchestrator/38-agent-env-resolution.md` | 5 | Agent env resolution: direct value, fromRef, refValue, validation, and secret redaction |
| orchestrator | `docs/qa/orchestrator/39-prompt-delivery.md` | 5 | PromptDelivery abstraction: default arg, stdin, file, env modes, preflight validation |
| orchestrator | `docs/qa/orchestrator/40-custom-resource-definitions.md` | 5 | CRD extension system: registration, validation, get/describe/delete, cascade protection, export round-trip |
| orchestrator | `docs/qa/orchestrator/41-project-scoped-agent-selection.md` | 5 | Project-scoped agent selection: apply --project, strict isolation, ticket cleanup, cross-project isolation |
| orchestrator | `docs/qa/orchestrator/42-crd-unified-resource-store.md` | 5 | Unified CRD ResourceStore: builtin CRD bootstrap, CrdProjectable round-trip, targeted writeback, apply/delete integration, edge cases |
| orchestrator | `docs/qa/orchestrator/43-cli-force-gate-audit.md` | 5 | CLI force gate audit: backfill-events, task retry, and existing force-gate regression checks |
| orchestrator | `docs/qa/orchestrator/44-parallel-item-execution.md` | 5 | Parallel item execution: max_parallel config, semaphore-gated JoinSet, RunningTask::fork(), pool size 20 |
| orchestrator | `docs/qa/orchestrator/45-cli-unsafe-mode.md` | 5 | CLI --unsafe mode: force-gate bypass, runtime runner policy override, audit event, warning banner |
| orchestrator | `docs/qa/orchestrator/46-persistent-store.md` | 5 | WP01 persistent store: CRD apply, local/command backends, schema validation, project isolation |
| orchestrator | `docs/qa/orchestrator/47-task-spawning.md` | 5 | WP02 task spawning: SpawnTask/SpawnTasks post-actions, spawn depth safety, task lineage tracking |
| orchestrator | `docs/qa/orchestrator/48-dynamic-items-selection.md` | 5 | WP03 dynamic items + selection: GenerateItems post-action, item_select builtin, min/max/threshold/weighted strategies |
| orchestrator | `docs/qa/orchestrator/49-invariant-constraints.md` | 5 | WP04 invariant constraints: command checks, protected files, checkpoint filtering, on_violation actions |
| orchestrator | `docs/qa/orchestrator/50-engine-wiring-store-invariant-itemselect.md` | 5 | WP01-WP04 engine wiring: store I/O, PostAction::StorePut, invariant checkpoints |
| orchestrator | `docs/qa/orchestrator/51-primitive-composition.md` | 5 | WP05 primitive composition: Store+Spawning, Store+Items, Invariant+Selection pairwise/triple |
| orchestrator | `docs/qa/orchestrator/52-engine-wiring-dynamic-items-selection.md` | 2 | Split from doc 50: pending_generate_items consumption, item_select orchestration |
| orchestrator | `docs/qa/orchestrator/53-client-server-architecture.md` | 5 | C/S architecture: daemon lifecycle, gRPC communication, embedded workers, service layer |
| orchestrator | `docs/qa/orchestrator/54-step-execution-profiles.md` | 5 | Step-level ExecutionProfile: resource round-trip, validation, mixed host/sandbox routing, compatibility default |
| orchestrator | `docs/qa/orchestrator/55-sandbox-write-boundaries.md` | 2 | Sandbox file write boundaries: deny workspace-root writes, allow declared writable subtree |
| orchestrator | `docs/qa/orchestrator/56-sandbox-resource-network-enforcement.md` | 3 | Sandbox resource/network enforcement: open-files limit event, network deny event, unsupported allowlist gating |
| orchestrator | `docs/qa/orchestrator/57-sandbox-resource-limits-extended.md` | 3 | Sandbox resource limits for CPU, memory, processes |
| orchestrator | `docs/qa/orchestrator/58-control-plane-security.md` | 5 | Secure TCP control plane: mTLS bootstrap, host-user client config, role-based RPC authorization, audit persistence |
| orchestrator | `docs/qa/orchestrator/59-dynamic-dag-mainline-execution.md` | 5 | FR-004: explicit `dynamic_dag` mode, CEL trigger validation, graph materialization, persisted graph debug bundles, and DAG debug view |
| orchestrator | `docs/qa/orchestrator/60-daemon-lifecycle-runtime-metrics.md` | 4 | FR-005: daemon runtime snapshot, live worker/task counters, graceful drain, and restart-state reset |
| orchestrator | `docs/qa/orchestrator/61-chain-steps-execution.md` | 4 | FR-008: chain_steps runtime contract, runtime plan preservation, parent/child failure ordering, and trace compatibility |
| orchestrator | `docs/qa/orchestrator/62-database-persistence-bootstrap-repositories.md` | 5 | FR-009 Phase 1: persistence bootstrap ownership, public ensure_column removal, and repository-backed session/store boundaries |
| orchestrator | `docs/qa/orchestrator/63-database-migration-kernel-and-repository-governance.md` | 6 | FR-009 follow-up governance for migration kernel split, repository expansion boundaries, and DB operations visibility |
| orchestrator | `docs/qa/orchestrator/64-secretstore-key-lifecycle.md` | 5 | FR-012: SecretStore key lifecycle — legacy migration, rotation, resume, revocation, audit history |
| orchestrator | `docs/qa/orchestrator/65-grpc-control-plane-protection.md` | 5 | FR-013 closure: protection config bootstrap, secure-TCP rate limits, stream occupancy limit, UDS fallback protection, and repeatable pressure validation |
| orchestrator | `docs/qa/orchestrator/66-error-semantics-governance.md` | 4 | FR-014: boundary error taxonomy, shared gRPC status mapping, CLI error rendering, and regression verification |
| orchestrator | `docs/qa/orchestrator/67-clone-reduction-and-shared-ownership.md` | 5 | FR-015 clone reduction: shallow-shared scheduler runtime fields, owned daemon summary mapping, builtin execution cleanup, and trace hotspot regression coverage |
| orchestrator | `docs/qa/orchestrator/68-clone-reduction-follow-up.md` | 5 | FR-015 follow-up: chain-step/task-fanout cleanup, graph replay ownership tightening, db-write owned fast-paths, export metadata helpers, and secret-key audit assembly |
| orchestrator | `docs/qa/orchestrator/69-async-lock-model-alignment.md` | 6 | FR-016: config runtime snapshots, async health/metrics locks, governance-gate regression, and documented sync exceptions |
| orchestrator | `docs/qa/orchestrator/70-libc-cross-platform-compilation.md` | 5 | FR-019: libc workspace dep unification, cfg(unix) gating, SIGXCPU test guard, and 5-target cross-compile CI |
| orchestrator | `docs/qa/orchestrator/smoke-orchestrator.md` | - | Smoke test: core CLI and DB initialization |
| script | `docs/qa/script/` | 6 | Executable QA scripts |
| self-bootstrap | `docs/qa/self-bootstrap/smoke-self-bootstrap.md` | - | Smoke test: self-bootstrap basics |
| self-bootstrap | `docs/qa/self-bootstrap/01-survival-binary-checkpoint-self-test.md` | 5 | Survival Layer 1-2: binary snapshot/restore and self-test acceptance gate |
| self-bootstrap | `docs/qa/self-bootstrap/02-survival-enforcement-watchdog.md` | 5 | Survival Layer 3-4: self-referential enforcement and watchdog script |
| self-bootstrap | `docs/qa/self-bootstrap/05-survival-smoke-binary-snapshot.md` | 5 | Unit tests for snapshot_binary() and restore_binary_snapshot() |
| self-bootstrap | `docs/qa/self-bootstrap/06-survival-smoke-binary-snapshot-verification.md` | 5 | Binary snapshot verification function and integration test |
| self-bootstrap | `docs/qa/self-bootstrap/07-self-restart-process-continuity.md` | 5 | Self-restart builtin step, restart_pending resumption, daemon restart loop, priority claiming |
| self-bootstrap | `docs/qa/self-bootstrap/08-build-version-hash.md` | 5 | Build version hash: compile-time git hash/timestamp, version subcommand, restart event enrichment |
| self-bootstrap | `docs/qa/self-bootstrap/09-self-restart-old-new-sha256-audit.md` | 4 | Self-restart old/new binary SHA256 audit chain: old_binary_sha256, new_binary_sha256, binary_changed, backward compat |
| self-bootstrap | `docs/qa/self-bootstrap/10-self-referential-safety-policy-alignment.md` | 5 | FR-003 policy alignment: required self-referential safeguards, warning-only binary snapshot, probe workspace binding, and audit diagnostics |
| self-bootstrap | `docs/qa/self-bootstrap/04-cycle2-validation-and-runtime-timestamps.md` | 2 | Regression: fixed two-cycle QA validation chain and task/item runtime timestamps |
| self-bootstrap | `docs/qa/self-bootstrap/scenario2-binary-rollback.md` | 1 | Binary snapshot restoration on auto-rollback |
| self-bootstrap | `docs/qa/self-bootstrap/scenario3-binary-skip-disabled.md` | 1 | Binary snapshot skip when disabled |
| self-bootstrap | `docs/qa/self-bootstrap/scenario4-self-test-pass.md` | 1 | Self-test step passes all three phases |
