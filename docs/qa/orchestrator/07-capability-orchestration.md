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

Entry point: `./scripts/orchestrator.sh <command>`

---

## Scenario 1: Capability Isolation (qa-only vs fix-only agents)

### Preconditions

- Fresh sqlite state

### Goal

Validate that qa steps dispatch to the qa-capable agent and fix steps dispatch
to the fix-capable agent when capabilities are disjoint.

### Fixture

`fixtures/manifests/bundles/capability-test.yaml`

- Workspace targets: `fixtures/qa-capability-test` (single file to avoid agent
  disease from repeated failures)
- `agent_qa_only` — capabilities: `[qa]`, template emits structured ticket JSON and `exit 1`
  (QA intentionally fails to create tickets, triggering the fix step)
- `agent_fix_only` — capabilities: `[fix]`, template emits structured code-change JSON
- Workflow `test_capability` — steps: qa, fix

### Steps

1. Initialize and apply:
   ```bash
   QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/capability-test.yaml
   ./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   ./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
   ```

2. Create and run task:
   ```bash
   ./scripts/orchestrator.sh task create \
     --name "capability-test" \
     --goal "Test capability isolation" \
     --project "${QA_PROJECT}" \
     --workflow test_capability
   ./scripts/orchestrator.sh task start --latest
   ```

3. Inspect logs:
   ```bash
   ./scripts/orchestrator.sh task logs {task_id}
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

- Fresh sqlite state

### Goal

Validate that when multiple agents share the same capability, the orchestrator
distributes work across them and each agent uses its own correct template.

### Fixture

`fixtures/manifests/bundles/multi-echo.yaml`

- `mock_echo_alpha` — capabilities: `[qa]`, template emits structured analysis JSON tagged `alpha-qa`
- `mock_echo_beta` — capabilities: `[qa]`, template emits structured analysis JSON tagged `beta-qa`
- Workflow `multi_agent_qa` — steps: qa (mode: once)

### Steps

1. Initialize and apply:
   ```bash
   QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/multi-echo.yaml
   ./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   ./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
   ```

2. Create and run task:
   ```bash
   ./scripts/orchestrator.sh task create \
     --name "multi-agent-test" \
     --goal "Test multi-agent distribution" \
     --project "${QA_PROJECT}" \
     --workflow multi_agent_qa
   ./scripts/orchestrator.sh task start --latest
   ```

3. Inspect logs:
   ```bash
   ./scripts/orchestrator.sh task logs {task_id}
   ```

### Expected

- Task status: `completed`, failed: 0
- Logs / persisted `output_json` contain both `alpha-qa` and `beta-qa` markers (both agents were used)
- Each agent produces its own identifiable output — no template mix-up

---

## Scenario 3: Repeatable Step Execution

### Preconditions

- Fresh sqlite state

### Goal

Validate that repeatable steps execute in every loop cycle.

### Fixture

`fixtures/manifests/bundles/repeatable-test.yaml`

- `test_agent` — capabilities: `[qa]`, template emits structured JSON containing `cycle {cycle}`
- Workflow `repeat_test` — steps: qa, loop mode: infinite, max_cycles: 3

### Steps

1. Initialize and apply:
   ```bash
   QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/repeatable-test.yaml
   ./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   ./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
   ```

2. Create and run task:
   ```bash
   ./scripts/orchestrator.sh task create \
     --name "repeatable-test" \
     --goal "Loop workflow test" \
     --project "${QA_PROJECT}" \
     --workflow repeat_test
   ./scripts/orchestrator.sh task start --latest
   ```

3. Verify cycles:
   ```bash
   ./scripts/orchestrator.sh task info {task_id}
   sqlite3 data/agent_orchestrator.db \
     "SELECT current_cycle FROM tasks WHERE id = '{task_id}'"
   ```

### Expected

- Task status: `completed`
- current_cycle >= 1 (loop terminates when all items pass; `max_cycles` is an
  upper bound, not a forced iteration count)
- QA step executed in every cycle that runs

---

## Scenario 4: Guard Step Termination

### Preconditions

- Fresh sqlite state

### Goal

Validate that a guard step can terminate the workflow loop.

### Fixture

`fixtures/manifests/bundles/guard-test.yaml`

- `test_agent` — QA template emits structured analysis JSON; loop_guard emits structured stop JSON (`{\"should_stop\":true}`)
- Workflow `guard_test` — steps: qa + loop_guard, loop mode: infinite, max_cycles: 3

### Steps

1. Initialize and apply:
   ```bash
   QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/guard-test.yaml
   ./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   ./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
   ```

2. Create and run task:
   ```bash
   ./scripts/orchestrator.sh task create \
     --name "guard-test" \
     --goal "Guard step test" \
     --project "${QA_PROJECT}" \
     --workflow guard_test
   ./scripts/orchestrator.sh task start --latest
   ```

3. Inspect result:
   ```bash
   ./scripts/orchestrator.sh task info {task_id}
   ```

### Expected

- Workflow `guard_test` appears in config
- Task creation and execution succeed
- Guard agent's structured stop output (`should_stop=true`) terminates the loop

---

## Scenario 5: Performance Selection Fixture Execution

### Preconditions

- Fresh sqlite state

### Goal

Validate that a fixture with two agents of different costs loads correctly and
both agents are used for execution.

### Fixture

`fixtures/manifests/bundles/selection-perf-test.yaml`

- `fast_agent` — cost: 20, capabilities: `[qa, fix]`
- `quality_agent` — cost: 80, capabilities: `[qa, fix]`
- Workflow `selection_test` — steps: qa, fix (mode: once)

### Steps

1. Initialize and apply:
   ```bash
   QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/selection-perf-test.yaml
   ./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   ./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
   ```

2. Create and run task:
   ```bash
   ./scripts/orchestrator.sh task create \
     --name "selection-perf" \
     --goal "Selection performance baseline" \
     --project "${QA_PROJECT}" \
     --workflow selection_test
   ./scripts/orchestrator.sh task start --latest
   ```

3. Inspect result:
   ```bash
   ./scripts/orchestrator.sh task info {task_id}
   ./scripts/orchestrator.sh task logs {task_id}
   ```

### Expected

- Task status: `completed`, failed: 0
- Logs contain both `fast-qa`/`fast-fix` and `quality-qa`/`quality-fix` entries
- Both agents selected via capability-aware scoring

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Capability Isolation | ☐ | | | |
| 2 | Multi-Agent Same Capability | ☐ | | | |
| 3 | Repeatable Step Execution | ☐ | | | |
| 4 | Guard Step Termination | ☐ | | | |
| 5 | Performance Selection Fixture | ☐ | | | |
