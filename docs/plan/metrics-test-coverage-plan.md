# Implementation Plan: Expand Unit Test Coverage for core/src/metrics.rs

## Overview

**Target File:** `core/src/metrics.rs`
**Current State:** 5 tests exist
**Target State:** At least 18 total tests (add 13+ new tests)
**Constraint:** Only modify `#[cfg(test)] mod tests` section — no production code changes

---

## Files to Change

### `core/src/metrics.rs` — Test Section Only

**Changes:** Add 13+ new test functions to the existing `#[cfg(test)] mod tests` block.

| Test Function | Purpose | Lines to Add |
|---------------|---------|--------------|
| `test_selection_strategy_cost_based` | Verify CostBased scoring formula | ~15 |
| `test_selection_strategy_success_rate_weighted` | Verify SuccessRateWeighted formula | ~20 |
| `test_selection_strategy_performance_first` | Verify PerformanceFirst formula | ~20 |
| `test_selection_strategy_load_balanced` | Verify LoadBalanced formula | ~20 |
| `test_selection_strategy_capability_aware` | Verify CapabilityAware formula | ~20 |
| `test_ema_convergence_success` | EMA convergence over multiple successes | ~25 |
| `test_ema_convergence_failure` | EMA convergence over multiple failures | ~20 |
| `test_ema_convergence_mixed` | EMA behavior with mixed success/failure | ~25 |
| `test_boundary_zero_total_runs` | Score with zero runs | ~15 |
| `test_boundary_none_metrics` | Score with None metrics | ~15 |
| `test_boundary_none_health` | Score with None health | ~15 |
| `test_boundary_max_load` | Load penalty at high load values | ~20 |
| `test_health_penalty_diseased_agent` | -100 penalty for diseased agent | ~20 |
| `test_health_penalty_consecutive_errors` | -15 per consecutive error | ~20 |
| `test_capability_health_zero_total` | success_rate() returns 0.5 for zero total | ~10 |
| `test_load_decrement_from_zero` | Decrement from 0 stays at 0 | ~10 |
| `test_load_increment_decrement_cycle` | Full cycle of load operations | ~15 |

---

## Approach

### Strategy: Direct Unit Tests with Known Inputs/Outputs

1. **Isolated Tests:** Each test is self-contained with explicit input values and expected output ranges.

2. **Formula Verification:** For each `SelectionStrategy`, create a test with:
   - Known `cost`, `metrics`, `health`, `requirement` inputs
   - Manually calculated expected scores
   - Tolerance-based assertions (floating point comparison)

3. **EMA Testing Approach:**
   - Start with default metrics (`recent_success_rate = 0.5`)
   - Apply multiple `record_success` or `record_failure` calls
   - Verify EMA converges toward 1.0 (success) or 0.0 (failure)
   - Use tolerance for floating point comparison (e.g., `abs(expected - actual) < 0.01`)

4. **Boundary Testing Approach:**
   - Test edge cases explicitly: zero values, None options, maximum values
   - Verify graceful handling (no panics, sensible defaults)

5. **Helper Function (Optional):** Create a helper within the test module to reduce boilerplate for creating test fixtures:
   ```rust
   fn create_test_metrics(total_runs: u32, successful_runs: u32, avg_duration_ms: u64, current_load: u32) -> AgentMetrics {
       AgentMetrics { total_runs, successful_runs, avg_duration_ms, current_load, ..Default::default() }
   }
   ```

### Scoring Formula Reference (from production code)

| Strategy | Formula |
|----------|---------|
| `CostBased` | `cost_score * 1.0` |
| `SuccessRateWeighted` | `cost_score * 0.2 + success_rate_score * 0.8` |
| `PerformanceFirst` | `cost_score * 0.2 + performance_score * 0.6 + success_rate_score * 0.2` |
| `Adaptive` | `cost * w.cost + success_rate * w.success_rate + performance * w.performance + load_penalty * w.load + health_penalty` |
| `LoadBalanced` | `cost_score * 0.2 + success_rate_score * 0.3 + load_penalty.abs() * 0.5` |
| `CapabilityAware` | `cost_score * 0.15 + success_rate_score * 0.35 + performance_score * 0.2 + health_penalty.max(-50.0)` |

### Score Component Calculations

- `cost_score = 100.0 - cost.unwrap_or(50)`
- `success_rate_score = (successful_runs / total_runs) * 100.0` or `50.0` if no metrics
- `performance_score = (60000 / avg_duration_ms).min(100.0)` or `50.0` if no data
- `load_penalty = -(current_load * 10.0).min(50.0)`
- `health_penalty = -100.0` (diseased) or `-(consecutive_errors * 15.0)` (has errors) or `0.0`

---

## Scope Boundary

### IN Scope

- Adding new test functions to `#[cfg(test)] mod tests`
- Adding helper functions within the test module (if needed for readability)
- Testing all 6 `SelectionStrategy` variants with known inputs/outputs
- Testing EMA convergence for `record_success` and `record_failure`
- Testing boundary conditions (zero runs, None values, max load)
- Testing health penalty logic (diseased_until, consecutive_errors)
- Testing `CapabilityHealth::success_rate()` with zero total
- Testing load increment/decrement edge cases
- Using `approx` or manual tolerance for float comparisons

### OUT of Scope

- Any changes to production code (structs, functions, constants)
- Adding new dependencies to `Cargo.toml`
- Modifying `SelectionWeights` defaults
- Changing EMA alpha value (0.3)
- Testing `is_agent_globally_healthy` directly (it's private, test via `calculate_agent_score`)
- Testing `chrono` timestamp behavior (use fixed values or ignore timestamp fields)
- Integration tests
- Performance benchmarks
- Adding new traits or abstractions

---

## Test Strategy

### Unit Test Structure

Each test follows this pattern:
```rust
#[test]
fn test_<descriptive_name>() {
    // 1. Arrange: Create inputs
    let metrics = Some(AgentMetrics { ... });
    let health = Some(AgentHealthState { ... });
    let requirement = SelectionRequirement { strategy: ..., ... };

    // 2. Act: Call the function
    let score = calculate_agent_score("test_agent", cost, &metrics, &health, &requirement);

    // 3. Assert: Verify outputs
    assert!((score.total_score - expected).abs() < 0.01);
}
```

### Test Cases Summary

| # | Test Name | Category | Key Assertion |
|---|-----------|----------|---------------|
| 1 | `test_new_agent_metrics` | (existing) | Default values correct |
| 2 | `test_record_success` | (existing) | Single success updates metrics |
| 3 | `test_record_failure` | (existing) | Single failure updates metrics |
| 4 | `test_capability_health_rate` | (existing) | 8/10 = 0.8 |
| 5 | `test_agent_score_calculation` | (existing) | Score > 0, cost_score = 70 |
| 6 | `test_selection_strategy_cost_based` | Scoring | `total_score == cost_score` |
| 7 | `test_selection_strategy_success_rate_weighted` | Scoring | `cost*0.2 + success*0.8` |
| 8 | `test_selection_strategy_performance_first` | Scoring | `cost*0.2 + perf*0.6 + success*0.2` |
| 9 | `test_selection_strategy_load_balanced` | Scoring | `cost*0.2 + success*0.3 + load*0.5` |
| 10 | `test_selection_strategy_capability_aware` | Scoring | `cost*0.15 + success*0.35 + perf*0.2 + health.max(-50)` |
| 11 | `test_ema_convergence_success` | EMA | After 10 successes, rate > 0.95 |
| 12 | `test_ema_convergence_failure` | EMA | After 10 failures, rate < 0.05 |
| 13 | `test_ema_convergence_mixed` | EMA | Mixed sequence converges correctly |
| 14 | `test_boundary_zero_total_runs` | Boundary | Uses `recent_success_rate * 100` |
| 15 | `test_boundary_none_metrics` | Boundary | Returns neutral scores (50.0) |
| 16 | `test_boundary_none_health` | Boundary | health_penalty = 0.0 |
| 17 | `test_boundary_max_load` | Boundary | load_penalty capped at -50 |
| 18 | `test_health_penalty_diseased_agent` | Health | health_penalty = -100.0 |
| 19 | `test_health_penalty_consecutive_errors` | Health | -15 per consecutive error |
| 20 | `test_capability_health_zero_total` | Boundary | Returns 0.5 (neutral) |
| 21 | `test_load_decrement_from_zero` | Load | Stays at 0 |
| 22 | `test_load_increment_decrement_cycle` | Load | Full cycle works |

### Floating Point Tolerance

Use `abs_diff` assertions with tolerance `0.01` for all float comparisons:
```rust
assert!((actual - expected).abs() < 0.01, "Expected {}, got {}", expected, actual);
```

---

## QA Strategy

### Task Classification: REFACTORING (Test Coverage Expansion)

This task adds unit tests to existing production code without changing behavior.

**QA Approach:**
- **Unit tests are the QA:** The tests themselves verify behavioral correctness.
- **No new QA docs needed:** This is purely a test coverage expansion task.
- **No scenario-based QA:** No new functionality or user-facing changes.

### Validation Checklist

1. All new tests pass: `cargo test --package agent_orchestrator metrics`
2. No regression in existing tests
3. Coverage increases (verify with `cargo llvm-cov` if available)
4. Code compiles without warnings: `cargo clippy`

---

## Implementation Order

1. **Helper function** (optional, reduces duplication)
2. **SelectionStrategy tests** (6 tests for formula verification)
3. **EMA convergence tests** (3 tests for success/failure/mixed)
4. **Boundary condition tests** (4 tests for edge cases)
5. **Health penalty tests** (2 tests for diseased/consecutive errors)
6. **CapabilityHealth test** (1 test for zero total)
7. **Load operation tests** (2 tests for increment/decrement)

---

## Risk Assessment

| Risk | Mitigation |
|------|------------|
| Float comparison flakiness | Use tolerance-based assertions (0.01) |
| Time-dependent tests (diseased_until) | Use `Utc::now() + Duration::seconds(X)` for future, `Utc::now() - Duration::seconds(X)` for past |
| Test isolation | Each test creates fresh metrics/health instances |
| Code bloat | Keep tests focused; extract helper if >3 tests share setup |

---

## Acceptance Criteria

- [ ] At least 18 total tests exist in `#[cfg(test)] mod tests`
- [ ] All 6 `SelectionStrategy` variants have dedicated tests with known inputs/outputs
- [ ] EMA convergence is tested for success, failure, and mixed sequences
- [ ] Boundary conditions (zero runs, None metrics, None health, max load) are tested
- [ ] Health penalties (diseased agent, consecutive errors) are tested
- [ ] `CapabilityHealth::success_rate()` with zero total is tested
- [ ] Load increment/decrement boundaries are tested
- [ ] All tests pass: `cargo test metrics`
- [ ] No production code changes
