# Orchestrator - Workflow Multi-Target Files

**Module**: orchestrator
**Scope**: Validate one task can fan out to multiple target files
**Scenarios**: 1
**Priority**: Medium

---

## Background

This document is split from `05-workflow-execution.md` to keep each QA document within 5 scenarios.

Entry point: `./scripts/run-cli.sh task <command>`

Project setup (run once):

```bash
./scripts/run-cli.sh init --force

QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
./scripts/run-cli.sh apply -f fixtures/manifests/bundles/echo-workflow.yaml
./scripts/run-cli.sh qa project reset "${QA_PROJECT}" --keep-config --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
./scripts/run-cli.sh qa project create "${QA_PROJECT}" --force
```

> Note: Fixture application is additive. Re-apply the expected fixture and
> recreate the isolated project scaffold instead of clearing global config.

---

## Scenario 1: Multiple Target Files

### Preconditions

- DB reset and project setup completed (see Background).
- Workspace and workflow are available.
- Multiple target files exist in repository.
- Project scaffold is freshly recreated: `qa project reset` + `rm -rf "workspace/${QA_PROJECT}"` + `qa project create --force`

### Steps

1. Create task with explicit multi-target inputs:
   ```bash
   ./scripts/run-cli.sh task create \
     --name "multi-file-test" \
     --goal "Test multiple files" \
     --project "${QA_PROJECT}" \
     --target-file docs/qa/orchestrator/01-cli-agent-orchestration.md \
     --target-file docs/qa/orchestrator/02-cli-task-lifecycle.md \
     --no-start
   ```

2. Start task:
   ```bash
   ./scripts/run-cli.sh task start {task_id}
   ```

3. Check task details:
   ```bash
   ./scripts/run-cli.sh task info {task_id}
   ```

### Expected

- A separate task item is created for each `--target-file` input.
- Progress reflects multi-item execution (`X/Y`).
- Task status is consistent with combined item results.

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| `Error: active config is not runnable ... loop.guard enabled but no agent has loop_guard template` | Residual workflow from a prior test run is still present because fixture application is additive | Re-apply `fixtures/manifests/bundles/echo-workflow.yaml`, then recreate the isolated QA project scaffold (`qa project reset` + `rm -rf workspace/<project>` + `qa project create --force`) |
| `Error: load task details failed ... task not found` | Task failed during execution and info lookup uses wrong project scope | Ensure `--project "${QA_PROJECT}"` is passed to `task info` |

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Multiple Target Files | ☐ | | | |
