# Orchestrator - Agent Selection Strategy (Scoring and Health)

**Module**: orchestrator
**Scope**: Validate multi-factor agent scoring, health degradation, and load balancing
**Scenarios**: 5
**Priority**: High

---

## Background

This document tests the agent selection strategy when multiple agents compete
for the same capability. Scenarios cover cost-based scoring, quality scoring,
health degradation after failures, manual retry, and load balancing.

Each scenario uses a dedicated fixture with concrete agents, so assertions are
grounded in real execution output rather than conceptual descriptions.

Entry point: `./scripts/orchestrator.sh <command>`

---

## Scenario 1: Cost-Based Scoring

### Preconditions

- Fresh sqlite state

### Goal

Validate that two agents with different costs are both used, and that the
lower-cost agent is selected more frequently by the scoring algorithm.

### Fixture

`fixtures/manifests/bundles/selection-perf-test.yaml`

- `fast_agent` — cost: 20, capabilities: `[qa, fix]`, templates: `fast-qa` / `fast-fix`
- `quality_agent` — cost: 80, capabilities: `[qa, fix]`, templates: `quality-qa` / `quality-fix`
- Workflow `selection_test` — steps: qa, fix (mode: once)

### Steps

1. Reset and apply:
   ```bash
   QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
   ./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
   ./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/selection-perf-test.yaml
   ```

2. Create and run task:
   ```bash
   ./scripts/orchestrator.sh task create \
     --name "cost-scoring-test" \
     --goal "Test cost-based scoring" \
     --project "${QA_PROJECT}" \
     --workflow selection_test
   ./scripts/orchestrator.sh task start --latest
   ```

3. Inspect logs to count agent selection:
   ```bash
   ./scripts/orchestrator.sh task logs {task_id}
   # Count occurrences of "fast-qa" vs "quality-qa"
   ```

### Expected

- Task status: `completed`, failed: 0
- Both agents appear in logs (both `fast-qa` and `quality-qa`)
- Selection uses capability-aware strategy (default)
- Lower-cost agent (`fast_agent`) tends to appear more frequently

---

## Scenario 2: Quality Scoring

### Preconditions

- Fresh sqlite state

### Goal

Validate that two agents with different costs but identical capability are
both used successfully.

### Fixture

`fixtures/manifests/bundles/selection-quality-test.yaml`

- `proven_agent` — cost: 50, capabilities: `[qa]`, template: `echo 'proven-qa'`
- `new_agent` — cost: 20, capabilities: `[qa]`, template: `echo 'new-qa'`
- Workflow `quality_selection_test` — steps: qa (mode: once)

### Steps

1. Reset and apply:
   ```bash
   QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
   ./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
   ./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/selection-quality-test.yaml
   ```

2. Create and run task:
   ```bash
   ./scripts/orchestrator.sh task create \
     --name "quality-scoring-test" \
     --goal "Test quality-based scoring" \
     --project "${QA_PROJECT}" \
     --workflow quality_selection_test
   ./scripts/orchestrator.sh task start --latest
   ```

3. Inspect logs:
   ```bash
   ./scripts/orchestrator.sh task logs {task_id}
   ```

### Expected

- Task status: `completed`, failed: 0
- Both `proven-qa` and `new-qa` appear in logs
- All items pass regardless of which agent is selected

---

## Scenario 3: Health Degradation

### Preconditions

- Fresh sqlite state

### Goal

Validate that after repeated failures, the failing agent is marked diseased
and the healthy agent handles an increasing share of work across cycles.

### Fixture

`fixtures/manifests/bundles/mixed-health.yaml`

- `mock_echo` — capabilities: `[qa]`, template: `echo 'echo-qa: {rel_path}'` (always succeeds)
- `mock_fail` — capabilities: `[qa]`, template: `echo 'QA failed' && exit 1` (always fails)
- Workflow `health_test` — steps: qa, loop mode: infinite, max_cycles: 3

### Steps

1. Reset and apply:
   ```bash
   QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
   ./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
   ./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/mixed-health.yaml
   ```

2. Create and run task:
   ```bash
   ./scripts/orchestrator.sh task create \
     --name "health-degradation-test" \
     --goal "Test health degradation" \
     --project "${QA_PROJECT}" \
     --workflow health_test
   ./scripts/orchestrator.sh task start --latest
   ```

3. Inspect logs across cycles:
   ```bash
   ./scripts/orchestrator.sh task logs {task_id}
   # Count "echo-qa:" vs "QA failed" entries to observe health shift
   ```

### Expected

- current_cycle = 3
- Cycle 1: both agents selected (mix of `echo-qa:` and `QA failed`)
- Later cycles: `mock_fail` is marked diseased after consecutive failures;
  `mock_echo` handles a larger proportion of items
- Items assigned to `mock_echo` always pass; items assigned to `mock_fail`
  always fail

---

## Scenario 4: Manual Retry Behavior

### Preconditions

- Fresh sqlite state
- Task from Scenario 3 (or any task with failed items)

### Goal

Validate that `task retry` resets a failed item to pending and re-queues it.

### Steps

1. Use a completed task with failed items (e.g. from Scenario 3):
   ```bash
   ./scripts/orchestrator.sh task info {task_id}
   ```

2. Pick a failed item and retry:
   ```bash
   ./scripts/orchestrator.sh task retry {task_item_id}
   ```

3. Inspect item status:
   ```bash
   ./scripts/orchestrator.sh task info {task_id}
   ```

### Expected

- `task retry` resets the item status to `pending`
- Automatic retry with agent rotation is **not implemented** — the same agent
  may be selected again
- After 2+ consecutive failures, the failing agent is marked diseased and
  excluded from future selection via health tracking

---

## Scenario 5: Load Balancing

### Preconditions

- Fresh sqlite state

### Goal

Validate that agent load tracking influences selection during execution.

### Fixture

`fixtures/manifests/bundles/selection-perf-test.yaml` (same as Scenario 1)

### Steps

1. Reset and apply:
   ```bash
   QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
   ./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
   ./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/selection-perf-test.yaml
   ```

2. Create and run task:
   ```bash
   ./scripts/orchestrator.sh task create \
     --name "load-balance-test" \
     --goal "Test load balancing" \
     --project "${QA_PROJECT}" \
     --workflow selection_test
   ./scripts/orchestrator.sh task start --latest
   ```

3. Inspect distribution:
   ```bash
   ./scripts/orchestrator.sh task logs {task_id}
   ```

### Expected

- Task status: `completed`, failed: 0
- `increment_load` called before each execution, `decrement_load` after
- Higher-load agents receive lower scores during concurrent selection
- Load data is tracked in-memory only (not persisted to events)

---

## Notes

- Runtime metrics (`total_runs`, `successful_runs`, `avg_duration_ms`) are
  collected in-memory via `MetricsCollector` and influence `calculate_agent_score`
- There is no dedicated CLI command to inspect raw agent metrics; verify
  indirectly via log distribution across agents
- Health state is tracked per-capability via `AgentHealthState.capability_health`

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Cost-Based Scoring | ☐ | | | |
| 2 | Quality Scoring | ☐ | | | |
| 3 | Health Degradation | ☐ | | | |
| 4 | Manual Retry Behavior | ☐ | | | |
| 5 | Load Balancing | ☐ | | | |
