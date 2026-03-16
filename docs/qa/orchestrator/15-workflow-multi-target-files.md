---
self_referential_safe: false
---

# Orchestrator - Workflow Multi-Target Files

**Module**: orchestrator
**Scope**: Validate one task can fan out to multiple target files
**Scenarios**: 1
**Priority**: Medium

---

## Background

This document is split from `05-workflow-execution.md` to keep each QA document within 5 scenarios.

Entry point: `orchestrator task <command>`

Project setup (run once):

```bash
orchestrator init --force

QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml
orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project "${QA_PROJECT}"
```

> Note: Fixture application is additive. Re-apply the expected fixture and
> recreate the isolated project scaffold instead of clearing global config.

---

## Scenario 1: Multiple Target Files

### Preconditions

- DB reset and project setup completed (see Background).
- Workspace and workflow are available.
- Multiple target files exist in repository.
- Project scaffold is freshly recreated: `delete project/<project> --force` + `rm -rf "workspace/${QA_PROJECT}"` + `apply -f <fixture> --project`
- **Ticket directory is clean**: remove any stale ticket files before running (`rm -f fixtures/ticket/auto_*.md`). Stale tickets matching a target file's QA path will cause `active_ticket_count > 0`, triggering the `fix_disabled_with_tickets` finalize rule and marking that item as "unresolved" even though QA passed.

### Steps

1. Create task with explicit multi-target inputs:
   ```bash
   orchestrator task create \
     --name "multi-file-test" \
     --goal "Test multiple files" \
     --project "${QA_PROJECT}" \
     --target-file docs/qa/orchestrator/01-cli-agent-orchestration.md \
     --target-file docs/qa/orchestrator/02-cli-task-lifecycle.md \
     --no-start
   ```

2. Start task:
   ```bash
   orchestrator task start {task_id}
   ```

3. Check task details:
   ```bash
   orchestrator task info {task_id}
   ```

### Expected

- A separate task item is created for each `--target-file` input.
- Progress reflects multi-item execution (`X/Y`).
- Task status is consistent with combined item results.

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| `Error: active config is not runnable ... loop.guard enabled but no agent has loop_guard template` | Residual workflow from a prior test run is still present because fixture application is additive | Re-apply `fixtures/manifests/bundles/echo-workflow.yaml`, then recreate the isolated QA project scaffold (`delete project/<project> --force` + `rm -rf workspace/<project>` + `apply -f <fixture> --project`) |
| `Error: load task details failed ... task not found` | Task failed during execution and info lookup uses wrong project scope | Ensure `--project "${QA_PROJECT}"` is passed to `task info` |
| Task shows "failed" with some items "unresolved" despite all runs exit=0 | Stale ticket files in `fixtures/ticket/` match a target file's QA path, causing `active_ticket_count > 0`. The `fix_disabled_with_tickets` finalize rule fires → "unresolved" | Run `rm -f fixtures/ticket/auto_*.md` before the test to clear stale tickets |

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Multiple Target Files | ☑ | 2026-03-16 | chenhan | False positive: stale ticket files in fixtures/ticket/ contaminated finalize. Preconditions updated. |
