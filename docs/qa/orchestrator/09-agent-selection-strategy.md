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

- Reset previous QA state — use `qa project reset` to clear task data, config,
  and auto-generated tickets without destroying global state.
- Apply fixture into project scope — use `--project` to ensure only fixture
  agents participate in selection.

### Goal

Validate that two agents with different costs are both used, and that the
lower-cost agent is selected more frequently by the scoring algorithm.

### Fixture

`fixtures/manifests/bundles/selection-perf-test.yaml`

- `fast_agent` — cost: 20, capabilities: `[qa, fix]`, templates emit structured JSON markers `fast-qa` / `fast-fix`
- `quality_agent` — cost: 80, capabilities: `[qa, fix]`, templates emit structured JSON markers `quality-qa` / `quality-fix`
- Workflow `selection_test` — steps: qa, fix (mode: once)

### Steps

1. Reset and apply into project scope:
   ```bash
   ./scripts/orchestrator.sh qa project reset qa-cost --force
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/selection-perf-test.yaml --project qa-cost
   ```

2. Create and run task:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create \
     --project qa-cost \
     --name "cost-scoring-test" \
     --goal "Test cost-based scoring" \
     --workspace default \
     --workflow selection_test \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/orchestrator.sh task start "${TASK_ID}"
   ```

3. Inspect logs to count agent selection:
   ```bash
   ./scripts/orchestrator.sh task logs "${TASK_ID}"
   # Count occurrences of structured output markers "fast-qa" vs "quality-qa"
   ```

### Expected

- Task status: `completed`, failed: 0
- Both agents appear in logs (both `fast-qa` and `quality-qa`)
- Selection uses capability-aware strategy (default)
- The scoring algorithm gives the lower-cost agent (`fast_agent`) a higher
  score, but since both agents are within the top-3 candidate pool, actual
  selection is randomized — distribution may be roughly equal

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| Only one agent selected across all runs | Project-scoped `apply` may have failed to register one agent, or the agent lacks the required capability | Run `./scripts/orchestrator.sh describe agent/fast_agent --project qa-cost` and `describe agent/quality_agent --project qa-cost` to verify both agents exist with correct capabilities |
| Selection distribution is heavily skewed | With only 2 agents in the top-3 pool, randomization should give ~50/50; extreme skew indicates only 1 agent was available | Check `resolve_effective_agents` returns both agents for the project |

---

## Scenario 2: Quality Scoring

### Preconditions

- Reset previous QA state — `qa project reset` clears task data, config, and auto-tickets.

### Goal

Validate that two agents with different costs but identical capability are
both used successfully.

### Fixture

`fixtures/manifests/bundles/selection-quality-test.yaml`

- `proven_agent` — cost: 50, capabilities: `[qa]`, template emits structured marker `proven-qa`
- `new_agent` — cost: 20, capabilities: `[qa]`, template emits structured marker `new-qa`
- Workflow `quality_selection_test` — steps: qa (mode: once)

### Steps

1. Reset and apply into project scope:
   ```bash
   ./scripts/orchestrator.sh qa project reset qa-quality --force
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/selection-quality-test.yaml --project qa-quality
   ```

2. Create and run task:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create \
     --project qa-quality \
     --name "quality-scoring-test" \
     --goal "Test quality-based scoring" \
     --workspace default \
     --workflow quality_selection_test \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/orchestrator.sh task start "${TASK_ID}"
   ```

3. Inspect agent selection via DB (more reliable than logs for verifying
   selection distribution):
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT agent_id, COUNT(*) FROM command_runs
      WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '${TASK_ID}')
      GROUP BY agent_id;"
   ```

4. Optionally inspect logs:
   ```bash
   ./scripts/orchestrator.sh task logs "${TASK_ID}"
   ```

### Expected

- Both `proven_agent` and `new_agent` appear in the `command_runs` query
- No other agents appear (DB isolation ensures only fixture-defined agents exist)
- All items from agents that exit 0 produce analysis findings

---

## Scenario 3: Health Degradation

### Preconditions

- Reset previous QA state — `qa project reset` clears task data, config, and auto-tickets.

### Goal

Validate that after repeated failures, the failing agent is marked diseased
and the healthy agent handles an increasing share of work across cycles.

### Fixture

`fixtures/manifests/bundles/mixed-health.yaml`

- `mock_echo` — capabilities: `[qa]`, template emits structured analysis JSON (always succeeds)
- `mock_fail` — capabilities: `[qa]`, template emits structured ticket JSON and `exit 1` (always fails)
- Workflow `health_test` — steps: qa, loop mode: infinite, max_cycles: 3

### Steps

1. Reset and apply into project scope:
   ```bash
   ./scripts/orchestrator.sh qa project reset qa-health --force
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/mixed-health.yaml --project qa-health
   ```

2. Create and run task:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create \
     --project qa-health \
     --name "health-degradation-test" \
     --goal "Test health degradation" \
     --workspace default \
     --workflow health_test \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/orchestrator.sh task start "${TASK_ID}"
   ```

3. Verify agent selection via DB (`task logs` does not show output from
   failed agent runs, so DB is the authoritative source):
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT agent_id, COUNT(*), GROUP_CONCAT(DISTINCT exit_code)
      FROM command_runs
      WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '${TASK_ID}')
      GROUP BY agent_id;"
   ```

4. Optionally check logs (only successful runs appear here):
   ```bash
   ./scripts/orchestrator.sh task logs "${TASK_ID}"
   ```

### Expected

- `mock_fail` appears in `command_runs` with a small count (typically 1–2)
  and `exit_code = 1`
- After 2 consecutive failures `mock_fail` is marked diseased and excluded
  from subsequent selection
- `mock_echo` handles the vast majority of runs across all cycles
- Task status: `failed` with some `unresolved` items — this is **expected**
  because `mock_fail`'s items generate tickets that trigger the
  `fallback_unresolved_with_tickets` finalize rule
- `task logs` will show only `echo-qa` markers because failed runs are not
  surfaced by the logs command; use the DB query to confirm `mock_fail` was
  selected

---

## Scenario 4: Manual Retry Behavior

### Preconditions

- Task from Scenario 3 (or any task with `unresolved`/`failed` items)
- Do **not** delete the DB — this scenario depends on the state from Scenario 3

### Goal

Validate that `task retry` resets a failed item to pending and re-queues it.

### Steps

1. Use a completed task with failed items (e.g. from Scenario 3):
   ```bash
   ./scripts/orchestrator.sh task info {task_id}
   ```

2. Pick an unresolved/failed item and verify its current status:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT id, status FROM task_items WHERE task_id = '{task_id}' AND status = 'unresolved' LIMIT 1;"
   ```

3. Retry with `--detach` to verify the reset separately from re-execution:
   ```bash
   ./scripts/orchestrator.sh task retry {task_item_id} --detach
   ```

4. Immediately check item status (before task loop runs):
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT id, status FROM task_items WHERE id = '{task_item_id}';"
   ```

### Expected

- Immediately after `task retry --detach`, the item status is `pending`
- Without `--detach`, `task retry` resets to `pending` and then runs the full
  task loop; the item is re-finalized after execution, so the final status
  depends on the finalize rules (it may return to `unresolved` if the
  underlying issue persists)
- Automatic retry with agent rotation is **not implemented** — the same agent
  may be selected again
- After 2+ consecutive failures, the failing agent is marked diseased and
  excluded from future selection via health tracking

---

## Scenario 5: Load Balancing

### Preconditions

- Reset previous QA state — `qa project reset` clears task data, config, and auto-tickets.

### Goal

Validate that agent load tracking influences selection during execution.

### Fixture

`fixtures/manifests/bundles/selection-perf-test.yaml` (same as Scenario 1)

### Steps

1. Reset and apply into project scope:
   ```bash
   ./scripts/orchestrator.sh qa project reset qa-load --force
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/selection-perf-test.yaml --project qa-load
   ```

2. Create and run task:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create \
     --project qa-load \
     --name "load-balance-test" \
     --goal "Test load balancing" \
     --workspace default \
     --workflow selection_test \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/orchestrator.sh task start "${TASK_ID}"
   ```

3. Inspect distribution:
   ```bash
   ./scripts/orchestrator.sh task logs "${TASK_ID}"
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
- **Agent isolation via project scope**: Use `apply -f ... --project <name>` to
  deploy fixture agents into a project scope. Agent selection for project tasks
  uses project-scoped agents exclusively, so global/bootstrap agents never
  interfere with test assertions.
- **Clean state via `qa project reset`**: Use `qa project reset <name> --force`
  before each scenario to clear task data, project config, and auto-generated
  ticket files in one command — no need to delete the DB file.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Cost-Based Scoring | ☐ | | | |
| 2 | Quality Scoring | ☐ | | | |
| 3 | Health Degradation | ☐ | | | |
| 4 | Manual Retry Behavior | ☐ | | | |
| 5 | Load Balancing | ☐ | | | |
