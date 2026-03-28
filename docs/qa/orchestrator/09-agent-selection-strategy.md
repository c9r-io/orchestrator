---
self_referential_safe: true
---

# Orchestrator - Agent Selection Strategy (Scoring and Health)

**Module**: orchestrator
**Scope**: Validate multi-factor agent scoring, health degradation, and load balancing
**Scenarios**: 5
**Priority**: High

---

## Background

This document tests the agent selection strategy when multiple agents compete
for the same capability. Scenarios cover cost-based scoring, quality scoring,
health degradation after failures, retry status transitions, and load balancing.

Each scenario is verified through code review of the scoring/health/load-balancing
logic and by running the corresponding unit tests that exercise these paths with
deterministic inputs.

Key source modules:
- `select_agent_advanced` — multi-factor scoring with cost, metrics, health penalty
- `AgentHealthState` / `CapabilityHealth` — per-capability health tracking and disease marking
- `MetricsCollector` — runtime success/failure rate, load tracking, EMA calculations
- `TaskItemRepository` — status transitions for retry behavior

---

## Scenario 1: Cost-Based Scoring

### Preconditions

- Rust toolchain available

### Goal

Validate that the scoring algorithm gives lower-cost agents a higher score,
and that the cost-based selection strategy works correctly — via code review
and unit tests.

### Steps

1. **Code review** — locate the cost differential logic in scoring:
   ```bash
   rg -n "cost_differential\|cost.*score\|lower_cost" core/src/ | head -15
   ```

2. **Code review** — verify the cost-based strategy test exists:
   ```bash
   rg -n "test_cost_differential_lower_cost_scores_higher|test_selection_strategy_cost_based" core/src/selection.rs core/src/metrics.rs
   ```

3. **Unit test** — run cost-based scoring tests:
   ```bash
   cargo test --workspace --lib -- test_cost_differential_lower_cost_scores_higher 2>&1 | tail -5
   cargo test --workspace --lib -- test_selection_strategy_cost_based 2>&1 | tail -5
   ```

### Expected

- `test_cost_differential_lower_cost_scores_higher` passes: an agent with lower cost
  receives a higher score than one with higher cost, all else equal
- `test_selection_strategy_cost_based` passes: the cost-based strategy variant
  correctly weights cost in the scoring formula
- No panics

---

## Scenario 2: Quality Scoring

### Preconditions

- Rust toolchain available

### Goal

Validate that agents with higher success rates are preferred by the scoring
algorithm — via code review and unit tests.

### Steps

1. **Code review** — locate success-rate weighting logic:
   ```bash
   rg -n "success_rate\|metrics_impact\|high_success_rate" core/src/ | head -15
   ```

2. **Code review** — verify the quality/metrics scoring tests exist:
   ```bash
   rg -n "test_selection_strategy_success_rate_weighted|test_metrics_impact_high_success_rate_preferred" core/src/selection.rs core/src/metrics.rs
   ```

3. **Unit test** — run quality scoring tests:
   ```bash
   cargo test --workspace --lib -- test_selection_strategy_success_rate_weighted 2>&1 | tail -5
   cargo test --workspace --lib -- test_metrics_impact_high_success_rate_preferred 2>&1 | tail -5
   ```

### Expected

- `test_selection_strategy_success_rate_weighted` passes: success-rate-weighted strategy
  ranks agents with higher historical success rates above others
- `test_metrics_impact_high_success_rate_preferred` passes: metrics integration correctly
  boosts scores for agents with high success rates
- No panics

---

## Scenario 3: Health Degradation

### Preconditions

- Rust toolchain available

### Goal

Validate that after repeated infrastructure failures, a failing agent's score
is penalized and eventually the agent is marked diseased and excluded from
candidate selection — via code review and unit tests.

> **Note:** Only infrastructure failures trigger disease — not negative task
> conclusions (`exit_code > 0`). An agent that correctly completes its work
> but reports a negative finding is **not** penalized.

### Steps

1. **Code review** — locate health penalty and disease logic:
   ```bash
   rg -n "health_penalty\|consecutive_errors\|diseased\|is_capability_healthy\|is_agent_healthy" core/src/ | head -20
   ```

2. **Code review** — verify the health degradation tests exist:
   ```bash
   rg -n "test_health_penalty_diseased_agent|test_diseased_agent_filtered_from_candidates|is_capability_healthy_" core/src/metrics.rs core/src/selection.rs core/src/health.rs
   ```

3. **Unit test** — run health penalty and disease filtering tests:
   ```bash
   cargo test --workspace --lib -- test_health_penalty_diseased_agent 2>&1 | tail -5
   cargo test --workspace --lib -- test_diseased_agent_filtered_from_candidates 2>&1 | tail -5
   ```

4. **Unit test** — run capability-level health tests:
   ```bash
   cargo test --workspace --lib -- is_capability_healthy_ 2>&1 | tail -5
   ```

5. **Unit test** — run agent-level health tests:
   ```bash
   cargo test --workspace --lib -- is_agent_healthy_ 2>&1 | tail -5
   ```

### Expected

- `test_health_penalty_diseased_agent` passes: consecutive
  infrastructure failures progressively lower an agent's score
- `test_diseased_agent_filtered_from_candidates` passes: a diseased agent is
  excluded from the candidate pool entirely
- `is_capability_healthy_*` tests pass (6 tests): capability-level health checks
  correctly identify healthy, degraded, and diseased states
- `is_agent_healthy_*` tests pass (4 tests): agent-level health aggregation works
- No panics

---

## Scenario 4: Retry Status Transitions

### Preconditions

- Rust toolchain available

### Goal

Validate that task item status transitions for retry behave correctly —
a failed/unresolved item can be reset to pending and re-queued, and that
status fields (e.g., `started_at`) are set appropriately — via code review
and unit tests.

### Steps

1. **Code review** — locate task item status transition logic:
   ```bash
   rg -n "update_task_item_status\|mark_task_item_running\|started_at" core/src/ | head -15
   ```

2. **Code review** — verify status transition tests exist:
   ```bash
   rg -n "update_task_item_status_changes_status|mark_task_item_running_sets_started_at|recover_orphaned_running_items_" core/src/db_write.rs core/src/task_repository/tests/items_tests.rs core/src/task_repository/tests/state_tests.rs
   ```

3. **Unit test** — run status transition tests:
   ```bash
   cargo test --workspace --lib -- update_task_item_status_changes_status 2>&1 | tail -5
   cargo test --workspace --lib -- mark_task_item_running_sets_started_at 2>&1 | tail -5
   ```

4. **Unit test** — run orphaned item recovery tests (related to retry resilience):
   ```bash
   cargo test --workspace --lib -- recover_orphaned_running_items_ 2>&1 | tail -5
   ```

### Expected

- `update_task_item_status_changes_status` passes: status transitions between pending,
  running, completed, failed, and unresolved are validated
- `mark_task_item_running_sets_started_at` passes: the `started_at` timestamp
  is set when an item enters running state
- `recover_orphaned_running_items_*` tests pass: items stuck in running state
  (e.g., after a crash) are correctly recovered to pending
- No panics

---

## Scenario 5: Load Balancing

### Preconditions

- Rust toolchain available

### Goal

Validate that agent load tracking influences selection — agents with lower
current load receive higher scores, and the load increment/decrement cycle
works correctly — via code review and unit tests.

### Steps

1. **Code review** — locate load balancing logic:
   ```bash
   rg -n "load_balanced|record_run_start|record_run_end|current_load" core/src/metrics.rs
   ```

2. **Code review** — verify load balancing tests exist:
   ```bash
   rg -n "test_load_balanced_low_load_scores_higher|test_selection_strategy_load_balanced|test_load_balanced_score_never_negative|record_run_start|record_run_end" core/src/metrics.rs
   ```

3. **Unit test** — run load balancing scoring and strategy tests:
   ```bash
   cargo test --workspace --lib -- test_load_balanced_low_load_scores_higher 2>&1 | tail -5
   cargo test --workspace --lib -- test_selection_strategy_load_balanced 2>&1 | tail -5
   ```

4. **Unit test** — run load tracking lifecycle test:
   ```bash
   cargo test --workspace --lib -- test_load_balanced_score_never_negative 2>&1 | tail -5
   ```

### Expected

- `test_load_balanced_low_load_scores_higher` passes: agents with lower current
  load receive higher scores than busy agents
- `test_selection_strategy_load_balanced` passes: the load-balanced strategy
  variant correctly weights current load in scoring
- Code review confirms load counters increment via `record_run_start` and
  decrement via `record_run_end`, returning to baseline
- `test_load_balanced_score_never_negative` passes: score calculation never
  produces negative values even under high load
- No panics

---

## Notes

- Runtime metrics (`total_runs`, `successful_runs`, `avg_duration_ms`) are
  collected in-memory via `MetricsCollector` and influence `calculate_agent_score`
- Health state is tracked per-capability via `AgentHealthState.capability_health`
- **Agent isolation via project scope**: `apply -f ... --project <name>` deploys
  fixture agents into a project scope. Agent selection for project tasks uses
  project-scoped agents exclusively, so global/bootstrap agents never interfere
  with test assertions.
- **Clean state via `delete project/`**: `delete project/<name> --force` clears
  task data, project config, and auto-generated ticket files in one command —
  no need to delete the DB file.
- Load data is tracked in-memory only (not persisted to the event store)

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Cost-Based Scoring | PASS | 2026-03-28 | Claude | 2 tests: test_cost_differential_lower_cost_scores_higher + test_selection_strategy_cost_based passed |
| 2 | Quality Scoring | PASS | 2026-03-28 | Claude | 2 tests: test_selection_strategy_success_rate_weighted + test_metrics_impact_high_success_rate_preferred passed |
| 3 | Health Degradation | PASS | 2026-03-28 | Claude | 1 health penalty + 1 diseased filter + 6 capability health + 4 agent health = 12 tests passed |
| 4 | Retry Status Transitions | PASS | 2026-03-28 | Claude | 2 status + 2 started_at + 5 orphaned = 9 tests passed |
| 5 | Load Balancing | PASS | 2026-03-28 | Claude | 3 tests: test_load_balanced_low_load_scores_higher + test_selection_strategy_load_balanced + test_load_balanced_score_never_negative passed |
