---
self_referential_safe: true
---

# Orchestrator - Degenerate Cycle Loop Guard Verification

**Module**: orchestrator
**Scope**: Validate rapid cycle detection (L2), trace anomaly reporting, blocked item recovery, and unit-tested circuit breaker (L1)
**Scenarios**: 5
**Priority**: High

---

## Background

This document validates the degenerate cycle detection and circuit breaker mechanism (FR-035). The feature provides layered defense against tasks that enter rapid-fire failing cycles:

- **L1**: Per-item per-step circuit breaker — blocks individual items after `max_item_step_failures` consecutive failures, with exponential backoff (30s/120s) before blocking.
- **L2**: Rapid cycle detection — pauses the entire task when the last 3 inter-cycle intervals are all below `min_cycle_interval_secs`.
- **Trace**: `DegenerateLoop` anomaly is emitted when command_runs show 3+ consecutive failures for the same item-phase pair.

Design doc: `docs/design_doc/orchestrator/12-degenerate-cycle-loop-guard.md`

---

## Scenario 1: Rapid Cycle Detection (L2) — Loop Mode and Max Cycles

### Goal

Verify that loop engine correctly enforces max cycle limits for different loop modes (fixed, infinite, once).

### Preconditions

- Rust toolchain available
- Unit tests available in loop_engine/tests.rs

### Steps

1. Run the max cycles unit tests:
   ```bash
   cargo test --workspace --lib proactive_max_cycles_fixed_mode
   cargo test --workspace --lib proactive_max_cycles_fixed_mode_default
   cargo test --workspace --lib proactive_max_cycles_infinite_mode_with_cap
   cargo test --workspace --lib proactive_max_cycles_infinite_mode_no_cap
   cargo test --workspace --lib proactive_max_cycles_once_mode_passthrough
   ```

2. Run the loop mode stop condition tests:
   ```bash
   cargo test --workspace --lib fixed_mode_stops_at_max_cycles
   cargo test --workspace --lib fixed_mode_defaults_to_one_cycle
   cargo test --workspace --lib once_mode_always_stops
   cargo test --workspace --lib infinite_mode_respects_max_cycles
   cargo test --workspace --lib infinite_mode_with_disabled_guard_continues_immediately
   ```

### Expected Results

- Fixed mode stops at configured max_cycles (default: 1)
- Once mode always stops after single cycle
- Infinite mode respects max_cycles cap when set
- Loop guard disabled mode continues immediately without delay

---

## Scenario 2: Trace Anomaly — DegenerateLoop Detection

### Goal

Verify that the `detect_degenerate_loop` trace detector correctly identifies item-phase pairs with 3+ consecutive command_run failures.

### Preconditions

- Unit tests available

### Steps

1. Run the degenerate loop anomaly unit tests:
   ```bash
   cargo test --workspace --lib degenerate_loop_emits_anomaly_on_three_consecutive_failures
   cargo test --workspace --lib degenerate_loop_no_anomaly_when_fewer_than_three_consecutive_failures
   cargo test --workspace --lib degenerate_loop_no_anomaly_when_failures_are_non_consecutive
   ```

2. Verify anomaly rule metadata:
   ```bash
   cargo test --workspace --lib canonical_name_roundtrip
   cargo test --workspace --lib severity_mapping
   cargo test --workspace --lib escalation_mapping
   ```

### Expected Results

- 3+ consecutive exit-1 runs for the same item-phase pair triggers `degenerate_loop` anomaly
- Fewer than 3 failures produce no anomaly
- Interrupted failure streak (success between failures) produces no anomaly
- `DegenerateLoop` maps to `Error` severity and `Intervene` escalation

---

## Scenario 3: Segment Building and Scope Classification

### Goal

Verify that the loop engine correctly builds execution segments from workflow steps, grouping contiguous scopes and handling guard/disabled steps.

### Preconditions

- Unit tests available

### Steps

1. Run the segment building unit tests:
   ```bash
   cargo test --workspace --lib build_segments_groups_contiguous_scopes
   cargo test --workspace --lib build_segments_skips_guards
   cargo test --workspace --lib build_segments_skips_disabled_steps
   cargo test --workspace --lib build_segments_empty_when_no_steps
   cargo test --workspace --lib build_segments_item_select_is_task_scoped
   ```

2. Run the scope resolution tests:
   ```bash
   cargo test --workspace --lib resolved_scope_uses_explicit_override
   cargo test --workspace --lib last_item_segment_detected_when_no_later_item_segments_exist
   cargo test --workspace --lib last_item_segment_rejects_item_segment_with_later_item_work_remaining
   ```

### Expected Results

- Contiguous same-scope steps are grouped into segments
- Guard steps (loop_guard) are excluded from segments
- Disabled steps are skipped
- Explicit scope override takes precedence over default scope
- Last-item-segment detection is correct for terminal state propagation

---

## Scenario 4: Auto-Rollback and Snapshot Safety

### Goal

Verify the auto-rollback circuit breaker logic and binary snapshot safety guards.

### Preconditions

- Unit tests available

### Steps

1. Run the auto-rollback unit tests:
   ```bash
   cargo test --workspace --lib should_auto_rollback_true_when_all_conditions_met
   cargo test --workspace --lib should_auto_rollback_false_when_disabled
   cargo test --workspace --lib should_auto_rollback_false_when_below_threshold
   cargo test --workspace --lib should_auto_rollback_false_when_no_checkpoint_strategy
   cargo test --workspace --lib should_auto_rollback_true_when_failures_exceed_threshold
   ```

2. Run the snapshot safety tests:
   ```bash
   cargo test --workspace --lib should_snapshot_true_when_both_enabled
   cargo test --workspace --lib should_snapshot_false_when_not_self_referential
   cargo test --workspace --lib should_snapshot_false_when_binary_snapshot_disabled
   cargo test --workspace --lib should_snapshot_false_when_both_disabled
   ```

3. Run the rollback tag computation tests:
   ```bash
   cargo test --workspace --lib compute_rollback_tag_normal
   cargo test --workspace --lib compute_rollback_tag_zero_failures
   cargo test --workspace --lib compute_rollback_tag_saturates_to_one
   cargo test --workspace --lib compute_rollback_tag_exact_cycle_one
   ```

### Expected Results

- Auto-rollback triggers only when all conditions met (enabled, threshold exceeded, checkpoint strategy exists)
- Snapshot guard requires both self-referential mode and binary snapshot enabled
- Rollback tag computation handles edge cases (zero failures, saturation)

---

## Scenario 5: FR-035 Config Serde and Safety Fields

### Goal

Verify that all FR-035 unit tests pass: safety config serde, anomaly rule definitions, and field serialization.

### Preconditions

- Rust toolchain available, project builds successfully

### Steps

1. Run FR-035 config serde tests:
   ```bash
   cargo test --workspace --lib test_fr035_fields_serde_round_trip
   cargo test --workspace --lib test_fr035_fields_explicit_json_deserialization
   ```

2. Run the terminal state propagation tests:
   ```bash
   cargo test --workspace --lib propagate_task_segment_terminal_state_marks_all_items_terminal
   cargo test --workspace --lib propagate_task_segment_terminal_state_no_execution_failed_flag
   cargo test --workspace --lib propagate_preserves_existing_item_state
   ```

3. Run pipeline var propagation tests:
   ```bash
   cargo test --workspace --lib collect_item_eval_states_maps_pipeline_vars
   cargo test --workspace --lib promote_winner_vars_inserts_into_pipeline
   ```

4. Run invariant check edge case:
   ```bash
   cargo test --workspace --lib check_invariants_returns_none_for_empty_invariants
   ```

### Expected Results

- `max_item_step_failures` and `min_cycle_interval_secs` serialize/deserialize correctly
- Explicit JSON values override defaults
- Terminal state propagation marks all items terminal
- Pipeline var promotion inserts winner vars correctly
- Empty invariant list returns None (no blocking)

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | Rapid Cycle Detection (L2) — Loop Mode and Max Cycles | ☐ | Unit test — safe |
| 2 | Trace Anomaly — DegenerateLoop Detection | ☐ | Unit test — safe |
| 3 | Segment Building and Scope Classification | ☐ | Unit test — safe |
| 4 | Auto-Rollback and Snapshot Safety | ☐ | Unit test — safe |
| 5 | FR-035 Config Serde and Safety Fields | ☐ | Unit test — safe |
