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

Every scenario starts from a clean slate. Because the echo-workflow fixture
uses `ticket_dir: fixtures/ticket`, stale auto-generated tickets from previous
runs can cause items to be marked "unresolved" even when QA passes. Always
remove them before starting:

```bash
# Clean stale auto-generated tickets (preserves README.md and manually created tickets)
rm -f fixtures/ticket/auto_*.md

QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/echo-workflow.yaml
```

Scenario 4 uses a different fixture (`fail-workflow.yaml`) — see its own
preconditions section.

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| qa_only/loop_test tasks fail with "unresolved" items despite QA exit 0 | Stale ticket files in `fixtures/ticket/` match item QA docs; finalize rules mark items with active tickets as "unresolved" when no fix step is present | Run `rm -f fixtures/ticket/auto_*.md` before testing |

---

## Scenario 1: qa_only Workflow

### Preconditions

- Common Preconditions (echo-workflow.yaml applied)

### Steps

1. Create task:
   ```bash
   ./scripts/orchestrator.sh task create \
     --name "qa-only-test" \
     --goal "Test QA only workflow" \
     --project "${QA_PROJECT}" \
     --workflow qa_only \
     --no-start
   ```

2. Start task and wait for completion:
   ```bash
   ./scripts/orchestrator.sh task start {task_id}
   ```

3. Inspect result:
   ```bash
   ./scripts/orchestrator.sh task info {task_id}
   ./scripts/orchestrator.sh task logs {task_id}
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
   ./scripts/orchestrator.sh task create \
     --name "qa-fix-test" \
     --goal "Test QA and fix workflow" \
     --project "${QA_PROJECT}" \
     --workflow qa_fix \
     --no-start
   ```

2. Start task:
   ```bash
   ./scripts/orchestrator.sh task start {task_id}
   ```

3. Inspect result:
   ```bash
   ./scripts/orchestrator.sh task info {task_id}
   ./scripts/orchestrator.sh task logs {task_id}
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
   ./scripts/orchestrator.sh task create \
     --name "qa-fix-retest-test" \
     --goal "Test full workflow" \
     --project "${QA_PROJECT}" \
     --workflow qa_fix_retest \
     --no-start
   ```

2. Start task:
   ```bash
   ./scripts/orchestrator.sh task start {task_id}
   ```

3. Inspect result:
   ```bash
   ./scripts/orchestrator.sh task info {task_id}
   ./scripts/orchestrator.sh task logs {task_id}
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
QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/fail-workflow.yaml
```

### Steps

1. Create task:
   ```bash
   ./scripts/orchestrator.sh task create \
     --name "ticket-test" \
     --goal "Test ticket creation" \
     --project "${QA_PROJECT}" \
     --workflow qa_fix \
     --no-start
   ```

2. Start task:
   ```bash
   ./scripts/orchestrator.sh task start {task_id}
   ```

3. Check task result and ticket directory:
   ```bash
   ./scripts/orchestrator.sh task info {task_id}
   ./scripts/orchestrator.sh task logs {task_id}
   ls workspace/${QA_PROJECT}/docs/ticket/
   ```

### Expected

- QA phase fails for every item (mock_fail exits 1)
- Ticket files are created under `workspace/${QA_PROJECT}/docs/ticket/`
- Fix phase executes after ticket scan
- Logs and DB command_runs show structured JSON outputs (`output_json`/`artifacts_json`); failing QA runs are marked by non-success status and ticket artifacts
- **Note**: If the agent becomes unhealthy after repeated QA failures, the task
  may report "No healthy agent found with capability: fix" — this is expected
  when health tracking marks the agent as diseased.

---

## Scenario 5: Loop Mode (max_cycles)

### Preconditions

- Common Preconditions (echo-workflow.yaml applied)

### Steps

1. Create task with loop_test workflow:
   ```bash
   ./scripts/orchestrator.sh task create \
     --name "loop-mode-test" \
     --goal "Test infinite loop with max_cycles" \
     --project "${QA_PROJECT}" \
     --workflow loop_test \
     --no-start
   ```

2. Start task:
   ```bash
   ./scripts/orchestrator.sh task start {task_id}
   ```

3. Verify cycle count:
   ```bash
   ./scripts/orchestrator.sh task info {task_id}
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
| 5 | Loop Mode (max_cycles) | ❌ | 2026-02-23 | chenhan | Status: failed(预期completed), Failed: 20 |
