# Orchestrator - Workflow Execution (Phases and Lifecycle)

**Module**: orchestrator
**Scope**: Validate that workflow phases execute in the correct order and lifecycle states are accurate
**Scenarios**: 5
**Priority**: High

---

## Background

This document tests workflow execution using a single deterministic mock agent
(`mock_echo`). Every scenario uses `echo-workflow.yaml` (or `fail-workflow.yaml`
for the error path), so results are fully reproducible — no random agent
selection can change the outcome.

### Common Preconditions

Every scenario starts from a clean slate. Two cleanup steps are required:

> **Fixture Workflow IDs**: `echo-workflow.yaml` defines `qa_only`, `qa_fix`, `qa_fix_retest`, `loop_test`. `fail-workflow.yaml` defines `qa_fix` (fail variant). Do NOT use stale names like `basic` or `echo`.

1. **Project isolation**: `apply` is additive — agents from previous test fixtures
   remain in the active config and participate in agent selection, causing unexpected
   failures. Re-apply the intended fixture and recreate the isolated QA project
   scaffold instead of deleting the DB.
2. **Stale tickets**: The echo-workflow fixture uses `ticket_dir: fixtures/ticket`.
   Stale auto-generated tickets from previous runs can cause items to be marked
   "unresolved" even when QA passes.

```bash
# 1. Ensure runtime is initialized
orchestrator init --force

# 2. Clean stale auto-generated tickets
rm -f fixtures/ticket/auto_*.md

# 3. Apply fixture and recreate the isolated project scaffold
orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml
QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project "${QA_PROJECT}"
```

Scenario 4 uses a different fixture (`fail-workflow.yaml`) — see its own
preconditions section.

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| qa_only/loop_test tasks fail with "unresolved" items despite QA exit 0 | Stale ticket files in `fixtures/ticket/` match item QA docs; finalize rules mark items with active tickets as "unresolved" when no fix step is present | Run `rm -f fixtures/ticket/auto_*.md` before testing |
| Task fails with unexpected agent selection (e.g., wrong agent handles qa) | Residual agents from previous test fixtures remain in active config because `apply` is additive. Agent selection uses a top-3 random pick; when agents tie on score (common with no metrics history), the wrong agent can be selected. | Re-apply the intended fixture, then recreate the isolated QA project scaffold (`delete project/<project> --force` + `rm -rf workspace/<project>` + `apply -f <fixture> --project`) using a fresh `QA_PROJECT` value. Agent selection now uses a stable tiebreaker (agent_id alphabetical) to reduce non-determinism. |

---

## Scenario 1: qa_only Workflow

### Preconditions

- Common Preconditions (echo-workflow.yaml applied)

### Steps

1. Create task:
   ```bash
   orchestrator task create \
     --name "qa-only-test" \
     --goal "Test QA only workflow" \
     --project "${QA_PROJECT}" \
     --workflow qa_only \
     --no-start
   ```

2. Start task and wait for completion:
   ```bash
   orchestrator task start {task_id}
   ```

3. Inspect result:
   ```bash
   orchestrator task info {task_id}
   orchestrator task logs {task_id}
   ```

### Expected

- Task status: `completed`
- Failed: 0
- Every log line shows `qa-phase: {rel_path}`

---

## Scenario 2: qa_fix Workflow

### Preconditions

- Common Preconditions (echo-workflow.yaml applied)

### Steps

1. Create task:
   ```bash
   orchestrator task create \
     --name "qa-fix-test" \
     --goal "Test QA and fix workflow" \
     --project "${QA_PROJECT}" \
     --workflow qa_fix \
     --no-start
   ```

2. Start task:
   ```bash
   orchestrator task start {task_id}
   ```

3. Inspect result:
   ```bash
   orchestrator task info {task_id}
   orchestrator task logs {task_id}
   ```

### Expected

- Task status: `completed`
- Failed: 0
- QA phase runs and passes for all items
- Fix phase is skipped (QA produced no failures / tickets)

---

## Scenario 3: qa_fix_retest Workflow

### Preconditions

- Common Preconditions (echo-workflow.yaml applied)

### Steps

1. Create task:
   ```bash
   orchestrator task create \
     --name "qa-fix-retest-test" \
     --goal "Test full workflow" \
     --project "${QA_PROJECT}" \
     --workflow qa_fix_retest \
     --no-start
   ```

2. Start task:
   ```bash
   orchestrator task start {task_id}
   ```

3. Inspect result:
   ```bash
   orchestrator task info {task_id}
   orchestrator task logs {task_id}
   ```

### Expected

- Task status: `completed`
- Failed: 0
- QA executes first and determines whether downstream phases are needed
- When QA creates no tickets, Fix/Retest may be skipped by design
- Logs contain `qa-phase:` entries; Fix/Retest logs appear only when ticket/failure conditions are met

---

## Scenario 4: QA Failure and Ticket Creation

### Preconditions

This scenario uses a **different fixture** with only the `mock_fail` agent:

```bash
orchestrator apply -f fixtures/manifests/bundles/fail-workflow.yaml

QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
orchestrator apply -f fixtures/manifests/bundles/fail-workflow.yaml --project "${QA_PROJECT}"
```

### Steps

1. Create task:
   ```bash
   orchestrator task create \
     --name "ticket-test" \
     --goal "Test ticket creation" \
     --project "${QA_PROJECT}" \
     --workflow qa_fix \
     --no-start
   ```

2. Start task:
   ```bash
   orchestrator task start {task_id}
   ```

3. Check task result and ticket directory:
   ```bash
   orchestrator task info {task_id}
   orchestrator task logs {task_id}
   ls fixtures/ticket/auto_*.md
   ```

### Expected

- QA phase fails for every item (mock_fail exits 1)
- Ticket files are created as `fixtures/ticket/auto_*.md` (the ticket_dir
  of the workspace the task runs against — the global `default` workspace
  has `ticket_dir: fixtures/ticket`)
- Fix phase executes after ticket scan; because the mock fix agent exits 0,
  every item transitions from `qa_failed` → `fixed`
- Task completes with `Failed: 0` (items are "fixed", not "qa_failed")
- Logs show structured JSON outputs (`output_json`/`artifacts_json`);
  failing QA runs are marked by non-success status and ticket artifacts
- **Note**: If the agent becomes unhealthy after repeated QA failures, the task
  may report "No healthy agent found with capability: fix" — this is expected
  when health tracking marks the agent as diseased.

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| No ticket files found in `workspace/${QA_PROJECT}/fixtures/ticket/` | The task uses the global `default` workspace (ticket_dir: `fixtures/ticket`), not the project workspace | Check `fixtures/ticket/auto_*.md` instead |
| `Failed: 0` when expecting failures | Fix phase succeeds (exit 0), transitioning items from `qa_failed` to `fixed` | This is correct behavior; "Failed" counts only items whose final status is a failure state |

---

## Scenario 5: Loop Mode (max_cycles)

### Preconditions

- Common Preconditions (echo-workflow.yaml applied)

### Steps

1. Create task with loop_test workflow:
   ```bash
   orchestrator task create \
     --name "loop-mode-test" \
     --goal "Test infinite loop with max_cycles" \
     --project "${QA_PROJECT}" \
     --workflow loop_test \
     --no-start
   ```

2. Start task:
   ```bash
   orchestrator task start {task_id}
   ```

3. Verify cycle count:
   ```bash
   orchestrator task info {task_id}
   sqlite3 data/agent_orchestrator.db \
     "SELECT current_cycle FROM tasks WHERE id = '{task_id}'"
   ```

### Expected

- Task status: `completed`
- Failed: 0
- current_cycle >= 1 (the loop terminates early when all items pass in the
  first cycle; `max_cycles` is an upper bound, not a forced iteration count)
- Every log line shows `qa-phase: {rel_path}`
- For forced multi-cycle verification, see Doc 07 Scenario 3 (repeatable-test)
  or Doc 09 Scenario 3 (mixed-health)

---

## Checklist

| 1 | qa_only Workflow | ✅ | 2026-02-23 | chenhan | Status: completed, Failed: 0 |
| 2 | qa_fix Workflow | ✅ | 2026-02-23 | chenhan | Status: completed, QA通过, Fix跳过 |
| 3 | qa_fix_retest Workflow | ✅ | 2026-02-23 | chenhan | QA执行, 无tickets时 fix/retest跳过(符合设计) |
| 4 | QA Failure and Ticket Creation | ✅ | 2026-02-23 | chenhan | 结构化 QA 失败产物落库，tickets创建，Fix阶段执行 |
| 5 | Loop Mode (max_cycles) | ✅ | 2026-03-02 | chenhan | Status: completed, Failed: 0, current_cycle: 1 |
