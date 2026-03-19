---
self_referential_safe: true
---

# Orchestrator - Workflow Execution (Phases and Lifecycle)

**Module**: orchestrator
**Scope**: Validate that workflow phases execute in the correct order and lifecycle states are accurate
**Scenarios**: 5
**Priority**: High

---

## Background

This document verifies workflow execution logic through code review and unit
tests. The orchestrator supports several workflow types (`qa_only`, `qa_fix`,
`qa_fix_retest`, `loop_test`) defined in fixture files like
`echo-workflow.yaml` and `fail-workflow.yaml`. Rather than running the daemon,
each scenario inspects the underlying Rust implementation and confirms
correctness via targeted unit tests.

### Key Source Files

- **Loop engine**: `crates/orchestrator-scheduler/src/scheduler/loop_engine/tests.rs`
- **Phase runner**: `crates/orchestrator-scheduler/src/scheduler/phase_runner/tests.rs`
- **Output validation**: `core/src/output_validation.rs`
- **Prehook / finalize rules**: `core/src/prehook/tests.rs`, `core/src/dynamic_orchestration/prehook.rs`
- **Ticket creation**: `core/src/ticket.rs`
- **Health system**: `core/src/health.rs`
- **Agent selection**: `core/src/selection.rs`

### Notes

Fixture files (`echo-workflow.yaml`, `fail-workflow.yaml`) are referenced as
documentation context to describe the workflow shapes being tested. The unit
tests exercise the same code paths without requiring a running daemon.

---

## Scenario 1: qa_only Workflow

### Goal

Verify that a once-mode workflow (single QA phase, no fix/retest) terminates
after exactly one cycle, and that output validation accepts well-formed JSON.

### Preconditions

- Rust toolchain available

### Steps

1. Review the loop engine to confirm once-mode always stops after one cycle:
   ```bash
   rg -n "fn once_mode_always_stops" crates/orchestrator-scheduler/src/scheduler/loop_engine/tests.rs
   ```

2. Review output validation to confirm strict-phase JSON acceptance:
   ```bash
   rg -n "fn strict_phase_accepts_json" core/src/output_validation.rs
   rg -n "fn strict_phase_requires_json" core/src/output_validation.rs
   ```

3. Run the unit tests:
   ```bash
   cargo test --workspace --lib -- once_mode_always_stops
   cargo test --workspace --lib -- strict_phase_accepts_json
   cargo test --workspace --lib -- strict_phase_requires_json
   ```

### Expected

- `once_mode_always_stops` passes: confirms that a once-mode loop engine
  returns `should_stop = true` after one cycle, matching qa_only behavior.
- `strict_phase_accepts_json` passes: confirms valid JSON output is accepted
  by the output validation layer.
- `strict_phase_requires_json` passes: confirms non-JSON output is rejected
  when strict validation is enabled.

---

## Scenario 2: qa_fix Workflow

### Goal

Verify that a two-step workflow (QA then Fix) correctly groups contiguous
scopes into segments, and that finalize rules skip the fix phase when QA
produces no failures.

### Preconditions

- Rust toolchain available

### Steps

1. Review segment grouping logic for multi-phase workflows:
   ```bash
   rg -n "fn build_segments_groups_contiguous_scopes" crates/orchestrator-scheduler/src/scheduler/loop_engine/tests.rs
   ```

2. Review finalize rule evaluation (fix is skipped when no tickets exist):
   ```bash
   rg -n "fn test_evaluate_finalize_rule_expression_true" core/src/prehook/tests.rs
   rg -n "fn test_evaluate_finalize_rule_expression_false" core/src/prehook/tests.rs
   rg -n "fn test_evaluate_finalize_rule_fix_variables" core/src/prehook/tests.rs
   ```

3. Run the unit tests:
   ```bash
   cargo test --workspace --lib -- build_segments_groups_contiguous_scopes
   cargo test --workspace --lib -- test_evaluate_finalize_rule_expression_true
   cargo test --workspace --lib -- test_evaluate_finalize_rule_expression_false
   cargo test --workspace --lib -- test_evaluate_finalize_rule_fix_variables
   ```

### Expected

- `build_segments_groups_contiguous_scopes` passes: confirms that QA and Fix
  phases are grouped into correct segments for sequential execution.
- `test_evaluate_finalize_rule_expression_true` passes: confirms a CEL
  expression evaluating to true triggers the finalize rule.
- `test_evaluate_finalize_rule_expression_false` passes: confirms a CEL
  expression evaluating to false skips the finalize rule.
- `test_evaluate_finalize_rule_fix_variables` passes: confirms finalize rules
  have access to fix-phase variables for skip decisions.

---

## Scenario 3: qa_fix_retest Workflow

### Goal

Verify that a three-step workflow (QA, Fix, Retest) correctly segments phases
and that prehook logic can skip downstream phases when no failures occur in QA.

### Preconditions

- Rust toolchain available

### Steps

1. Review segment grouping to confirm three-phase workflows are handled:
   ```bash
   rg -n "fn build_segments_groups_contiguous_scopes" crates/orchestrator-scheduler/src/scheduler/loop_engine/tests.rs
   ```

2. Review prehook skip logic (downstream phases skipped when not needed):
   ```bash
   rg -n "fn test_prehook_decision_skip_does_not_run" core/src/dynamic_orchestration/prehook.rs
   rg -n "fn test_prehook_decision_default_is_run" core/src/dynamic_orchestration/prehook.rs
   ```

3. Review finalize rule evaluation for retest variables:
   ```bash
   rg -n "fn test_evaluate_finalize_rule_retest_variables" core/src/prehook/tests.rs
   rg -n "fn test_evaluate_finalize_rule_retest_new_ticket_count" core/src/prehook/tests.rs
   ```

4. Run the unit tests:
   ```bash
   cargo test --workspace --lib -- build_segments_groups_contiguous_scopes
   cargo test --workspace --lib -- test_prehook_decision_skip_does_not_run
   cargo test --workspace --lib -- test_prehook_decision_default_is_run
   cargo test --workspace --lib -- test_evaluate_finalize_rule_retest_variables
   cargo test --workspace --lib -- test_evaluate_finalize_rule_retest_new_ticket_count
   ```

### Expected

- `build_segments_groups_contiguous_scopes` passes: confirms three contiguous
  scopes (QA, Fix, Retest) are grouped into proper segments.
- `test_prehook_decision_skip_does_not_run` passes: confirms that a prehook
  returning Skip prevents the phase from executing.
- `test_prehook_decision_default_is_run` passes: confirms the default prehook
  decision allows phases to run.
- `test_evaluate_finalize_rule_retest_variables` passes: confirms retest-phase
  variables are available in finalize rule CEL expressions.
- `test_evaluate_finalize_rule_retest_new_ticket_count` passes: confirms the
  `new_ticket_count` variable is correctly bound for retest decisions.

---

## Scenario 4: QA Failure and Ticket Creation

### Goal

Verify that when QA fails (non-zero exit code), tickets are created correctly,
the health system degrades the failing agent, and diseased agents are filtered
from future candidate selection.

### Preconditions

- Rust toolchain available

### Steps

1. Review ticket creation on QA failure:
   ```bash
   rg -n "fn test_create_ticket_for_qa_failure" core/src/ticket.rs
   ```

2. Review health degradation logic:
   ```bash
   rg -n "fn is_agent_healthy_diseased_in_future_is_unhealthy" core/src/health.rs
   rg -n "fn is_capability_healthy_diseased_with_bad_capability_rate" core/src/health.rs
   ```

3. Review agent selection filtering of diseased agents:
   ```bash
   rg -n "fn test_diseased_agent_filtered_from_candidates" core/src/selection.rs
   ```

4. Review active ticket status detection:
   ```bash
   rg -n "fn test_is_active_ticket_status" core/src/ticket.rs
   ```

5. Run the unit tests:
   ```bash
   cargo test --workspace --lib -- test_create_ticket_for_qa_failure
   cargo test --workspace --lib -- is_agent_healthy_diseased_in_future_is_unhealthy
   cargo test --workspace --lib -- is_capability_healthy_diseased_with_bad_capability_rate
   cargo test --workspace --lib -- test_diseased_agent_filtered_from_candidates
   cargo test --workspace --lib -- test_is_active_ticket_status
   ```

### Expected

- `test_create_ticket_for_qa_failure` passes: confirms that a QA run with
  non-zero exit code produces a ticket markdown file with correct frontmatter.
- `test_create_ticket_for_qa_failure_preserves_redacted_snippets` passes:
  confirms redacted content in stdout is preserved in ticket artifacts.
- `test_create_ticket_for_qa_failure_long_stdout_truncated` passes: confirms
  excessively long stdout is truncated in ticket output.
- `is_agent_healthy_diseased_in_future_is_unhealthy` passes: confirms an
  agent with a future disease expiry is marked unhealthy.
- `is_capability_healthy_diseased_with_bad_capability_rate` passes: confirms
  a diseased agent with poor capability success rate is filtered.
- `test_diseased_agent_filtered_from_candidates` passes: confirms diseased
  agents are excluded from the candidate pool during selection.
- `test_is_active_ticket_status_*` tests pass: confirm correct classification
  of ticket statuses (open, failed = active; closed = inactive).

---

## Scenario 5: Loop Mode (max_cycles)

### Goal

Verify that the loop engine respects `max_cycles` configuration, stopping
execution when the cycle limit is reached, and that fixed-mode defaults to
one cycle.

### Preconditions

- Rust toolchain available

### Steps

1. Review infinite-mode max_cycles enforcement:
   ```bash
   rg -n "fn infinite_mode_respects_max_cycles" crates/orchestrator-scheduler/src/scheduler/loop_engine/tests.rs
   ```

2. Review fixed-mode stops at max_cycles:
   ```bash
   rg -n "fn fixed_mode_stops_at_max_cycles" crates/orchestrator-scheduler/src/scheduler/loop_engine/tests.rs
   ```

3. Review fixed-mode default cycle count:
   ```bash
   rg -n "fn fixed_mode_defaults_to_one_cycle" crates/orchestrator-scheduler/src/scheduler/loop_engine/tests.rs
   ```

4. Run the unit tests:
   ```bash
   cargo test --workspace --lib -- infinite_mode_respects_max_cycles
   cargo test --workspace --lib -- fixed_mode_stops_at_max_cycles
   cargo test --workspace --lib -- fixed_mode_defaults_to_one_cycle
   ```

### Expected

- `infinite_mode_respects_max_cycles` passes: confirms that an infinite-mode
  loop terminates when `current_cycle >= max_cycles`.
- `fixed_mode_stops_at_max_cycles` passes: confirms that a fixed-mode loop
  stops at the configured max_cycles boundary.
- `fixed_mode_defaults_to_one_cycle` passes: confirms that when no max_cycles
  is specified, fixed mode defaults to a single cycle.

---

## Checklist

| # | Scenario | Status | Date | Tester | Notes |
|---|----------|--------|------|--------|-------|
| 1 | qa_only Workflow | PASS | 2026-03-19 | QA | `once_mode_always_stops`, output validation tests |
| 2 | qa_fix Workflow | PASS | 2026-03-19 | QA | `build_segments_groups_contiguous_scopes`, finalize rule tests |
| 3 | qa_fix_retest Workflow | PASS | 2026-03-19 | QA | Segment grouping, prehook skip logic tests |
| 4 | QA Failure and Ticket Creation | PASS | 2026-03-19 | QA | 5 ticket tests + 2 health tests + 1 selection filter + 7 ticket status tests |
| 5 | Loop Mode (max_cycles) | PASS | 2026-03-19 | QA | `infinite_mode_respects_max_cycles`, `fixed_mode_stops_at_max_cycles`, `fixed_mode_defaults_to_one_cycle` |
