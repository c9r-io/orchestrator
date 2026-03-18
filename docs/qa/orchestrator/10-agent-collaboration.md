---
self_referential_safe: true
---

# Orchestrator - Agent Collaboration Mainline Validation

**Module**: orchestrator
**Scope**: Validate structured AgentOutput handling, phase output validation, trace/event observability, template rendering, and prehook structured fields
**Scenarios**: 5
**Priority**: High

---

## Background

This document validates collaboration-related behavior after scheduler mainline integration through code review and unit tests:

- phase output validation and normalization into `AgentOutput`
- event and trace publication for phase execution results
- capture extraction from agent output (exit code, JSON path)
- template placeholders in scheduler execution path
- structured prehook context fields availability

### Preconditions

- Rust toolchain available (`cargo` on `$PATH`)
- Repository checked out with all workspace crates

---

## Database Schema Reference

### Table: command_runs
| Column | Type | Notes |
|--------|------|-------|
| output_json | TEXT | Serialized `AgentOutput` |
| artifacts_json | TEXT | Serialized artifact list |
| confidence | REAL | Parsed confidence value |
| quality_score | REAL | Parsed quality score value |
| validation_status | TEXT | `passed` / `failed` / `unknown` |

### Table: events
| Column | Type | Notes |
|--------|------|-------|
| event_type | TEXT | Includes `output_validation_failed`, `phase_output_published`, `step_started`, `step_skipped`, `step_finished` |
| payload_json | TEXT | Event payload details |

---

## Scenario 1: Structured AgentOutput Persistence

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm AgentOutput construction and capture extraction:
   ```bash
   rg -n "struct AgentOutput" core/src/
   rg -n "fn apply_captures" core/src/
   rg -n "stdout_json_path" core/src/
   ```
   - `AgentOutput` stores confidence, quality_score, artifacts, and raw output
   - Builder methods allow incremental construction
   - Confidence is clamped to valid range
   - Capture extraction parses exit code and JSON path fields from stdout

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- test_agent_output_creation
   cargo test --workspace --lib -- test_agent_output_failure
   cargo test --workspace --lib -- test_agent_output_builder_methods
   cargo test --workspace --lib -- test_agent_output_confidence_clamped
   cargo test --workspace --lib -- apply_captures_exit_code
   cargo test --workspace --lib -- apply_captures_stdout_json_path_extracts_score
   cargo test --workspace --lib -- apply_captures_stdout_json_path_extracts_stream_json_score
   cargo test --workspace --lib -- strict_phase_accepts_json
   ```

### Expected

- `AgentOutput` can be constructed with structured fields (confidence, quality_score, artifacts)
- Builder methods set fields correctly
- Confidence values are clamped to [0.0, 1.0]
- Failure outputs are constructed with appropriate error state
- Capture extraction correctly parses exit code and JSON path fields
- Strict phases accept valid JSON output

---

## Scenario 2: Strict Phase Validation Behavior

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm output validation logic for strict vs non-strict phases:
   ```bash
   rg -n "strict_phase\|validation_status\|output_validation" core/src/
   rg -n "fn validate_output\|fn is_strict_phase" core/src/
   ```
   - Strict phases (qa, fix, retest, guard) require JSON output
   - Non-strict SDLC phases accept plain text
   - Stream JSON output is accepted for SDLC phases
   - Suffix matching requires JSON for strict phases

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- strict_phase_requires_json
   cargo test --workspace --lib -- strict_phase_suffix_match_requires_json
   cargo test --workspace --lib -- sdlc_phases_accept_plain_text_output
   cargo test --workspace --lib -- sdlc_phases_accept_stream_json_output
   ```

### Expected

- Strict phases reject non-JSON output with validation failure
- Suffix matching enforces JSON requirement for strict phases
- SDLC phases (non-strict) accept plain text output
- SDLC phases accept stream JSON output
- Validation status is set to `failed` for rejected output, `passed` for accepted output

---

## Scenario 3: MessageBus Publication Observability

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm trace and event construction for phase execution:
   ```bash
   rg -n "build_trace\|TraceEvent\|CycleTrace" core/src/
   rg -n "fn single_cycle\|fn multi_cycle\|skipped_step" core/src/
   ```
   - Phase execution results are recorded as trace events
   - Single-cycle and multi-cycle traces capture step-level detail
   - Skipped steps are recorded in the trace

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- build_trace
   cargo test --workspace --lib -- single_cycle_with_steps
   cargo test --workspace --lib -- multi_cycle_trace
   cargo test --workspace --lib -- skipped_step_recorded
   ```

### Expected

- Trace events are built with correct phase and step metadata
- Single-cycle traces capture all executed steps
- Multi-cycle traces record iterations correctly
- Skipped steps appear in the trace with skip reason

---

## Scenario 4: Scheduler Template Placeholders

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm template rendering and placeholder escaping:
   ```bash
   rg -n "pipeline_vars\|escaped_in_template\|render_template" core/src/
   rg -n "rel_path\|ticket_paths\|\\{phase\\}\|\\{task_id\\}\|\\{cycle\\}\|\\{unresolved_items\\}" core/src/
   ```
   - Template engine supports placeholders: `{rel_path}`, `{ticket_paths}`, `{phase}`, `{task_id}`, `{cycle}`, `{unresolved_items}`
   - Pipeline variables are properly escaped during rendering
   - Placeholders are replaced with concrete values before command execution

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- test_pipeline_vars_escaped_in_template
   ```

### Expected

- Pipeline variables are correctly escaped in rendered templates
- Template placeholders are replaced with concrete values (no literal `{phase}` or `{cycle}` in rendered output)

---

## Scenario 5: StepPrehookContext Structured Fields

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm prehook context definition and CEL expression evaluation:
   ```bash
   rg -n "struct StepPrehookContext\|qa_confidence\|qa_quality_score\|fix_has_changes\|upstream_artifacts" core/src/
   rg -n "fn evaluate_step_prehook\|fn prehook_cel_context" core/src/
   ```
   - `StepPrehookContext` includes structured fields: `qa_confidence`, `qa_quality_score`, `fix_has_changes`, `upstream_artifacts`
   - CEL expressions can reference these fields
   - Prehook evaluation drives step skip/execute branching

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- test_evaluate_step_prehook_expression_
   cargo test --workspace --lib -- test_prehook_cel_context_
   ```

### Expected

- Prehook context exposes structured fields for CEL evaluation
- CEL expressions evaluate without missing-field errors
- Prehook-driven branching correctly skips or executes steps based on expression results
- 150+ prehook tests pass covering various CEL expression patterns

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Structured AgentOutput Persistence | ✅ PASS | 2026-03-18 | QA | 8 tests passed (5 core + 3 scheduler) |
| 2 | Strict Phase Validation Behavior | ✅ PASS | 2026-03-18 | QA | 4 tests passed |
| 3 | MessageBus Publication Observability | ✅ PASS | 2026-03-18 | QA | 7 trace tests passed |
| 4 | Scheduler Template Placeholders | ✅ PASS | 2026-03-18 | QA | 1 template escaping test passed |
| 5 | StepPrehookContext Structured Fields | ✅ PASS | 2026-03-18 | QA | 53 prehook CEL tests passed |
