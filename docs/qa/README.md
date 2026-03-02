# QA Docs

This directory contains reproducible, verifiable QA test documents.

## Source of Truth

- Runtime state source: SQLite (`data/agent_orchestrator.db`)
- YAML role: import/export/edit artifact (`apply`, `manifest export`, `edit export`)
- QA docs must not assume a mandatory `default.yaml` file is auto-generated.

## QA Contract

- Canonical CLI contract: `docs/qa/orchestrator/00-command-contract.md`
- Preferred entry point: `./scripts/orchestrator.sh <command>`
- Repository root is the default execution directory for all QA steps.

## Document Rules (Strict)

1. Keep each document to at most **5 scenarios**.
2. Every scenario must include Preconditions, Steps, and Expected Result.
3. Commands must align with actual CLI surface from `core/src/cli.rs`.
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
- Recreate per-project scaffolding via `orchestrator qa project reset <project> --keep-config --force`, remove `workspace/<project>`, then run `orchestrator qa project create <project> --force`.

Project isolation requirements for QA execution:
- QA setup must treat `project` as the primary isolation boundary. Do not rely on global DB resets to obtain a clean environment.
- Before each isolated QA run, recreate the target project with the current CLI: run `orchestrator qa project reset <project> --keep-config --force`, remove `workspace/<project>`, then run `orchestrator qa project create <project> --force`.
- All QA task creation, task execution, and follow-up inspection must explicitly bind to the intended project. Do not rely on ambient defaults when a project-scoped command is available.
- Fixture manifests used by QA must be applied only to support that QA run's project/workflow setup. Do not use QA fixtures to overwrite or replace the active orchestrator control-plane state for unrelated tasks.
- Do not run `orchestrator db reset --include-config`, `orchestrator db reset --force --include-config`, or any equivalent config-destructive reset as part of routine QA scenario execution.
- Do not change `Defaults` to point the whole runtime at a QA-only workflow as part of scenario setup. QA fixtures must not hijack the default workspace/workflow used by unrelated runs such as `self-bootstrap`.

## Lint Guard

Run:

```bash
./scripts/qa-doc-lint.sh
```

This checks:
- banned stale patterns (`cd orchestrator`, `--workspace-id`, `orchestrator agent health`, `orchestrator/config/default.yaml`, `config bootstrap --from`, `--config <file>`)
- scenario count limit (<=5)
- orchestrator QA docs are indexed in this README

## Index

| Module | Doc | Scenarios | Notes |
|--------|-----|-----------|-------|
| orchestrator | `docs/qa/orchestrator/00-command-contract.md` | 3 | Canonical CLI command contract |
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
| orchestrator | `docs/qa/orchestrator/20-structured-output-worker-scheduler.md` | 5 | Structured output validation + detach/worker scheduling mainline |
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
| orchestrator | `docs/qa/orchestrator/32-task-trace.md` | 5 | Task trace: execution timeline reconstruction and anomaly detection |
| orchestrator | `docs/qa/orchestrator/33-fatal-agent-error-detection.md` | 1 | Regression: fatal provider stderr must override outer exit code 0 and mark runs failed |
| orchestrator | `docs/qa/orchestrator/34-config-heal-auditability.md` | 5 | Config self-heal audit log persistence, heal-log CLI, check enhancement |
| orchestrator | `docs/qa/orchestrator/35-legacy-observability-backfill.md` | 5 | Legacy event step_scope backfill, unknown→legacy display, backfill-events CLI |
| orchestrator | `docs/qa/orchestrator/smoke-orchestrator.md` | 1 | Smoke test: core CLI and DB initialization |
| orchestrator | `docs/qa/script/` | 7 | Executable QA scripts |
| self-bootstrap | `docs/qa/self-bootstrap/smoke-self-bootstrap.md` | - | Smoke test: self-bootstrap basics |
| self-bootstrap | `docs/qa/self-bootstrap/01-survival-binary-checkpoint-self-test.md` | 5 | Survival Layer 1-2: binary snapshot/restore and self-test acceptance gate |
| self-bootstrap | `docs/qa/self-bootstrap/02-survival-smoke-binary-snapshot.md` | 5 | Unit tests for snapshot_binary() and restore_binary_snapshot() |
| self-bootstrap | `docs/qa/self-bootstrap/02-survival-enforcement-watchdog.md` | 5 | Survival Layer 3-4: self-referential enforcement and watchdog script |
| self-bootstrap | `docs/qa/self-bootstrap/03-survival-smoke-binary-snapshot-verification.md` | 5 | Binary snapshot verification function and integration test |
| self-bootstrap | `docs/qa/self-bootstrap/04-cycle2-validation-and-runtime-timestamps.md` | 2 | Regression: fixed two-cycle QA validation chain and task/item runtime timestamps |
