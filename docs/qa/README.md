# QA Docs

This directory contains reproducible, verifiable QA test documents.

## Source of Truth

- Runtime state source: SQLite (`data/agent_orchestrator.db`)
- YAML role: import/export/edit artifact (`apply`, `config export`, `edit export`)
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
- Reset per-project data via `orchestrator qa project reset <project> --keep-config --force`.

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
| orchestrator | `docs/qa/orchestrator/04-cli-config-db.md` | 4 | Config set and DB reset |
| orchestrator | `docs/qa/orchestrator/05-workflow-execution.md` | 5 | Workflow execution core scenarios |
| orchestrator | `docs/qa/orchestrator/06-cli-output-formats.md` | 5 | JSON/YAML output validation |
| orchestrator | `docs/qa/orchestrator/07-capability-orchestration.md` | 5 | Capability-driven orchestration core |
| orchestrator | `docs/qa/orchestrator/08-project-namespace.md` | 5 | Project namespace behavior |
| orchestrator | `docs/qa/orchestrator/09-agent-selection-strategy.md` | 5 | Multi-factor selection strategy |
| orchestrator | `docs/qa/orchestrator/10-agent-collaboration.md` | 5 | AgentOutput and MessageBus |
| orchestrator | `docs/qa/orchestrator/10-config-error-handling.md` | 4 | Config error paths |
| orchestrator | `docs/qa/orchestrator/11-config-creation-flow.md` | 4 | Apply-based resource creation |
| orchestrator | `docs/qa/orchestrator/12-config-validation.md` | 4 | Config validate command |
| orchestrator | `docs/qa/orchestrator/13-dynamic-orchestration.md` | 5 | Dynamic orchestration unit-level validation |
| orchestrator | `docs/qa/orchestrator/14-config-validation-enhanced.md` | 5 | Enhanced config validation |
| orchestrator | `docs/qa/orchestrator/15-workflow-multi-target-files.md` | 1 | Split from doc 05 |
| orchestrator | `docs/qa/orchestrator/16-capability-config-view-fields.md` | 1 | Split from doc 07 |
| orchestrator | `docs/qa/orchestrator/17-dynamic-yaml-integration.md` | 1 | Split from doc 13 |
| orchestrator | `docs/qa/orchestrator/18-kubectl-style-extensions.md` | 4 | Get list / create / stdin apply / label selector |
| orchestrator | `docs/qa/orchestrator/19-scheduler-repository-refactor-regression.md` | 5 | P0/P1 scheduler repository refactor regression and observability checks |
| orchestrator | `docs/qa/orchestrator/20-structured-output-worker-scheduler.md` | 5 | Structured output validation + detach/worker scheduling mainline |
| orchestrator | `docs/qa/orchestrator/21-runner-security-observability.md` | 5 | Runner allowlist boundary, redaction, and task execution metrics observability |
| orchestrator | `docs/qa/script/` | 3 | Executable QA scripts |
