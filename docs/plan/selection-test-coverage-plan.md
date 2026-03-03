# Plan: Expand Unit Test Coverage for `core/src/selection.rs`

## Files to Change

| File | Change |
|------|--------|
| `core/src/selection.rs` | Add 8+ new tests inside the existing `#[cfg(test)] mod tests` block (lines 109–227). No production code changes. |

## Approach

All new tests operate on the two public functions `select_agent_advanced` and `select_agent_by_preference` by constructing specific `HashMap<String, AgentConfig>`, `HashMap<String, AgentHealthState>`, `HashMap<String, AgentMetrics>`, and `HashSet<String>` inputs, then asserting on the returned `Result<(String, String)>`.

The existing helper `make_test_agent` creates agents with a given capability and cost. New tests will reuse it and add inline construction of `AgentMetrics` and `AgentHealthState` where needed — no new helper functions or abstractions.

Key constraint: `select_agent_advanced` picks randomly from the top-3 scored candidates. Tests that need deterministic assertions must ensure either (a) only 1 candidate exists, or (b) all top-3 candidates satisfy the assertion. For score-comparison tests (cost, metrics, health), we use **2 agents where only 1 has the target capability** or **ensure the score gap is large enough that the better agent is always in the top-1 slot** and run the selection in a small retry loop to confirm the winner appears.

### New Tests (8 tests, bringing total from 6 → 14+)

1. **`test_cost_differential_lower_cost_scores_higher`** — Two agents both with capability "qa", one cost=10, one cost=90. Empty metrics/health. With default `CapabilityAware` strategy, cost_score difference (90 vs 10) dominates. Assert the lower-cost agent is selected (single candidate after scoring — the gap is 80 points on cost alone; since top-3 includes both, run 20 iterations and assert the cheap agent wins majority).

2. **`test_metrics_impact_high_success_rate_preferred`** — Two agents with capability "qa", same cost=50. Agent A has metrics: `total_runs=100, successful_runs=95, avg_duration_ms=2000`. Agent B has metrics: `total_runs=100, successful_runs=20, avg_duration_ms=50000`. Agent A scores much higher on both success_rate_score and performance_score. Run 20 iterations, assert agent A wins majority.

3. **`test_health_penalty_consecutive_errors_lowers_score`** — Two agents with capability "qa", same cost=30, same metrics. Agent A has `AgentHealthState { consecutive_errors: 5, ..Default::default() }` (penalty = -75). Agent B has default health (penalty = 0). Assert agent B is selected (run 20 iterations, agent B should dominate).

4. **`test_all_candidates_excluded_returns_error`** — Two agents with capability "qa". Both agent IDs in the `excluded_agents` set. Assert `select_agent_advanced` returns `Err` containing "No healthy agent".

5. **`test_single_candidate_deterministic`** — One agent with capability "qa". Call `select_agent_advanced` 10 times, assert it always returns the same agent ID and command. This verifies no panic on `gen_range(0..1)`.

6. **`test_preference_empty_capabilities_non_default_name`** — One agent with `capabilities: vec![]` and `metadata.name = "custom_blank"`. `select_agent_by_preference` should match it via the `cfg.capabilities.is_empty()` branch (line 85) and return it. Assert returned agent_id and command.

7. **`test_preference_all_specialized_random_fallback`** — Two agents, both with non-empty capabilities and names that are NOT "default_agent". `select_agent_by_preference` should fall through the preference loop and hit the random fallback (line 95). Assert one of the two agent IDs is returned.

8. **`test_diseased_agent_filtered_from_candidates`** — Two agents with capability "qa". Agent A has `diseased_until` set to future time with no capability_health data (so `is_capability_healthy` returns false). Agent B is healthy. Assert only agent B is selected.

## Scope Boundary

### IN Scope
- Adding new `#[test]` functions inside `core/src/selection.rs` `mod tests`
- Reusing existing `make_test_agent` helper
- Constructing `AgentMetrics`, `AgentHealthState`, `AgentConfig` inline in tests

### OUT of Scope
- Any changes to production code (lines 1–107)
- Changes to `core/src/metrics.rs`, `core/src/health.rs`, or `core/src/config/agent.rs`
- New helper functions, traits, or abstractions
- New test fixture files
- Changes to any other test modules

## Test Strategy

All 8 new tests are unit tests within the existing `#[cfg(test)]` module. They exercise:

| Category | Tests | What's validated |
|----------|-------|-----------------|
| Scoring integration | #1, #2, #3, #8 | `select_agent_advanced` correctly propagates cost, metrics, and health data through `calculate_agent_score` to influence selection |
| Edge cases | #4, #5 | All-excluded error path; single-candidate no-panic path |
| `select_agent_by_preference` branches | #6, #7 | Empty-capabilities match (non-default name); random fallback when no preference match |

For tests involving randomness (#1, #2, #3, #7, #8): run in a loop (20 iterations) and assert the expected agent wins at least 15/20 times (or 100% for cases where score gap makes it mathematically certain the target is always rank-1 and top-3 only has 2 agents).

Run with: `cargo test -p agent-orchestrator selection::tests`

## QA Strategy

**Classification: TEST-ONLY change (expanding unit test coverage)**

This task adds only `#[cfg(test)]` code — no production behavior changes. QA validation is the tests themselves:

- All 14+ tests pass with `cargo test`
- No new QA documents needed under `docs/qa/`
- No security or UI/UX implications
