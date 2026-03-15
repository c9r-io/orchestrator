# Design Doc 61: Prehook CEL Pipeline Variables (FR-049)

## Overview

Extends `StepPrehookContext` with pipeline variables so that prehook CEL expressions can reference values captured by earlier steps. This closes the asymmetry where `ConvergenceContext` already supported pipeline vars but `StepPrehookContext` did not.

## Motivation

In the self-bootstrap workflow, `qa_doc_gen` captures `regression_targets` to narrow the QA scope from 127 items to ~5. However, the `qa_testing` prehook CEL expression could not reference these captured variables, causing full-scope execution (~10 hours instead of ~5 minutes). Additionally, when `generate_items` post-action fails silently, there was no fallback filter.

## Design

### Data Model

`StepPrehookContext` gains a new field:

```rust
#[serde(default)]
pub vars: std::collections::HashMap<String, String>,
```

### Variable Population

`StepExecutionAccumulator::to_prehook_context()` merges task-scoped and item-scoped pipeline vars:

```rust
vars: {
    let mut merged = task_ctx.pipeline_vars.vars.clone();
    merged.extend(self.pipeline_vars.vars.iter().map(|(k, v)| (k.clone(), v.clone())));
    merged
}
```

Item-scoped vars override task-scoped vars when names collide.

### CEL Context Injection

`build_step_prehook_cel_context()` injects pipeline vars **before** built-in variables, ensuring built-ins always take precedence on name collision.

Type inference chain (same as `ConvergenceContext`):
1. JSON array (`[...]`) -> `Vec<String>` (CEL list, enables `in` operator)
2. `i64` (integer)
3. `f64` (float)
4. `bool`
5. `String` (fallback)

Truncated/spilled variables (containing `[truncated`) are skipped to avoid injecting partial data.

### Workflow Adaptation

`self-bootstrap.yaml` changes:
- `qa_doc_gen` step gains a `capture` block extracting `regression_target_ids` from stdout JSON
- `qa_testing` prehook adds `&& (size(regression_target_ids) == 0 || qa_file_path in regression_target_ids)` for list-based filtering with empty-list fallback

## Files Changed

| File | Role |
|------|------|
| `crates/orchestrator-config/src/config/execution.rs` | `StepPrehookContext.vars` field |
| `crates/orchestrator-scheduler/src/scheduler/item_executor/accumulator.rs` | Populate vars with merged task + item pipeline vars |
| `crates/orchestrator-scheduler/src/scheduler/item_executor/dispatch.rs` | Pass vars through `DynamicStepContext` alias |
| `core/src/prehook/context.rs` | Inject pipeline vars into CEL context with type inference |
| `docs/workflow/self-bootstrap.yaml` | `qa_doc_gen` capture + `qa_testing` prehook update |
| `docs/guide/04-cel-prehooks.md` | Document pipeline variable availability |

## Backward Compatibility

- `vars` has `#[serde(default)]` — existing serialized contexts deserialize without breaking
- Pipeline vars are injected before built-ins — no existing CEL expression behavior changes
- `self-bootstrap.yaml` prehook uses `size(regression_target_ids) == 0` fallback for when the variable is absent
