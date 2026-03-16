# Orchestrator - Capability-Driven Orchestration (Routing Correctness)

**Module**: orchestrator
**Scope**: Validate that capability routing dispatches steps to the correct agent and template
**Scenarios**: 5
**Priority**: High

---

## Background

This document validates that the orchestrator correctly routes workflow steps to
agents based on their declared capabilities. Each scenario uses a dedicated
fixture where agent output is identifiable, so routing correctness can be
verified by inspecting logs.

Entry point: `orchestrator <command>`

---

## Scenario 1: Capability Isolation (qa-only vs fix-only agents)

### Preconditions

- Reset previous QA state — use `delete project/<name> --force` to clear task data and
  project config (including auto-generated tickets) without affecting other QA projects.
- Apply fixture into project scope — use `--project` to isolate fixture agents
  from resources belonging to other projects.

### Goal

Validate that qa steps dispatch to the qa-capable agent and fix steps dispatch
to the fix-capable agent when capabilities are disjoint.

### Fixture

`fixtures/manifests/bundles/capability-test.yaml`

- Workspace targets: `fixtures/qa-capability-test` (single file for simplicity)
- `agent_qa_only` — capabilities: `[qa]`, template emits structured ticket JSON and `exit 1`
  (QA intentionally fails to create tickets, triggering the fix step)
- `agent_fix_only` — capabilities: `[fix]`, template emits structured code-change JSON
- Workflow `test_capability` — steps: qa, fix

### Steps

1. Reset and apply into project scope:
   ```bash
   orchestrator delete project/qa-cap --force
   orchestrator apply -f fixtures/manifests/bundles/capability-test.yaml --project qa-cap
   ```

2. Create and run task:
   ```bash
   TASK_ID=$(orchestrator task create \
     --project qa-cap \
     --name "capability-test" \
     --goal "Test capability isolation" \
     --workspace default \
     --workflow test_capability \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}"
   ```

3. Inspect logs:
   ```bash
   orchestrator task logs "${TASK_ID}"
   ```

### Expected

- Task status: `completed`
- QA phase run contains structured ticket artifact output from `agent_qa_only`
- Fix phase run contains structured code-change artifact output from `agent_fix_only`
- No cross-contamination (qa agent never runs fix template, and vice versa)

> **Note**: The fix step only executes when active tickets exist. QA must fail
> (exit 1) to create tickets that trigger fix. If QA succeeds, the fix step is
> correctly skipped by design.

---

## Scenario 2: Multi-Agent Same Capability

### Preconditions

- Reset previous QA state — use `delete project/<name> --force` to clear task data, config,
  and auto-generated tickets without affecting other QA projects.
- Apply fixture into project scope — use `--project` to ensure only fixture
  agents participate in selection.

### Goal

Validate that when multiple agents share the same capability, the orchestrator
distributes work across them and each agent uses its own correct template.

### Fixture

`fixtures/manifests/bundles/multi-echo.yaml`

- `mock_echo_alpha` — capabilities: `[qa]`, template emits structured analysis JSON tagged `alpha-qa`
- `mock_echo_beta` — capabilities: `[qa]`, template emits structured analysis JSON tagged `beta-qa`
- Workflow `multi_agent_qa` — steps: qa (mode: once)

### Steps

1. Reset and apply into project scope:
   ```bash
   orchestrator delete project/qa-multi --force
   orchestrator apply -f fixtures/manifests/bundles/multi-echo.yaml --project qa-multi
   ```

2. Create and run task:
   ```bash
   TASK_ID=$(orchestrator task create \
     --project qa-multi \
     --name "multi-agent-test" \
     --goal "Test multi-agent distribution" \
     --workspace default \
     --workflow multi_agent_qa \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}"
   ```

3. Inspect logs and agent distribution:
   ```bash
   orchestrator task info "${TASK_ID}"
   sqlite3 data/agent_orchestrator.db \
     "SELECT cr.agent_id, COUNT(*) FROM command_runs cr
      WHERE cr.task_item_id IN (SELECT id FROM task_items WHERE task_id = '${TASK_ID}')
      GROUP BY cr.agent_id"
   ```

### Expected

- Task status: `completed`, failed: 0
- Both `mock_echo_alpha` and `mock_echo_beta` appear in `command_runs`
- Each agent produces its own identifiable output — no template mix-up

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| Other agents selected (e.g. agents from another QA fixture project) | Fixture not applied with `--project`, or task created under the wrong project | Re-apply with `--project <name>` and recreate the task in that same project |
| Items marked `unresolved` despite agents exiting 0 | Auto-ticket files in `fixtures/ticket/` from a prior run | Use `delete project/<name> --force` to clean tickets |

---

## Scenario 3: Repeatable Step Execution

### Preconditions

- Reset previous QA state — `delete project/<name> --force` clears task data, config, and auto-tickets.

### Goal

Validate that repeatable steps execute in every loop cycle.

### Fixture

`fixtures/manifests/bundles/repeatable-test.yaml`

- `test_agent` — capabilities: `[qa]`, template emits structured JSON containing `cycle {cycle}`
- Workflow `repeat_test` — steps: qa, loop mode: infinite, max_cycles: 3

### Steps

1. Reset and apply into project scope:
   ```bash
   orchestrator delete project/qa-repeat --force
   orchestrator apply -f fixtures/manifests/bundles/repeatable-test.yaml --project qa-repeat
   ```

2. Create and run task:
   ```bash
   TASK_ID=$(orchestrator task create \
     --project qa-repeat \
     --name "repeatable-test" \
     --goal "Loop workflow test" \
     --workspace default \
     --workflow repeat_test \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}"
   ```

3. Verify cycles:
   ```bash
   orchestrator task info "${TASK_ID}"
   sqlite3 data/agent_orchestrator.db \
     "SELECT current_cycle FROM tasks WHERE id = '${TASK_ID}'"
   ```

### Expected

- Task status: `completed`
- current_cycle >= 1 (loop terminates when all items pass; `max_cycles` is an
  upper bound, not a forced iteration count)
- QA step executed in every cycle that runs

---

## Scenario 4: Guard Step Termination

### Preconditions

- Reset previous QA state — `delete project/<name> --force` clears task data, config, and auto-tickets.

### Goal

Validate that a guard step can terminate the workflow loop.

### Fixture

`fixtures/manifests/bundles/guard-test.yaml`

- `test_agent` — QA template emits structured analysis JSON; capabilities include `loop_guard`
- Workflow `guard_test` — steps: qa + loop_guard (builtin), loop mode: infinite, max_cycles: 3

> **Note**: The `loop_guard` step type uses the **builtin** guard implementation.
> The builtin guard terminates based on `stop_when_no_unresolved` configuration
> and `max_cycles` limit — it does not parse agent JSON output.
> Since `stop_when_no_unresolved` defaults to `true`, the loop terminates after
> cycle 1 when all items pass. The loop only runs up to `max_cycles` when there
> are unresolved items remaining, or when `stop_when_no_unresolved` is explicitly
> set to `false` in the workflow's `loop` config.

### Steps

1. Reset and apply into project scope:
   ```bash
   orchestrator delete project/qa-guard --force
   orchestrator apply -f fixtures/manifests/bundles/guard-test.yaml --project qa-guard
   ```

2. Create and run task:
   ```bash
   TASK_ID=$(orchestrator task create \
     --project qa-guard \
     --name "guard-test" \
     --goal "Guard step test" \
     --workspace default \
     --workflow guard_test \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}"
   ```

3. Inspect result:
   ```bash
   orchestrator task info "${TASK_ID}"
   ```

### Expected

- Workflow `guard_test` appears in config
- Task creation and execution succeed — status: `completed`, failed: 0
- current_cycle = 1 — the loop terminates after cycle 1 because all items pass
  and `stop_when_no_unresolved` defaults to `true`

---

## Scenario 5: Performance Selection Fixture Execution

### Preconditions

- Reset previous QA state — `delete project/<name> --force` clears task data, config, and auto-tickets.

### Goal

Validate that a fixture with two agents of different costs loads correctly and
both agents are used for execution.

### Fixture

`fixtures/manifests/bundles/selection-perf-test.yaml`

- `fast_agent` — cost: 20, capabilities: `[qa, fix]`
- `quality_agent` — cost: 80, capabilities: `[qa, fix]`
- Workflow `selection_test` — steps: qa, fix (mode: once)

### Steps

1. Reset and apply into project scope:
   ```bash
   orchestrator delete project/qa-perf --force
   orchestrator apply -f fixtures/manifests/bundles/selection-perf-test.yaml --project qa-perf
   ```

2. Create and run task:
   ```bash
   TASK_ID=$(orchestrator task create \
     --project qa-perf \
     --name "selection-perf" \
     --goal "Selection performance baseline" \
     --workspace default \
     --workflow selection_test \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}"
   ```

3. Inspect result:
   ```bash
   orchestrator task info "${TASK_ID}"
   orchestrator task logs "${TASK_ID}"
   ```

### Expected

- Task status: `completed`, failed: 0
- Logs contain both `fast-qa`/`fast-fix` and `quality-qa`/`quality-fix` entries
- Both agents selected via capability-aware scoring

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Capability Isolation | ✅ | 2026-03-05 | auto | agent_qa_only → qa, agent_fix_only → fix |
| 2 | Multi-Agent Same Capability | ✅ | 2026-03-05 | auto | alpha/beta distributed, 0 failures |
| 3 | Repeatable Step Execution | ✅ | 2026-03-15 | claude | All 130 items passed in cycle 1, loop terminated early (expected behavior - all items passed) |
| 4 | Guard Step Termination | ✅ | 2026-03-16 | claude | current_cycle=1 correct: stop_when_no_unresolved defaults to true, loop terminates early when all items pass |
| 5 | Performance Selection Fixture | ✅ | 2026-03-05 | auto | fast_agent 58%, quality_agent 42% |
