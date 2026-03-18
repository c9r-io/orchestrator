---
self_referential_safe: true
---

# Orchestrator - Capability-Driven Orchestration (Routing Correctness)

**Module**: orchestrator
**Scope**: Validate that capability routing dispatches steps to the correct agent and template
**Scenarios**: 5
**Priority**: High

---

## Background

This document validates that the orchestrator correctly routes workflow steps to
agents based on their declared capabilities. Each scenario verifies routing logic
through code review of the selection, loop engine, and metrics modules, confirmed
by running the corresponding unit tests.

Key source files:
- `core/src/selection.rs` — agent selection and capability matching
- `core/src/metrics.rs` — selection strategy scoring (capability-aware, cost-based)
- `crates/orchestrator-scheduler/src/scheduler/loop_engine/tests.rs` — loop execution, guard segments, cycle limits

---

## Scenario 1: Capability Isolation (qa-only vs fix-only agents)

### Preconditions

- Rust toolchain available

### Goal

Validate that qa steps dispatch to the qa-capable agent and fix steps dispatch
to the fix-capable agent when capabilities are disjoint.

### Steps

1. Review the capability-matching selection logic:
   ```bash
   rg -n "resolve_effective_agents" core/src/selection.rs
   ```

2. Confirm that `select_agent_advanced` filters candidates by matching capability:
   ```bash
   rg -n "test_select_agent_advanced_finds_matching_capability" core/src/selection.rs
   ```

3. Run the unit tests that verify capability isolation:
   ```bash
   cargo test --workspace --lib -- test_select_agent_advanced_finds_matching_capability
   cargo test --workspace --lib -- resolve_effective_agents_returns_project_agents_when_capability_matches
   ```

### Expected

- `test_select_agent_advanced_finds_matching_capability` passes — confirms that an agent
  is only selected when its declared capabilities include the requested capability
- `resolve_effective_agents_returns_project_agents_when_capability_matches` passes —
  confirms that project-scoped agents are returned only when their capabilities match
- No cross-contamination: an agent without the requested capability is never selected

---

## Scenario 2: Multi-Agent Same Capability

### Preconditions

- Rust toolchain available

### Goal

Validate that when multiple agents share the same capability, the orchestrator
distributes work across them and each agent uses its own correct template.

### Steps

1. Review the multi-candidate selection logic (random fallback and deterministic single-candidate paths):
   ```bash
   rg -n "test_select_agent_by_preference_random_fallback" core/src/selection.rs
   rg -n "test_single_candidate_deterministic" core/src/selection.rs
   ```

2. Review exclusion logic to confirm agents are not incorrectly filtered:
   ```bash
   rg -n "test_select_agent_advanced_excludes_agents" core/src/selection.rs
   ```

3. Run the unit tests:
   ```bash
   cargo test --workspace --lib -- test_select_agent_by_preference_random_fallback
   cargo test --workspace --lib -- test_single_candidate_deterministic
   ```

### Expected

- `test_select_agent_by_preference_random_fallback` passes — confirms that when multiple
  agents match the same capability, selection distributes across candidates
- `test_single_candidate_deterministic` passes — confirms that a single matching
  candidate is always deterministically selected
- Each agent retains its own identity; no template mix-up occurs

---

## Scenario 3: Repeatable Step Execution

### Preconditions

- Rust toolchain available

### Goal

Validate that repeatable steps execute in every loop cycle, respecting max_cycles
and loop mode configuration.

### Steps

1. Review the loop engine's cycle control logic:
   ```bash
   rg -n "infinite_mode_respects_max_cycles" crates/orchestrator-scheduler/src/scheduler/loop_engine/tests.rs
   rg -n "once_mode_always_stops" crates/orchestrator-scheduler/src/scheduler/loop_engine/tests.rs
   ```

2. Review segment grouping to confirm steps are correctly scheduled per cycle:
   ```bash
   rg -n "build_segments_groups_contiguous_scopes" crates/orchestrator-scheduler/src/scheduler/loop_engine/tests.rs
   ```

3. Run the unit tests:
   ```bash
   cargo test --workspace --lib -- infinite_mode_respects_max_cycles
   cargo test --workspace --lib -- once_mode_always_stops
   cargo test --workspace --lib -- build_segments_groups_contiguous_scopes
   ```

### Expected

- `infinite_mode_respects_max_cycles` passes — confirms that infinite-mode loops
  terminate at the configured max_cycles limit
- `once_mode_always_stops` passes — confirms that once-mode loops execute exactly
  one cycle regardless of remaining work
- `build_segments_groups_contiguous_scopes` passes — confirms that steps within a
  cycle are correctly grouped into execution segments

---

## Scenario 4: Guard Step Termination

### Preconditions

- Rust toolchain available

### Goal

Validate that a guard step can terminate the workflow loop.

### Steps

1. Review how the segment builder handles guard steps:
   ```bash
   rg -n "build_segments_skips_guards" crates/orchestrator-scheduler/src/scheduler/loop_engine/tests.rs
   ```

2. Confirm that loop termination logic respects guard/stop conditions and max_cycles:
   ```bash
   rg -n "infinite_mode_respects_max_cycles" crates/orchestrator-scheduler/src/scheduler/loop_engine/tests.rs
   rg -n "once_mode_always_stops" crates/orchestrator-scheduler/src/scheduler/loop_engine/tests.rs
   ```

3. Run the unit tests:
   ```bash
   cargo test --workspace --lib -- build_segments_skips_guards
   cargo test --workspace --lib -- infinite_mode_respects_max_cycles
   cargo test --workspace --lib -- once_mode_always_stops
   ```

### Expected

- `build_segments_skips_guards` passes — confirms that guard steps are excluded from
  normal execution segments (they are processed by the loop engine's termination logic,
  not dispatched as regular agent steps)
- `infinite_mode_respects_max_cycles` passes — confirms the loop terminates at the
  configured upper bound
- `once_mode_always_stops` passes — confirms once-mode termination

> **Architecture note**: The `loop_guard` step type uses the **builtin** guard
> implementation. The builtin guard terminates based on `stop_when_no_unresolved`
> configuration and `max_cycles` limit — it does not parse agent JSON output.
> Since `stop_when_no_unresolved` defaults to `true`, the loop terminates after
> cycle 1 when all items pass.

---

## Scenario 5: Performance Selection

### Preconditions

- Rust toolchain available

### Goal

Validate that agent selection scoring accounts for cost differentials and
capability-aware strategies when choosing between agents.

### Steps

1. Review cost-based scoring logic:
   ```bash
   rg -n "test_cost_differential_lower_cost_scores_higher" core/src/selection.rs
   ```

2. Review capability-aware selection strategy:
   ```bash
   rg -n "test_selection_strategy_capability_aware" core/src/metrics.rs
   rg -n "test_selection_strategy_cost_based" core/src/metrics.rs
   ```

3. Run the unit tests:
   ```bash
   cargo test --workspace --lib -- test_cost_differential_lower_cost_scores_higher
   cargo test --workspace --lib -- test_selection_strategy_capability_aware
   cargo test --workspace --lib -- test_selection_strategy_cost_based
   ```

### Expected

- `test_cost_differential_lower_cost_scores_higher` passes — confirms that a
  lower-cost agent receives a higher score, biasing selection toward cheaper agents
  when capabilities are equivalent
- `test_selection_strategy_capability_aware` passes — confirms that the
  capability-aware strategy correctly factors capability match into scoring
- `test_selection_strategy_cost_based` passes — confirms cost-based strategy
  ordering

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Capability Isolation | | | | `test_select_agent_advanced_finds_matching_capability`, `resolve_effective_agents_returns_project_agents_when_capability_matches` |
| 2 | Multi-Agent Same Capability | | | | `test_select_agent_by_preference_random_fallback`, `test_single_candidate_deterministic` |
| 3 | Repeatable Step Execution | | | | `infinite_mode_respects_max_cycles`, `once_mode_always_stops`, `build_segments_groups_contiguous_scopes` |
| 4 | Guard Step Termination | | | | `build_segments_skips_guards`, loop termination tests |
| 5 | Performance Selection | | | | `test_cost_differential_lower_cost_scores_higher`, `test_selection_strategy_capability_aware` |
