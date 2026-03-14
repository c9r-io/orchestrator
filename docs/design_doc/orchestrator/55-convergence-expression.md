# Design Doc 55: loop_guard convergence expression (FR-043)

## Overview

Adds a `convergence_expr` field to the workflow `loop` configuration, allowing users to define custom CEL-based convergence conditions that terminate loop execution when satisfied.

## Motivation

The loop_guard builtin previously only supported `stop_when_no_unresolved` and `max_cycles` for termination decisions. Real-world evolution workflows need fine-grained convergence semantics such as "stop when code diff is small" or "stop when benchmark plateau is reached".

## Design

### YAML Surface

```yaml
loop:
  mode: infinite
  max_cycles: 20
  convergence_expr:
    - engine: cel
      when: "delta_lines < 5 && cycle >= 2"
      reason: "code diff converged"
    - engine: cel
      when: "active_ticket_count == 0 && self_test_passed"
      reason: "all tickets resolved"
```

### Data Model

- `ConvergenceExprEntry` (config struct): `engine: StepHookEngine`, `when: String`, `reason: Option<String>`
- `ConvergenceExprSpec` (CRD spec): string-based equivalent for YAML serialization
- `ConvergenceContext`: lightweight CEL context struct with `cycle`, `active_ticket_count`, `self_test_passed`, `max_cycles`, and user-defined `vars` (from pipeline captures)

### CEL Context Variables

Framework-provided: `cycle`, `active_ticket_count`, `self_test_passed`, `max_cycles`.
User-provided via step captures: any key in `pipeline_vars.vars` is injected as a top-level CEL variable with automatic type coercion (i64 > f64 > bool > string).

### Evaluation Points

1. **Builtin loop_guard** (`guard.rs`): evaluated after `stop_when_no_unresolved` check, before returning GuardResult. Triggers `workflow_terminated` event path.
2. **Loop continuation** (`continuation.rs`): evaluated after mode-level rules (Once/Fixed/Infinite/max_cycles). Triggers `loop_guard_decision` event with convergence reason.

Both paths use the same `evaluate_convergence_expression()` function from the prehook CEL module.

### Validation

CEL expressions are compiled at config load time in `validate_loop_policy()`. Invalid expressions fail fast with descriptive error messages.

### Backward Compatibility

- `convergence_expr` is `Option<Vec<_>>` with `serde(default)` — omitting it preserves existing behavior exactly.
- `max_cycles` remains the hard safety cap regardless of convergence expressions.
- No changes to non-loop_guard step behavior.

## Files Changed

| File | Role |
|------|------|
| `config/workflow.rs` | `ConvergenceExprEntry` struct, `WorkflowLoopConfig.convergence_expr` field |
| `cli_types.rs` | `ConvergenceExprSpec` for CRD layer |
| `resource/workflow/workflow_convert.rs` | Bidirectional spec<->config conversion |
| `config_load/validate/loop_policy.rs` | CEL compilation validation |
| `config/execution.rs` | `ConvergenceContext` struct |
| `prehook/cel.rs` | `evaluate_convergence_expression()` |
| `prehook/context.rs` | `build_convergence_cel_context()` |
| `prehook/mod.rs` | Re-export |
| `scheduler/item_executor/guard.rs` | Builtin loop_guard convergence check |
| `scheduler/loop_engine/continuation.rs` | Loop continuation convergence check |
