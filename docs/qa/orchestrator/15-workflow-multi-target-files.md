# Orchestrator - Workflow Multi-Target Files

**Module**: orchestrator
**Scope**: Validate one task can fan out to multiple target files
**Scenarios**: 1
**Priority**: Medium

---

## Background

This document is split from `05-workflow-execution.md` to keep each QA document within 5 scenarios.

Entry point: `./scripts/orchestrator.sh task <command>`

Project setup (run once):

```bash
./scripts/orchestrator.sh db reset --force --include-config
./scripts/orchestrator.sh init

QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/echo-workflow.yaml
```

> Note: DB reset is required to clear any residual workflows from prior test runs
> that may cause config validation errors (e.g., stale workflows requiring
> missing agent templates).

---

## Scenario 1: Multiple Target Files

### Preconditions

- DB reset and project setup completed (see Background).
- Workspace and workflow are available.
- Multiple target files exist in repository.
- Project is prepared: `./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force`

### Steps

1. Create task with explicit multi-target inputs:
   ```bash
   ./scripts/orchestrator.sh task create \
     --name "multi-file-test" \
     --goal "Test multiple files" \
     --project "${QA_PROJECT}" \
     --target-file docs/qa/orchestrator/01-cli-agent-orchestration.md \
     --target-file docs/qa/orchestrator/02-cli-task-lifecycle.md \
     --no-start
   ```

2. Start task:
   ```bash
   ./scripts/orchestrator.sh task start {task_id}
   ```

3. Check task details:
   ```bash
   ./scripts/orchestrator.sh task info {task_id}
   ```

### Expected

- A separate task item is created for each `--target-file` input.
- Progress reflects multi-item execution (`X/Y`).
- Task status is consistent with combined item results.

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| `Error: active config is not runnable ... loop.guard enabled but no agent has loop_guard template` | Residual workflow from a prior test run exists in the DB | Reset DB: `./scripts/orchestrator.sh db reset --force --include-config && ./scripts/orchestrator.sh init` |
| `Error: load task details failed ... task not found` | Task failed during execution and info lookup uses wrong project scope | Ensure `--project "${QA_PROJECT}"` is passed to `task info` |

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Multiple Target Files | ☐ | | | |
