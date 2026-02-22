# Orchestrator - Workflow Multi-Target Files

**Module**: orchestrator
**Scope**: Validate one task can fan out to multiple target files
**Scenarios**: 1
**Priority**: Medium

---

## Background

This document is split from `05-workflow-execution.md` to keep each QA document within 5 scenarios.

Entry point: `./scripts/orchestrator.sh task <command>`

---

## Scenario 1: Multiple Target Files

### Preconditions

- Workspace and workflow are available.
- Multiple target files exist in repository.

### Steps

1. Create task with explicit multi-target inputs:
   ```bash
   ./scripts/orchestrator.sh task create \
     --name "multi-file-test" \
     --goal "Test multiple files" \
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

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Multiple Target Files | ☐ | | | |
