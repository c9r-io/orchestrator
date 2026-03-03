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
| orchestrator | `docs/design_doc/orchestrator/10-structured-output-worker-scheduler.md` | `docs/qa/orchestrator/20-structured-output-worker-scheduler.md` | Structured output scheduler mainline + detach worker model |
| orchestrator | `docs/design_doc/orchestrator/11-performance-io-queue-optimizations.md` | `docs/qa/orchestrator/22-performance-io-queue-optimizations.md` | Single-write command runs, bounded IO reads, true tail, and atomic multi-worker claim |
| self-bootstrap | `docs/design_doc/self-bootstrap/01-survival-mechanism.md` | `docs/qa/self-bootstrap/01-survival-binary-checkpoint-self-test.md`, `docs/qa/self-bootstrap/02-survival-enforcement-watchdog.md` | 4-layer survival mechanism: binary checkpoint, self-test gate, self-referential enforcement, watchdog |
| orchestrator | `docs/design_doc/orchestrator/12-step-scope-segment-execution.md` | `docs/qa/orchestrator/29-step-scope-segment-execution.md` | StepScope enum + segment-based execution: task-scoped once, item-scoped fan out |
| orchestrator | `docs/design_doc/orchestrator/13-unified-step-execution-model.md` | `docs/qa/orchestrator/30-unified-step-execution-model.md` | Unified step execution: WorkflowStepType deletion, StepBehavior data types, StepExecutionAccumulator |
| orchestrator | `docs/design_doc/orchestrator/14-check-command.md` | `docs/qa/orchestrator/31-check-command.md` | New check CLI command: workspace/agent/config/all subcommands with output formats |
| self-bootstrap | `docs/design_doc/self-bootstrap/02-binary-snapshot-verification.md` | `docs/qa/self-bootstrap/03-survival-smoke-binary-snapshot-verification.md` | Binary verification function with MD5 checksum comparison |
| orchestrator | `docs/design_doc/orchestrator/15-task-trace.md` | `docs/qa/orchestrator/32-task-trace.md` | Post-mortem diagnostics: execution timeline reconstruction and 9-rule anomaly detection |
| orchestrator | `docs/design_doc/orchestrator/16-structured-logging.md` | `docs/qa/orchestrator/36-structured-logging.md` | Structured logging bootstrap, CLI log overrides, stderr/stdout separation, and rolling system log files |
| orchestrator | `docs/design_doc/orchestrator/17-envstore-secretstore-agent-env.md` | `docs/qa/orchestrator/37-envstore-secretstore-resources.md`, `docs/qa/orchestrator/38-agent-env-resolution.md` | EnvStore/SecretStore resources and agent env configuration with runtime resolution and secret redaction |
