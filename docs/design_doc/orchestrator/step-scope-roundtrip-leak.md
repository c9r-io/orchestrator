# Design: Custom Step ID Scope Round-Trip Leak Fix (FR-094)

## Problem

The benchmark workflow `fixtures/benchmarks/workflow-benchmark-bootstrap.yaml`
declares an evaluation step like this:

```yaml
- id: benchmark_eval
  type: qa_testing
  scope: task
```

`qa_testing` has `scope: item` in `sdlc_conventions.yaml`, but the workflow
author explicitly overrode it to `task` because the eval step runs once per
task, not once per QA file.

In the v3 benchmark retest (2026-04-07), two of the three test groups (D1
Gemini and E1 Codex) saw their tasks materialize **180 task items** instead
of 1, with each item pointing at a real `docs/qa/**.md` file.  Both tasks
were marked `status=failed` despite the agents having actually written
correct, compiling, tested code on the first item.  The third group (C1
OpenCode) ran the same workflow and produced the expected single
synthetic-anchor item.

Root cause: a two-stage interaction between `resolved_scope()` and
`workflow_step_config_to_spec()` silently flipped the `benchmark_eval`
step's scope from Task to Item across a config↔spec round trip, then
`task_ops::resolve_task_targets` saw "the workflow has at least one
Item-scoped step" and switched to the `QaDirectoryScan` strategy, which
materializes one task item per `docs/qa/**.md` file (180 in our repo at the
time of v3 retest).  C1 escaped because its task creation happened
immediately after `apply` while the workflow was still in fresh
in-memory form; D1/E1's task creation happened after intervening
secret/agent apply cycles that triggered a config reload through the
buggy round-trip.

## Causal chain

```
YAML:        id: benchmark_eval, type: qa_testing, scope: task
   ↓ workflow_step_spec_to_config
config:      id="benchmark_eval", required_capability="qa_testing", scope=Some(Task)   ✓
   ↓ workflow_step_config_to_spec      ← stage A bug
spec':       id="benchmark_eval", step_type="qa_testing", scope=None                  ✗
   ↓ workflow_step_spec_to_config
config':     id="benchmark_eval", required_capability="qa_testing", scope=None
   ↓ TaskExecutionStep::resolved_scope()  ← stage B bug
runtime:     StepScope::Item                                                          ✗
   ↓ task_ops::execution_plan_requires_item_targets => true
   ↓ select_target_seed_strategy => QaDirectoryScan
result:      180 task items
```

### Stage A — `workflow_step_config_to_spec` dropped explicit scope

`core/src/resource/workflow/workflow_convert/mod.rs:288-298` (pre-fix):

```rust
scope: step.scope.and_then(|s| {
    let default = CONVENTIONS.default_scope(&step.id);
    if s != default {
        Some(/* "task" or "item" */)
    } else {
        None  // dropped because Task == Task (id-based default for unknown id)
    }
}),
```

This was a "harmless storage optimisation" — omit the override when it
matches the default.  In practice it broke spec↔config identity for
custom step ids: `default_scope("benchmark_eval")` returns `Task`
(unknown-id fallback), so an explicit `scope: task` gets dropped.

### Stage B — `resolved_scope` fell through to capability scope

`crates/orchestrator-config/src/config/execution.rs:101-114` (pre-fix):

```rust
pub fn resolved_scope(&self) -> StepScope {
    self.scope.unwrap_or_else(|| {
        let scope = CONVENTIONS.default_scope(&self.id);  // Task for unknown id
        if scope == StepScope::Task {
            if let Some(ref cap) = self.required_capability {
                let cap_scope = CONVENTIONS.default_scope(cap);  // Item for qa_testing
                if cap_scope == StepScope::Item {
                    return cap_scope;  // → Item
                }
            }
        }
        scope
    })
}
```

The capability fallback was designed for known SDLC step ids that piggyback
on capability-derived scope.  When applied unconditionally to unknown ids,
it lets the capability re-impose its default scope after stage A erased
the explicit declaration.

## Solution

Two precise, mutually independent fixes — either alone closes the bug, but
both are applied for defense in depth:

### Fix 1 — Scope `resolved_scope` capability fallback to known ids

`execution.rs:101-114` (post-fix):

```rust
pub fn resolved_scope(&self) -> StepScope {
    self.scope.unwrap_or_else(|| {
        let scope = CONVENTIONS.default_scope(&self.id);
        if scope == StepScope::Task && CONVENTIONS.lookup(&self.id).is_some() {
            //                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
            //                          New: only walk fallback for known ids
            if let Some(ref cap) = self.required_capability {
                let cap_scope = CONVENTIONS.default_scope(cap);
                if cap_scope == StepScope::Item {
                    return cap_scope;
                }
            }
        }
        scope
    })
}
```

Why solution B (gate by `lookup().is_some()`) instead of solution A
(remove fallback entirely):

- Preserves the design intent for known SDLC step ids (`qa`, `fix`,
  `retest`, `ticket_fix`) that legitimately want capability-derived scope
- Minimizes blast radius — only custom-id wrapper steps (e.g.
  `benchmark_eval`, `evo_benchmark`) are affected
- Audit confirmed only one production step uses `id != type` with a
  qa_testing-family capability (`fixtures/workflow/self-evolution.yaml`'s
  `evo_benchmark`), and it explicitly declares `scope: item`, so its
  behavior is unchanged

### Fix 2 — Make `workflow_step_config_to_spec` preserve all explicit scopes

`workflow_convert/mod.rs:288-298` (post-fix):

```rust
scope: step.scope.map(|s| match s {
    StepScope::Task => "task".to_string(),
    StepScope::Item => "item".to_string(),
}),
```

Why this is the right shape:

- "Drop default to save space" was nominally cheaper but introduced an
  implicit dependency on `default_scope`'s implementation; any future
  change to conventions would silently mutate every persisted workflow
- `spec ↔ config` should be a structural identity for `scope`; the
  serialized form has no compelling reason to be lossy
- Storage savings are negligible: a single string field per step

### Fix 3 — QaDirectoryScan diagnostic events (observability)

Even after fixing both stages above, future regressions in this area
should surface immediately rather than via "benchmark output looks weird".
Add two events emitted from `task_ops::create_task_impl` and
`create_run_step_task` whenever the resolver picks `QaDirectoryScan`:

| Event type | Level | When emitted | Payload |
|------------|-------|--------------|---------|
| `qa_directory_scan_triggered` | info | every time `QaDirectoryScan` materializes ≥1 item | `{trigger_step_id, trigger_capability, materialized_count, qa_targets, level: "info"}` |
| `qa_directory_scan_oversize` | warning | only when `materialized_count > 50` | `{...above..., threshold: 50, level: "warning"}` |

Both events are inserted inside the same SQL transaction that creates the
task and its items, so they commit atomically.  No hard cap is imposed —
this is observability-only, to avoid a regression that breaks legitimate
full-qa workflows.

The threshold (`QA_DIRECTORY_SCAN_OVERSIZE_THRESHOLD = 50`) is picked to
sit comfortably above realistic full-qa material counts but well below
the 180-item explosion observed in v3.

## Why C1 escaped but D1/E1 hit the bug

Same workflow, three runs, two outcomes — explained by the order of
operations between `apply` and `task create`:

- **C1**: `apply secret → apply agent → apply workflow → task create`.
  The workflow was loaded into memory from its fresh YAML parse.
  `WorkflowStepConfig.scope = Some(Task)`, no round trip happened, stage
  A never fired, `resolved_scope()` returned Task on the first call.
  → SyntheticAnchor → 1 item.
- **D1 / E1**: a sequence of `delete agent`, `apply secret` (with
  validation error then retry), `apply agent`, `task create`.  Each
  intermediate apply touched the active config, and at least one of those
  paths went through `workflow_config_to_spec` (during persistence /
  reload), triggering stage A.  By the time `task create` ran, the
  in-memory workflow had `scope = None` for `benchmark_eval`, stage B
  fired, `resolved_scope()` returned Item.  → QaDirectoryScan → 180 items.

The exact reload trigger is not load-bearing for the fix; both stages
are now closed independently.

## Files modified

- `crates/orchestrator-config/src/config/execution.rs` — `resolved_scope`
  fallback gated on `CONVENTIONS.lookup(self.id).is_some()`; +2 unit tests
- `core/src/resource/workflow/workflow_convert/mod.rs` — drop optimisation
  removed; +1 round-trip regression test in `tests.rs`
- `core/src/task_ops.rs` — `ResolvedTaskTargets::qa_directory_scan_diagnostic`,
  `QaDirectoryScanDiagnostic` struct, `first_item_scoped_step` helper,
  `emit_qa_directory_scan_events` writer, both
  `create_task_impl` / `create_run_step_task` callsites; +3 unit tests
- `core/src/task_repository/mod.rs` — re-export
  `write_ops::insert_event as insert_event_row` so `task_ops` can use it
  without depending on a private module
- `core/src/task_repository/write_ops.rs` — added missing doc comment on
  `insert_event` (workspace `#![deny(missing_docs)]` lint)
- `core/src/db_write.rs`,
  `core/src/task_repository/tests/fixtures.rs`,
  `crates/orchestrator-scheduler/src/service/task.rs` — three test
  fixtures switched to the `Explicit` strategy (passing `target_files`)
  so they no longer trigger QaDirectoryScan and pollute the events table
  that downstream tests assert against

## Test coverage

| Test | File | What it locks |
|------|------|---------------|
| `resolved_scope_does_not_leak_capability_scope_for_custom_id` | `crates/orchestrator-config/src/config/execution.rs` | Fix 1 — custom id never inherits Item from capability |
| `resolved_scope_still_uses_capability_fallback_for_known_id` | same | Positive lock — known ids unaffected |
| `workflow_explicit_scope_survives_round_trip_for_custom_step_id` | `core/src/resource/workflow/workflow_convert/tests.rs` | Fix 2 — explicit scope survives spec↔config↔spec↔config; tail asserts `resolved_scope()` returns Task |
| `create_task_with_explicit_task_scope_qa_testing_step_does_not_explode` | `core/src/task_ops.rs` | End-to-end at the create_task layer — exactly 1 item, not 180 |
| `create_task_after_workflow_config_round_trip_does_not_explode` | same | End-to-end with a deliberate `workflow_config_to_spec → workflow_spec_to_config` cycle |
| `create_task_emits_qa_directory_scan_event_when_triggered` | same | Fix 3 — diagnostic event payload + level; oversize warn correctly NOT emitted at count=1 |

All workspace tests pass and `cargo clippy --workspace --all-targets --
-D warnings` is clean.

## Related

- Triggering report: `results/benchmark-report-retest-v3.md` §6 Workflow Anomaly
- Upstream spill-path fix that revealed C1's behavior was a control case:
  FR-092 (Closed)
- Sibling sandbox-readable-paths fix from the same release:
  FR-093 (Closed)
- QA verification: `docs/qa/orchestrator/141-step-scope-roundtrip-leak.md`
