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
6. GUI documents must include an explicit **Entry Visibility** scenario that verifies the normal discoverable route.
7. Every document opens with lifecycle frontmatter (see below), alongside the existing
   `self_referential_safe` declaration.

## Lifecycle Frontmatter (Enforced)

`scripts/qa/doc-lifecycle.rb` requires every document here to declare whether it still describes
the live behaviour, and fails CI when one arrives without it:

```yaml
---
lifecycle: active            # active | superseded
related_fr: FR-132           # optional; omit rather than guess
self_referential_safe: true  # existing safety declaration, unchanged
---
```

When a later change replaces the behaviour a document verifies, set `lifecycle: superseded` and add
`superseded_by:` naming the successor's repository-relative path. Regenerate the reverse index in
the same commit with `ruby scripts/qa/doc-lifecycle.rb --emit-index --write`.

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
| orchestrator | `docs/qa/orchestrator/10-agent-collaboration.md` | 5 | AgentOutput and collaboration |
| orchestrator | `docs/qa/orchestrator/10-config-error-handling.md` | 4 | Config error paths |
| orchestrator | `docs/qa/orchestrator/11-config-creation-flow.md` | 4 | Apply-based resource creation |
| orchestrator | `docs/qa/orchestrator/12-config-validation.md` | 4 | Manifest validate command |
| orchestrator | `docs/qa/orchestrator/13-dynamic-orchestration.md` | 5 | Dynamic orchestration unit-level validation |
| orchestrator | `docs/qa/orchestrator/14-config-validation-enhanced.md` | 5 | Enhanced config validation |
| orchestrator | `docs/qa/orchestrator/15-workflow-multi-target-files.md` | 1 | Split from doc 05 |
| orchestrator | `docs/qa/orchestrator/16-capability-config-view-fields.md` | 1 | Split from doc 07 |
| orchestrator | `docs/qa/orchestrator/17-dynamic-yaml-integration.md` | 1 | Split from doc 13 |
| orchestrator | `docs/qa/orchestrator/18-kubectl-style-extensions.md` | 3 | Get list / create / stdin apply / label selector |
| orchestrator | `docs/qa/orchestrator/19-scheduler-repository-refactor-regression.md` | 5 | P0/P1 scheduler repository refactor regression and observability checks |
| orchestrator | `docs/qa/orchestrator/20-structured-output-worker-scheduler.md` | 5 | Structured output validation + queue-only daemon worker scheduling mainline |
| orchestrator | `docs/qa/orchestrator/21-runner-security-observability.md` | 5 | Runner allowlist boundary, redaction, and task execution metrics observability |
| orchestrator | `docs/qa/orchestrator/22-performance-io-queue-optimizations.md` | 5 | Transactional phase-result persistence, bounded output reads, true tail, and atomic multi-worker queue checks |
| orchestrator | `docs/qa/orchestrator/23-dynamic-plan-step-exec-tty.md` | - | Dynamic `plan` step insertion, step-level `tty`, and `exec` target contract |
| orchestrator | `docs/qa/orchestrator/24-exec-interactive-simulation.md` | - | Interactive execution simulation via stdin pipe/here-doc and reusable QA script |
| orchestrator | `docs/qa/orchestrator/25-session-attach-reattach.md` | - | Real session lifecycle: task session list/info/close, attach, re-attach, and close rejection checks |
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
| orchestrator | `docs/qa/orchestrator/51-primitive-composition.md` | 2 | WP05 primitive composition: Store+Spawning, Store+Invariants (FR-149 removed the three WP03 dynamic-items scenarios) |
| orchestrator | `docs/qa/orchestrator/52-engine-wiring-dynamic-items-selection.md` | 2 | Split from doc 50: pending_generate_items consumption, item_select orchestration |
| orchestrator | `docs/qa/orchestrator/53-client-server-architecture.md` | 5 | C/S architecture: daemon lifecycle, gRPC communication, embedded workers, service layer |
| orchestrator | `docs/qa/orchestrator/54-step-execution-profiles.md` | 5 | Step-level ExecutionProfile: resource round-trip, validation, mixed host/sandbox routing, compatibility default |
| orchestrator | `docs/qa/orchestrator/55-sandbox-write-boundaries.md` | 2 | Sandbox file write boundaries: deny workspace-root writes, allow declared writable subtree |
| orchestrator | `docs/qa/orchestrator/56-sandbox-denial-anomaly-trace.md` | 2 | Sandbox denial anomaly trace and empty-change guard |
| orchestrator | `docs/qa/orchestrator/56-sandbox-resource-network-enforcement.md` | 3 | Sandbox resource/network enforcement: open-files limit event, network deny event, unsupported allowlist gating |
| orchestrator | `docs/qa/orchestrator/57-sandbox-resource-limits-extended.md` | 3 | Sandbox resource limits for CPU, memory, processes |
| orchestrator | `docs/qa/orchestrator/58-control-plane-security.md` | 5 | Secure TCP control plane: mTLS bootstrap, host-user client config, role-based RPC authorization, audit persistence |
| orchestrator | `docs/qa/orchestrator/58b-control-plane-uds-policy.md` | 4 | UDS role boundary, flag/policy precedence, and audit enrichment |
| orchestrator | `docs/qa/orchestrator/59-dynamic-dag-mainline-execution.md` | 5 | FR-004: explicit `dynamic_dag` mode, CEL trigger validation, graph materialization, persisted graph debug bundles, and DAG debug view |
| orchestrator | `docs/qa/orchestrator/60-daemon-lifecycle-runtime-metrics.md` | 4 | FR-005: daemon runtime snapshot, live worker/task counters, graceful drain, and restart-state reset |
| orchestrator | `docs/qa/orchestrator/61-chain-steps-execution.md` | 4 | FR-008: chain_steps runtime contract, runtime plan preservation, parent/child failure ordering, and trace compatibility |
| orchestrator | `docs/qa/orchestrator/62-database-persistence-bootstrap-repositories.md` | 5 | FR-009 Phase 1: persistence bootstrap ownership, public ensure_column removal, and repository-backed session/store boundaries |
| orchestrator | `docs/qa/orchestrator/63-database-migration-kernel-and-repository-governance.md` | 5 | FR-009 follow-up governance for migration kernel split, repository expansion boundaries, and DB operations visibility |
| orchestrator | `docs/qa/orchestrator/64-secretstore-key-lifecycle.md` | 5 | FR-012: SecretStore key lifecycle — legacy migration, rotation, resume, revocation, audit history |
| orchestrator | `docs/qa/orchestrator/65-grpc-control-plane-protection.md` | 5 | FR-013 closure: protection config bootstrap, secure-TCP rate limits, stream occupancy limit, UDS fallback protection, and repeatable pressure validation |
| orchestrator | `docs/qa/orchestrator/66-error-semantics-governance.md` | - | FR-014: boundary error taxonomy, shared gRPC status mapping, CLI error rendering, and regression verification |
| orchestrator | `docs/qa/orchestrator/67-clone-reduction-and-shared-ownership.md` | - | FR-015 clone reduction: shallow-shared scheduler runtime fields, owned daemon summary mapping, builtin execution cleanup, and trace hotspot regression coverage |
| orchestrator | `docs/qa/orchestrator/68-clone-reduction-follow-up.md` | - | FR-015 follow-up: chain-step/task-fanout cleanup, graph replay ownership tightening, db-write owned fast-paths, export metadata helpers, and secret-key audit assembly |
| orchestrator | `docs/qa/orchestrator/69-async-lock-model-alignment.md` | - | FR-016: config runtime snapshots, async health/metrics locks, governance-gate regression, and documented sync exceptions |
| orchestrator | `docs/qa/orchestrator/70-libc-cross-platform-compilation.md` | - | FR-019: libc workspace dep unification, cfg(unix) gating, SIGXCPU test guard, and 5-target cross-compile CI |
| orchestrator | `docs/qa/orchestrator/71-automate-protoc-dependency.md` | - | FR-020: automate protoc dependency, PROTOC env var override, CI enforcement |
| orchestrator | `docs/qa/orchestrator/72-audit-reduce-expect-calls.md` | - | FR-021: audit and reduce expect() calls, deny-level lint enforcement |
| orchestrator | `docs/qa/orchestrator/73-integration-test-coverage.md` | 5 | FR-023: integration test coverage for CLI-daemon-core interaction |
| orchestrator | `docs/qa/orchestrator/73b-integration-test-coverage-advanced.md` | 3 | FR-023: multi-cycle loop, gRPC compat, full regression (split from doc 73) |
| orchestrator | `docs/qa/orchestrator/74-audit-unsafe-blocks.md` | - | FR-024: audit unsafe blocks, SAFETY comment enforcement |
| orchestrator | `docs/qa/orchestrator/75-public-api-doc-comments.md` | 5 | FR-022: public API doc comment governance and lint enforcement |
| orchestrator | `docs/qa/orchestrator/75b-public-api-doc-comments-legacy.md` | 1 | FR-022: legacy exemption cleanup (split from doc 75) |
| orchestrator | `docs/qa/orchestrator/76-config-load-module-split.md` | - | FR-025: config_load module split and responsibility segregation |
| orchestrator | `docs/qa/orchestrator/77-event-table-ttl-archival.md` | 5 | Event table TTL and archival: event stats, cleanup, archive to JSONL |
| orchestrator | `docs/qa/orchestrator/78-worker-notify-wakeup.md` | - | FR-027: worker notify wakeup governance, wake-file removal |
| orchestrator | `docs/qa/orchestrator/79-benchmark-score-capture.md` | - | Historical FR-028 capture/JSONPath evidence; production path retired by FR-125 / QA-175 |
| orchestrator | `docs/qa/orchestrator/80-item-scoped-git-worktree-isolation.md` | 4 | Item-scoped git worktree isolation: config round-trip, vendored protoc, workspace regression, self-evolution manifest |
| orchestrator | `docs/qa/orchestrator/81-self-evolution-db-schema-alignment.md` | - | FR-030: self-evolution DB schema alignment, monitoring queries |
| orchestrator | `docs/qa/orchestrator/82-step-variable-expansion-completeness.md` | 5 | Variable expansion completeness: renderer helpers, runtime propagation, step-family coverage matrix, and unexpanded-placeholder anomaly guard |
| orchestrator | `docs/qa/orchestrator/83-generate-items-mixed-text-extraction.md` | 5 | **superseded** (FR-149) — GenerateItems extraction from non-pure-JSON agent output; the post-action was retired by DD-137 |
| orchestrator | `docs/qa/orchestrator/84-generate-items-regression-narrowing.md` | 3 | **superseded** (FR-149) — generate_items regression narrowing; the post-action was retired by DD-137 |
| orchestrator | `docs/qa/orchestrator/85-daemon-crash-resilience.md` | 5 | FR-032: worker auto-respawn, stale PID crash recovery, panic hook crash log, supervisor health monitoring, total_worker_restarts metric |
| orchestrator | `docs/qa/orchestrator/86-orphaned-running-items-recovery.md` | 5 | FR-033: orphaned running items auto-recovery, startup recovery, stall detection, CLI task recover, audit events |
| orchestrator | `docs/qa/orchestrator/87-self-referential-daemon-pid-guard.md` | 4 | FR-034: daemon PID kill guard for self-referential workspace safety |
| orchestrator | `docs/qa/orchestrator/88-degenerate-cycle-loop-guard.md` | 5 | FR-035: rapid cycle detection (L2), degenerate loop trace anomaly, blocked item recovery, circuit breaker unit tests |
| orchestrator | `docs/qa/orchestrator/89-plan-output-context-overflow-mitigation.md` | 5 | FR-036: plan output context overflow mitigation, stream-JSON result extraction |
| orchestrator | `docs/qa/orchestrator/89b-plan-output-spill-regression.md` | 2 | FR-036: spill regression and stream-JSON extraction (split from doc 89) |
| orchestrator | `docs/qa/orchestrator/90-unquoted-json-extraction.md` | 5 | FR-031: generate_items unquoted JSON extraction, LLM non-standard output tolerance |
| orchestrator | `docs/qa/orchestrator/90b-unquoted-json-extraction-advanced.md` | 5 | FR-031: file path repair, e2e extraction, regression (split from doc 90) |
| orchestrator | `docs/qa/orchestrator/91-daemon-crash-resilience.md` | 5 | FR-032: daemon crash resilience, worker survival, health monitoring |
| orchestrator | `docs/qa/orchestrator/91b-daemon-crash-resilience-shutdown.md` | 2 | FR-032: graceful shutdown and full regression (split from doc 91) |
| orchestrator | `docs/qa/orchestrator/92-dynamic-items-cycle-overflow.md` | 4 | **superseded** (FR-149) — FR-037 max_cycles enforcement, reached through the retired generate_items post-action |
| orchestrator | `docs/qa/orchestrator/93-inflight-step-completion-race.md` | 5 | FR-038: daemon restart in-flight step completion race condition |
| orchestrator | `docs/qa/orchestrator/94-trigger-resource-cron-event-driven.md` | 5 | FR-039: trigger resource cron & event-driven task creation |
| orchestrator | `docs/qa/orchestrator/94b-trigger-resource-advanced.md` | 2 | FR-039: trigger suspend/resume and preflight check (split from doc 94) |
| orchestrator | `docs/qa/orchestrator/95-prehook-self-referential-safe-filter.md` | 1 | Prehook self-referential safe filter for QA doc execution |
| orchestrator | `docs/qa/orchestrator/96-self-restart-socket-continuity.md` | 5 | Self-restart socket and PID file continuity across exec() lifecycle |
| orchestrator | `docs/qa/orchestrator/97-follow-task-logs-callback.md` | 3 | FR-042: follow_task_logs callback refactor, gRPC TaskFollow log delivery |
| orchestrator | `docs/qa/orchestrator/98-convergence-expression.md` | - | FR-043: convergence_expr CEL-based loop termination |
| orchestrator | `docs/qa/orchestrator/99-long-lived-command-guard.md` | 4 | FR-045: task watch --timeout, stall auto-termination, QA agent timeout guidance |
| orchestrator | `docs/qa/orchestrator/100-agent-subprocess-daemon-pid-guard.md` | 4 | FR-046: agent subprocess daemon PID guard with CLAUDE.md + hooks injection |
| orchestrator | `docs/qa/orchestrator/100-agent-command-rules-step-vars.md` | 5 | FR-084: agent command_rules CEL selection, step_vars overlay, command_rule_index audit |
| orchestrator | `docs/qa/orchestrator/100b-agent-command-rules-step-vars-advanced.md` | 3 | FR-084: DB migration, YAML manifest parsing for command_rules and step_vars (split from doc 100) |
| orchestrator | `docs/qa/orchestrator/101-core-crate-split-config.md` | - | FR-047: core crate split phase 1 — orchestrator-config extraction |
| orchestrator | `docs/qa/orchestrator/102-core-crate-split-scheduler.md` | 5 | FR-048: core crate split phase 2+3 — orchestrator-scheduler extraction and scheduler_service.rs decomposition |
| orchestrator | `docs/qa/orchestrator/103-prehook-pipeline-vars.md` | - | FR-049: prehook CEL pipeline variables — type inference, JSON array `in`, truncation skip, builtin precedence |
| orchestrator | `docs/qa/orchestrator/104-cli-uds-fallback-robustness.md` | - | FR-050: CLI UDS fallback robustness — local socket priority, env override, home-dir TCP fallback |
| orchestrator | `docs/qa/orchestrator/105-workflow-yaml-unknown-field-warning.md` | - | FR-051: workflow YAML unknown field warnings and CEL prehook variable cross-check |
| orchestrator | `docs/qa/orchestrator/106-inflight-wait-heartbeat-aware-timeout.md` | - | FR-052: inflight wait heartbeat-aware timeout, configurable grace period, diagnostic events |
| orchestrator | `docs/qa/orchestrator/107-parallel-dispatch-completeness-guard.md` | - | FR-053: parallel dispatch completeness guard, dispatched_count accuracy, error propagation |
| orchestrator | `docs/qa/orchestrator/108-incremental-item-progress.md` | - | FR-054: incremental item progress, real-time step-level counters, batch finalize idempotency |
| orchestrator | `docs/qa/orchestrator/109-parallel-spawn-stagger-delay.md` | 5 | FR-055: parallel spawn stagger delay, workflow/step-level config, sequential path bypass |
| orchestrator | `docs/qa/orchestrator/109b-parallel-spawn-stagger-delay-compat.md` | 1 | FR-055: unknown-field warning compatibility (split from doc 109) |
| orchestrator | `docs/qa/orchestrator/110-agent-health-policy-configuration.md` | 5 | FR-056: agent health policy configuration, workspace fallback, disease disable, agent override |
| orchestrator | `docs/qa/orchestrator/110b-agent-health-policy-advanced.md` | 2 | FR-056: capability threshold and check output (split from doc 110) |
| orchestrator | `docs/qa/orchestrator/111-daemon-proper-daemonize.md` | - | FR-057: proper Unix daemonization, SIGHUP survival, daemon stop/status CLI |
| orchestrator | `docs/qa/orchestrator/112-scenario-level-self-referential-safety.md` | 5 | Scenario-level self-referential safety: prehook filter, agent isolation, workspace binding |
| orchestrator | `docs/qa/orchestrator/113-logging-env-var-override.md` | 5 | FR-061: ORCHESTRATOR_LOG/RUST_LOG/ORCHESTRATOR_LOG_FORMAT env var override |
| orchestrator | `docs/qa/orchestrator/114-agent-health-state-observability.md` | 5 | FR-062: agent health state CLI observability via `agent list` and `task info` |
| orchestrator | `docs/qa/orchestrator/115-agent-mailbox-session-communication.md` | 7 | FR-065: Agent 间通信接口草案 — Mailbox + Session 设计验证 |
| orchestrator | `docs/qa/orchestrator/116-gui-architecture-tauri-grpc.md` | 7 | FR-063: GUI 架构 Tauri 2.x + gRPC 安全客户端验证 |
| orchestrator | `docs/qa/orchestrator/117-gui-uiux-wish-pool-progress.md` | 11 | GUI 用户界面 — 许愿池 + 进度观察 + 专家模式 |
| orchestrator | `docs/qa/orchestrator/118-gui-realtime-wish-isolation.md` | 7 | GUI 实时状态推送与许愿池数据隔离 |
| orchestrator | `docs/qa/orchestrator/119-gui-cli-rpc-parity.md` | 4 | GUI CLI 功能对齐 — RPC 覆盖补全 |
| orchestrator | `docs/qa/orchestrator/120-gui-connection-resilience.md` | 4 | GUI 连接韧性 — 向导 / 重连 / 流式 |
| orchestrator | `docs/qa/orchestrator/120b-gui-notification-error-humanization.md` | 4 | GUI 系统通知与错误信息人性化 |
| orchestrator | `docs/qa/orchestrator/121-gui-polish-visual.md` | 4 | GUI 体验打磨 — 主题 / 动画 / DAG / 日志 |
| orchestrator | `docs/qa/orchestrator/121b-gui-i18n-ux.md` | 3 | GUI i18n / 响应式 / 构建分发 |
| orchestrator | `docs/qa/orchestrator/122-evo-apply-winner-observability.md` | - | FR-070 item_select/diff observability; capture scenarios superseded by FR-125 / QA-175 |
| orchestrator | `docs/qa/orchestrator/123-open-source-compliance.md` | 6 | FR-071: open-source compliance — LICENSE, CHANGELOG, templates, release |
| orchestrator | `docs/qa/orchestrator/124-homebrew-tap-distribution.md` | - | FR-072: Homebrew tap formula, cargo publish, release workflow distribution |
| orchestrator | `docs/qa/orchestrator/125-documentation-site.md` | 5 | FR-073: VitePress doc site, EN/ZH landing, search, language switcher |
| orchestrator | `docs/qa/orchestrator/125b-documentation-site-advanced.md` | 4 | FR-073: VitePress doc site, guide nav, "Why?" page, README check, Cloudflare deploy |
| orchestrator | `docs/qa/orchestrator/126-task-items-event-list-cli.md` | - | FR-078: task items and event list CLI commands with filters and JSON output |
| orchestrator | `docs/qa/orchestrator/127-data-lifecycle-governance.md` | - | FR-079: db status/vacuum/cleanup, auto log and task retention |
| orchestrator | `docs/qa/orchestrator/128-webhook-trigger-infrastructure.md` | - | FR-080: webhook server, trigger firing, HMAC auth, project scope |
| orchestrator | `docs/qa/orchestrator/129-per-trigger-webhook-auth-cel-filter.md` | 5 | FR-081: per-trigger webhook secret from SecretStore, multi-key rotation |
| orchestrator | `docs/qa/orchestrator/129b-per-trigger-webhook-auth-cel-filter-advanced.md` | 3 | FR-081: global secret fallback, CEL filter unit test (split from doc 129) |
| orchestrator | `docs/qa/orchestrator/130-integration-manifest-packages.md` | 5 | FR-082: Slack/GitHub/LINE integration manifest packages |
| orchestrator | `docs/qa/orchestrator/130b-integration-manifest-packages-advanced.md` | 2 | FR-082: secret rotation showcase, README completeness (split from doc 130) |
| orchestrator | `docs/qa/orchestrator/131-workflow-template-library.md` | 5 | FR-077: workflow template YAML structure and capability matching |
| orchestrator | `docs/qa/orchestrator/131b-workflow-template-library-advanced.md` | 5 | FR-077: echo agents, showcase docs, doc site pages (split from doc 131) |
| orchestrator | `docs/qa/orchestrator/131c-workflow-template-library-regression.md` | 1 | FR-077: progressive complexity regression check (split from doc 131) |
| orchestrator | `docs/qa/orchestrator/132-filesystem-trigger.md` | 5 | FR-085: filesystem trigger source, validation, path and event checks |
| orchestrator | `docs/qa/orchestrator/132b-filesystem-trigger-advanced.md` | 5 | FR-085: serde roundtrip, watcher lifecycle, trigger engine (split from doc 132) |
| orchestrator | `docs/qa/orchestrator/132c-filesystem-trigger-regression.md` | 2 | FR-085: path safety guards and event payload format (split from doc 132) |
| orchestrator | `docs/qa/orchestrator/133-daemon-config-hot-reload.md` | 5 | FR-086: ArcSwap atomic config snapshot, persist+notify reload path, trigger/webhook runtime reads |
| orchestrator | `docs/qa/orchestrator/134-qa-doctor-observability.md` | - | FR-088: `orchestrator qa doctor` CLI for task execution metrics observability |
| orchestrator | `docs/qa/orchestrator/135-secretstore-key-emergency-recovery.md` | 8 | FR-089: SecretStore key emergency bootstrap recovery |
| orchestrator | `docs/qa/orchestrator/136-crd-plugin-system.md` | 5 | CRD plugin system: definitions, validation, interceptor/transformer/cron execution with PluginExecutionContext |
| orchestrator | `docs/qa/orchestrator/137-plugin-policy-governance.md` | 5 | FR-087-SEC: plugin policy governance — allowlist, deny, audit, hooks, execution_profile, env_deny_prefixes, RBAC |
| orchestrator | `docs/qa/orchestrator/100-configurable-spill-path.md` | 5 | FR-092 workspace-configured pipeline artifact spill path |
| orchestrator | `docs/qa/orchestrator/100b-configurable-spill-path-regression.md` | 1 | FR-092 workspace regression gate |
| orchestrator | `docs/qa/orchestrator/101-sandbox-readable-paths.md` | 5 | FR-093 sandbox read-only access outside the workspace |
| orchestrator | `docs/qa/orchestrator/101b-sandbox-readable-paths-regression.md` | 4 | FR-093 environment, validation, test, and clippy gates |
| orchestrator | `docs/qa/orchestrator/138-lightweight-step-run.md` | 5 | Lightweight step filtering and initial-variable persistence |
| orchestrator | `docs/qa/orchestrator/138b-lightweight-step-run-direct-assembly.md` | 5 | Lightweight synchronous run and direct RunStep assembly |
| orchestrator | `docs/qa/orchestrator/139-linux-sandbox-filesystem-isolation.md` | 5 | FR-091: Linux sandbox filesystem isolation — mount namespaces, workspace_readonly, workspace_rw_scoped |
| orchestrator | `docs/qa/orchestrator/139b-linux-sandbox-filesystem-isolation-integration.md` | 2 | FR-091 profile propagation and namespace composition |
| orchestrator | `docs/qa/orchestrator/140-plugin-sandbox-isolation.md` | 5 | Plugin sandbox isolation — TOCTOU defense, profile precedence, env sanitization, audit enhancement |
| orchestrator | `docs/qa/orchestrator/141-step-scope-roundtrip-leak.md` | 5 | FR-094 step scope preservation and task-item explosion regression |
| orchestrator | `docs/qa/orchestrator/141b-step-scope-directory-scan-diagnostics.md` | 1 | FR-094 QaDirectoryScan diagnostic events |
| orchestrator | `docs/qa/orchestrator/142-process-timeline-read-model.md` | 5 | FR-095 semantic process timeline, evidence, pagination, live GUI reconciliation |
| orchestrator | `docs/qa/orchestrator/143-attention-inbox.md` | 5 | FR-096 persistent attention projection, concurrency, RBAC, actions, and default GUI |
| orchestrator | `docs/qa/orchestrator/144-handoff-and-safe-resume.md` | 5 | FR-097 immutable handoffs, logical boundaries, stale-safe execution, provider opacity, and GUI preview |
| orchestrator | `docs/qa/orchestrator/145-agent-session-control-plane.md` | 5 | FR-098 original session control specification; execution status superseded by QA-149 |
| orchestrator | `docs/qa/orchestrator/146-source-events-and-slack-binding.md` | 5 | FR-099 provider-neutral source ingestion, Slack verification, deterministic process binding, audited actions, and Sources UI |
| orchestrator | `docs/qa/orchestrator/147-process-console-ui.md` | 5 | FR-100 Attention-first console navigation, failed-process evidence flow, role gates, session re-entry, and responsive fallbacks |
| orchestrator | `docs/qa/orchestrator/148-control-plane-action-audit-envelope.md` | 5 | FR-101 canonical mutation envelope, request-ID joins, retry conflicts, denial evidence, redaction, and rollout enforcement |
| orchestrator | `docs/qa/orchestrator/149-agent-session-control-plane-hardening.md` | 5 | FR-102 executable closure for migration, bounded readers, fenced atomic input, PID identity, restart, RBAC, and GUI re-entry |
| orchestrator | `docs/qa/orchestrator/150-process-console-recovery-notifications-e2e.md` | 5 | FR-103 reviewed recovery routing, actor-aware Attention notifications, real Tauri/gRPC vertical proof, audit privacy, and accessibility |
| orchestrator | `docs/qa/orchestrator/151-process-console-operational-metrics.md` | 5 | FR-104 exact product metrics, bounded gRPC/CLI, local Operations UI, lifecycle controls, privacy, and release performance |
| orchestrator | `docs/qa/orchestrator/152-session-runtime-policy-authority.md` | 4 | FR-105 deterministic `_system` policy selection, immediate Session read/control gates, restart persistence, and safety regression |
| orchestrator | `docs/qa/orchestrator/153-process-console-release-acceptance.md` | 5 | FR-106 clean current-HEAD aggregate gate, migration identity, populated upgrade, integrated recovery, performance, and rollback contract |
| orchestrator | `docs/qa/orchestrator/154-process-console-functional-ui-regression.md` | 5 | Post-release Process Console unit/UI coverage expansion across navigation, mutations, timeline, handoff, Sources, Sessions, accessibility, and coverage reporting |
| orchestrator | `docs/qa/orchestrator/155-slack-reaction-source-event-contract.md` | 5 | FR-107 typed reaction ingestion, Slack validation, deduplication, non-mutating routing, bounded reads, and Sources UI evidence |
| orchestrator | `docs/qa/orchestrator/156-source-task-template-skill-invocation.md` | 5 | FR-108 native template lifecycle, safe deterministic preview, zero mutation, restart stability, and governed deletion |
| orchestrator | `docs/qa/orchestrator/157-source-task-binding-badge-matching.md` | 5 | FR-109 native binding lifecycle, exact trusted matching, conflict rollback, audited hot mutation, and reference-safe deletion |
| orchestrator | `docs/qa/orchestrator/158-slack-permalink-canonical-task-routing.md` | 5 | FR-110 signed badge to validated permalink and one canonical task, durable provenance, replay/restart convergence, RBAC, and UI deep links |
| orchestrator | `docs/qa/orchestrator/159-source-automation-reliability-operations.md` | 5 | FR-111 bounded route leases/retries, Attention recovery, safe operations, suspension, metrics, retention, and compatibility |
| orchestrator | `docs/qa/orchestrator/160-process-console-source-automation-ui.md` | 5 | FR-112 Process Console template/badge management, daemon preview/simulation, route diagnosis/replay, CAS/RBAC/privacy, accessibility, and real Tauri bridge |
| orchestrator | `docs/qa/orchestrator/161-slack-reaction-skill-automation-release.md` | 5 | FR-113 clean-tree aggregate, signed two-badge Skill/workflow routing, concurrency/restart/replay, populated migration, compatible rollback, Tauri/UI, and release documentation |
| orchestrator | `docs/qa/orchestrator/162-managed-slack-connection-shared-oauth.md` | 5 | FR-114 shared official Slack App OAuth, SourceConnection lifecycle, tenant-isolated delivery, target-side transfer, CLI/Tauri/UI/security, and live sandbox certification |
| orchestrator | `docs/qa/orchestrator/163-dedicated-slack-app-auto-provisioning.md` | 5 | Fixed-manifest private App creation, local Configuration Token custody, cross-App isolation, reviewed lifecycle/migration, same-message two-badge routing, offline cursor recovery, UI, and completed live certification |
| orchestrator | `docs/qa/orchestrator/164-agent-driver-abstraction.md` | 5 | Provider-neutral driver resources, apply-time capability gates, direct event folding, sandbox/cancel invariants, session privacy, MCP isolation, and shell compatibility |
| orchestrator | `docs/qa/orchestrator/165-non-code-workspace-and-global-file-sharing.md` | 5 | Task Workspace compatibility gates, canonical file-sharing ceiling, isolated HOME/global Skills, implicit-item convergence, Console semantics, and Slack pilot |
| orchestrator | `docs/qa/orchestrator/166-codex-session-resume-conformance.md` | 5 | Codex CLI 0.144.5 resume grammar, recorded JSONL replay, real same-thread context continuity, credential/session privacy, and repository regression |
| orchestrator | `docs/qa/orchestrator/167-global-skill-directory-provenance.md` | 4 | FR-117-A daemon UID, permission-bit, task-writable overlap, trusted-path, platform, and non-code vertical regression |
| orchestrator | `docs/qa/orchestrator/168-coordination-collapse-mcp-tools.md` | 5 | FR-118 authenticated daemon tool host, real tools, stdio forwarding, parity/events, 100% coordination-line collapse, and residual channels |
| orchestrator | `docs/qa/orchestrator/169-expert-resources-governed-editing.md` | 5 | FR-119 typed catalogs, canonical Describe, accessible role-aware editing, optimistic conflicts, Action Audit, privacy, and real Tauri/daemon evidence |
| orchestrator | `docs/qa/orchestrator/170-handoff-dialog-focus-lifecycle.md` | 5 | FR-120 manual and Attention review entry, modal focus containment and restoration, async invalidation, busy/failure recovery, visual accessibility, and Chromium regression |
| orchestrator | `docs/qa/orchestrator/171-attention-mutation-error-reconciliation.md` | 5 | FR-121 shared mutation failure lifecycle, authoritative reconciliation, safe error copy, focus recovery, telemetry privacy, and two-client version competition |
| orchestrator | `docs/qa/orchestrator/172-boundary-layer-coverage-governance.md` | 5 | FR-122 machine-readable component/module coverage, approved baseline enforcement, explicit branch support, daemon risk matrices, CLI/Tauri tonic adapters, and evidence traceability |
| orchestrator | `docs/qa/orchestrator/173-slack-sandbox-continuous-certification.md` | 5 | FR-123 unified shared/dedicated live modes, checkpoint resume, minimal secret custody, recorded provider CI, expiring safe evidence, and reviewed cleanup |
| orchestrator | `docs/qa/orchestrator/174-coordination-strangler-completion.md` | 5 | FR-124 exact production inventory, seven independent legacy/tool pairs, explicit tool/session boundaries, two-cycle self-bootstrap survival, and retirement ratchet |
| orchestrator | `docs/qa/orchestrator/175-legacy-coordination-decommission.md` | 5 | FR-125 exact consumer inventory, capture/JSONPath production removal, narrow residual state, explicit blockers, and post-retirement tool/repository closure |
| orchestrator | `docs/qa/orchestrator/176-agent-driver-execution-migration.md` | 5 | FR-126 per-Agent fingerprints, production shell/fake-Claude parity, layered compatibility, EN/ZH guide semantics, rollback, and mandatory repository closure |
| orchestrator | `docs/qa/orchestrator/177-qa-gate-enforcement-surface.md` | 5 | FR-127 enforcement classification of every QA gate, bidirectional surface compare, workflow wiring truth, provider isolation invariant, induced gate failure, and stale enforcement claims |
| orchestrator | `docs/qa/orchestrator/178-governance-ledger-regeneration.md` | 5 | FR-128 ledger regeneration sharing the compared expression, CI write refusal and byte-quiet round trip, per-agent mismatch diagnosis with same-commit detection, exact source ratchets, and gate wiring |
| orchestrator | `docs/qa/orchestrator/179-skill-mirror-integrity.md` | 5 | FR-129 skill mirror coverage across every declared root, the read that proves each mirrored `SKILL.md` opens as a regular file, isolated corruption fixtures including a shape-perfect read-only failure, gate wiring truth, and removal of the unproduced third copy under a single-source rule that stops it returning |
| orchestrator | `docs/qa/orchestrator/180-core-boundary-freeze.md` | 5 | FR-130 core crate boundary frozen in both directions with a per-file `rusqlite` inventory, the reviewed migration schema baseline with a doctored-snapshot negative fixture, idempotency and resume-to-identical-schema at all 37 interruption points, and one Rust source scanner shared by both governance ledgers |
| orchestrator | `docs/qa/orchestrator/181-docs-publishing-integrity.md` | 5 | FR-131 showcase sources recovered and single-sourced, publish set proven by running the generator and diffing in both directions, navigation reachability gated in both directions, 36 hand-maintained site pages untracked with per-object evidence, and every relative markdown link resolved without reporting the eight that were never broken |
| orchestrator | `docs/qa/orchestrator/182-doc-lifecycle-governance.md` | 5 | FR-132 lifecycle frontmatter on all 378 design and QA documents, the three genuinely superseded documents pointed at their successor by human inspection, twelve gate cases each isolated by a targeted mutation, exact-equality reverse index with a CI write refusal, and the existing documentation gates proven unaffected |
| orchestrator | `docs/qa/orchestrator/183-gate-surface-execution-truth.md` | 5 | FR-134 four reproduced defects turned into resident fixtures that take the surface gate from 5/0 to three failures, wiring decided from parsed workflow steps and pinning per agent, coverage discovered for markdown/scripts/mirror roots/CI jobs instead of enumerated, ripgrep and workspace scope repaired with the checks landed first, environment equivalence proving a gate that was green locally and dead in CI, and a real Rust lexer that leaves all four ratchets at 53/30/9/0 where a per-line fix moves one to 60 |
| orchestrator | `docs/qa/orchestrator/184-bash32-compatibility.md` | 5 | FR-135 the empty-array defect reproduced under a real bash 3.2 and gone after the fix, all 95 tracked shell files clean on a git-derived set, seven hazard classes executed rather than only matched with the skips reported loudly on bash 4+, the coverage shell main path asserted by exact argv behind stubs, upload-step diagnostic fidelity read from the parsed workflow, and the job observed recovering on a real macOS runner |
| orchestrator | `docs/qa/orchestrator/185-persistence-dependency-chokepoint.md` | 5 | FR-136/FR-139 the chokepoint decision scoped to the orchestrator database, every driver reference outside core classified with the one surviving coverage assertion proven able to fail, a rule proven to discriminate by a crate it must NOT reject, SQL caught in a file naming no driver at all, `PRAGMA` counted and log prose proven not counted in the same file, a `forbidden` crate's build script and a member outside `crates/` both scanned, and no production code introduced |
| orchestrator | `docs/qa/orchestrator/186-persistence-crate-extraction.md` | 5 | FR-130 Phase A and C: core compiled with the `orchestrator-persistence` dependency commented out must fail, the migration resume sweep's own extent asserted against the applied rows and a `step_by`-shortened copy proven to fail, a write/read round trip crossing every moved module with each write read back through a different one, the same calls against an unmigrated database required to error rather than return a plausible nothing, cargo's resolved tree proven to hold no path from the layer back to core, a `?` on a `rusqlite::Result` proven no longer to convert into `OrchestratorError`, and the gate refusing to run on a dirty worktree because three fixtures are built from `git archive HEAD` |
| orchestrator | `docs/qa/orchestrator/187-governance-aggregation-completeness.md` | 4 | FR-137 a gate left out of the aggregate reproduced and rejected while the classification, wiring and dependency checks all stay green, the no-id and dangling directions the FR under-specified each with a disjoint fixture, the real `Governance result` script executed against synthetic outcomes to prove referenced is load-bearing and that an empty outcome fails closed rather than silently, and coverage derived from globbed workflows with no job name written into the rule |
| orchestrator | `docs/qa/orchestrator/188-trigger-history-limit-cascade.md` | 5 | FR-142 a task that ran deleted with its items, command runs, events and log paths while the original bare `DELETE` stays reproducible as raw SQL; a task held by `resume_plans` skipped with every seeded row unchanged and a cascading `task_graph_runs` row proven not to be reported as a blocker; a failure that is not a child row surfacing as an error rather than a skip; the sweep's summary and skip cause captured through a subscriber at the daemon's own `info` fallback, so a severity regression fails here instead of in production; and the frozen schema unchanged with the persistence ledger moving by exactly one statement |
| orchestrator | `docs/qa/orchestrator/189-persistence-connection-capability-boundary.md` | 5 | FR-141 the layer's unconditional public API proven to hand out no driver connection, the three-fact gate that holds it, the compiler-level guarantee behind the test-only door, and DD-147's frozen residual paid off |
| orchestrator | `docs/qa/orchestrator/190-bash32-scanner-lexical-state.md` | 5 | FR-138 a here-document lookalike inside a cross-line quoted region with the hazard on the last line so partial recovery cannot pass, the same shape inside `$( ... ' ... ' )` kept as a separate fixture because it survives the fix the FR asked for, an apostrophe inside double quotes whose mutation target is the replacement rather than the original defect and is named as such, an unclosed here-document reported at its opener and naming the terminator, negation caught while mentions stay uncaught, and coverage asserted by per-file line accounting rather than by a green gate — the state the defect passed in |
| orchestrator | `docs/qa/orchestrator/191-governance-execution-cost.md` | 5 | FR-140 a gate whose step has no cost record fails and names both, with the mutation an added step rather than a deleted record; a ceiling with no written reason fails; a measurement that is not an ancestor of HEAD fails; the budget proved to evaluate real recorded seconds by lowering the ceiling below the total rather than raising the durations, because a coverage-only ledger passes on a pipeline of any length; `--write` refused unattended asserted on the diagnostic and not only the exit code; and the lexer rewrite pinned by 15 known-answer masking cases written as counts rather than captured from the implementation |
| orchestrator | `docs/qa/orchestrator/192-jq-status-observed.md` | 5 | FR-144 the reader fails naming the file and quoting jq rather than on exit code alone; `require-rows` fails on empty and `allow-empty` still passes on empty, so the first is not satisfied by a reader that rejects everything; a read failing inside a process substitution still leaves a record the parent finds — the case that caught the reader misreading its own status where `set -e` was live; the **real** gate-surface gate asserted to reject a manifest jq cannot parse, since this defect's signature is that the gate and its fixtures disagreed; the scanner proven to parse and not grep by requiring the same line in a comment and in a here-document body to be non-findings; a reader captured without testing its status rejected while a correctly-written multi-line one whose `||` sits past a backslash continuation is not, since a rule that flags correct code is switched off before it catches anything; **all five rules the scanner defines proven by a case**, two of which had none until the rules were listed against the cases; and coverage shown to follow the manifest by registering a gate and watching the scanned set grow |
| orchestrator | `docs/qa/orchestrator/193-fixture-target-drift.md` | 5 | FR-143 a stale premise costs one failed assertion and the run still reaches its summary line, rather than aborting where a truncated run reads exactly like a complete one; **the recorded incident replayed** — an in-place substitution matching nothing fails naming the file, and the case's own accusation against the gate never prints, which is the property that makes this worse than a gap since the fixture reported a defect that was not there; a target that is a directory rejected, the state an emptied ledger read produces; an empty derivation rejected because zero bytes and a correct one are the same exit code; **all five rules the scanner defines proven by a case and each paired with its opposite**, including two that have no violation in the tree and are therefore knowable only from their fixtures; the scanner shown to parse rather than grep by requiring three of four occurrences of the forbidden word — a shell comment, a here-document body, a Ruby comment — to be non-findings, and by not reading `(cd "$DIR" && ruby "$GATE")` as a mutation; an empty scanned set rejected, since zero gates scanned and twenty-eight scanned clean were otherwise the same exit code; and coverage shown to follow the manifest |
| orchestrator | `docs/qa/orchestrator/194-dependency-policy-gate.md` | 5 | FR-133 the committed policy passing three ways, then **the ratchet shown to take two observers rather than one** — a skip naming a version the graph no longer has fails `cargo deny`, while a skip naming a version still present but no longer duplicated passes it cleanly and is caught only by the gate's own `skip-is-live`, a division found because the first version of the case failed; deleting one of the 70 acceptances shown to fail by name, so the licence-to-fail on a new duplicate is exercised without waiting for one; **the policy shown to still bind** — a run line with the ratchet flag dropped, with `advisories` or `all` added, with `continue-on-error` set, or commented out entirely, each a finding, where a grep for `cargo deny` is satisfied by the commented-out line; every severity weakened to `warn` a finding; a `skip-tree` a finding while the same words inside a reason string are not; every acceptance required to carry a reason, and every licence exception a comment, since cargo-deny rejects a `reason` key there; and a `security.yml` with no jobs or a lock with no packages required to **fail rather than pass vacuously** |
| orchestrator | `docs/qa/orchestrator/195-pipefail-short-circuit.md` | 5 | FR-145 a reader that leaves on the first match kills the producer, and under `set -o pipefail` the producer's EPIPE becomes the pipeline's answer — **the direction is set by which branch the match feeds, not by the defect**: where the match feeds the passing branch it is a mystery red people re-run until green, and where it feeds the failing branch a real violation reports as clean, measured 2/200, which is how three leak assertions over `sqlite3 … .dump` and two `cargo test` probes were written; the mechanism asserted **deterministically** by a producer that sleeps after emitting the match, 10/10 piped and 0/10 through a here-string, because the buffer race measures 8-13/400 on one machine and 0/200 on a 1 MB producer and is not something to gate on; five mutations each asserted on a **derived** line number rather than a written one, and six silent shapes — a comment, a here-document body, a file without `pipefail`, a quoted alternation, a pattern after `--`, and a counting `grep -c` that this gate itself false-positived on three times; the governed set grown by adding one tracked file with no edit to the scanner; and every rewritten gate's pass count held before and after. **No exemption for a file that does not enable `pipefail`** — the gate's first version granted one, and the closure self-check found `run-cli-probes.sh` sourcing every scenario into a shell that sets it, so two files the FR called immune were live sites; case 9b demonstrates it by execution |
| orchestrator | `docs/qa/orchestrator/196-fixture-bundle-validity.md` | 5 | FR-148 nothing compared what a fixture bundle declares against what the product still accepts, so `test-coordination-collapse.sh` sat broken for four days over a `behavior.captures` DD-137 had removed — the fixture said it, the validator rejected it, and no artifact put the two side by side; the corpus is derived from `git ls-files` and every rejection must appear in `config/governance/fixture-bundle-validity.json` with a reason and **the diagnostic it fails by**, because capability validation runs before the retirement checks and an exit code cannot tell `no agent supports capability` apart from the retirement a fixture exists to demonstrate; **31 of 93 are rejected, not the 4 the FR believed**, 19 of them rot DD-137 left behind and one of those breaks a second gate nobody had noticed; scenario 3 is the case an exit-code check cannot produce — rejected, declared, and rejected for the wrong reason; the injection fixture derives its target rather than naming it, and the ratchet on the rot is exact in both directions so retiring one has to move the number |
| orchestrator | `docs/qa/orchestrator/agent-drain-enabled.md` | - | FR-017: agent drain and enabled switch, selection filtering, in-flight counting |
| orchestrator | `docs/qa/orchestrator/guide-alignment.md` | - | FR-018 compile-driven EN/ZH guide review plus FR-126 deterministic Agent driver semantics gate |
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

## FR-095 Through FR-118 Executable Evidence Index

This index records the strongest executable evidence layer for each closed FR.
`Closed` means its acceptance evidence is complete; it does not mean every
related production file has high line coverage. `Live` is never part of normal
PR CI.

| FR | Design / QA authority | Unit | Integration | Shell QA | Playwright | Controlled live |
|---|---|---:|---:|---|---:|---|
| FR-095 | DD-105 / QA-142 | Yes | Yes | `test-process-timeline.sh` | No direct journey | No |
| FR-096 | DD-106 / QA-143 | Yes | Yes | `test-attention-inbox.sh` | Yes | No |
| FR-097 | DD-107 / QA-144 | Yes | Yes | `test-handoff-safe-resume.sh` | Yes | No |
| FR-098 | DD-108 / QA-145, superseded by QA-149 | Yes | Yes | `test-agent-session-control-plane.sh` | Yes | No |
| FR-099 | DD-109 / QA-146 | Yes | Yes | `test-source-events-slack.sh` | Yes | No |
| FR-100 | DD-110 / QA-147 | Vitest | Tauri mock | `test-process-console-ui.sh` | Yes | No |
| FR-101 | DD-111 / QA-148 | Yes | Yes | `test-control-plane-action-audit.sh` | No direct journey | No |
| FR-102 | DD-112 / QA-149 | Yes | Yes | `test-agent-session-control-plane.sh` | Yes | No |
| FR-103 | DD-113 / QA-150 | Yes | Real Tauri/daemon | `test-process-console-vertical-flow.sh` | Yes | Deterministic local daemon only |
| FR-104 | DD-114 / QA-151 | Yes | Yes | `test-process-console-metrics.sh` | Yes | No |
| FR-105 | DD-115 / QA-152 | Yes | Yes | `test-agent-session-control-plane.sh` | Session regression | No |
| FR-106 | DD-116 / QA-153 | Yes | Aggregate | `test-process-console-release.sh` | Yes | No |
| FR-107 | DD-118 / QA-155 | Yes | Signed local webhook | `test-slack-reaction-source.sh` | Yes | No |
| FR-108 | DD-119 / QA-156 | Yes | Isolated daemon | `test-source-task-template.sh` | No direct journey | No |
| FR-109 | DD-120 / QA-157 | Yes | Isolated daemon | `test-source-task-binding.sh` | No direct journey | No |
| FR-110 | DD-121 / QA-158 | Yes | Signed local route | `test-slack-reaction-task-routing.sh` | Deep-link regression | No |
| FR-111 | DD-122 / QA-159 | Yes | Restart/retry fixture | `test-source-automation-operations.sh` | Operations regression | No |
| FR-112 | DD-123 / QA-160 | Vitest | Real Tauri/daemon | `test-source-automation-ui.sh` | Yes | No |
| FR-113 | DD-124 / QA-161 | Yes | Release vertical | `test-slack-skill-automation-release.sh` | Yes | No public provider |
| FR-114 | DD-125 / QA-162 | Yes | Gateway/daemon | `test-slack-managed-shared-oauth.sh` | Yes | `certify-slack-managed-live.sh` |
| FR-115 | DD-126 / QA-163 | Yes | Two-App lifecycle | `test-slack-dedicated-app-provisioning.sh` | Yes | Dedicated sandbox addendum |
| FR-116 | DD-127 / QA-164; DD-129 / QA-166 extension | Yes | Driver fixture | `test-agent-driver-abstraction.sh` | No | `certify-codex-session-resume.sh` |
| FR-117 | DD-128 / QA-165, QA-167 extension | Yes | Local Slack pilot | `test-non-code-workspace.sh` | Yes | No public provider |
| FR-118 | DD-130 / QA-168 | Yes | Authenticated tool host | `test-coordination-collapse.sh` | No | No |
| FR-124 | DD-136 / QA-174 | Yes | Seven legacy/tool pairs | `test-coordination-strangler.sh` | No | No |
| FR-125 | DD-137 / QA-175 | Yes | Seven post-retirement tool workflows | `test-legacy-coordination-decommission.sh` | No | No |
| FR-126 | DD-138 / QA-176 | Yes | Three production shell pairs + recorded/fake Claude parity + guide negative fixture | `test-agent-driver-execution-migration.sh` | No | No |

Evidence rows are maintained when an owning FR closes. A later hardening FR may
supersede an earlier QA document without erasing the historical design link.
