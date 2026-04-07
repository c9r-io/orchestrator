---
self_referential_safe: true
---

# QA-141: Step Scope Round-Trip Leak (FR-094)

**Module**: Orchestrator
**Scope**: `TaskExecutionStep::resolved_scope` capability fallback;
`workflow_step_config_to_spec` scope serialization;
`task_ops::resolve_task_targets` `QaDirectoryScan` diagnostics
**Scenarios**: 6
**Priority**: High

---

## Background

The benchmark workflow declares its evaluation step as
`id: benchmark_eval, type: qa_testing, scope: task` — a custom step id
wrapping an Item-default capability with an explicit Task override.
Before FR-094, two interacting bugs could silently flip the resolved
scope from Task to Item across a config↔spec round trip, causing
`task_ops::resolve_task_targets` to switch from `SyntheticAnchor` (1
item) to `QaDirectoryScan` (one item per `docs/qa/**.md`, 180 in our
repo at the time).  This produced the v3 benchmark explosion where
D1 (Gemini) and E1 (Codex) both materialized 180 items and got
`status=failed` despite having actually written correct code.

The fix has three pieces:

1. `resolved_scope` capability fallback now only fires for step ids
   registered in `sdlc_conventions.yaml`
2. `workflow_step_config_to_spec` no longer drops explicit scope when it
   matches the id-based default
3. `task_ops` emits `qa_directory_scan_triggered` (info) and
   `qa_directory_scan_oversize` (warning, threshold=50) diagnostic events
   so future regressions surface immediately

Key files:
- `crates/orchestrator-config/src/config/execution.rs` — `resolved_scope`
- `core/src/resource/workflow/workflow_convert/mod.rs` — round-trip
- `core/src/task_ops.rs` — diagnostics + e2e regressions

---

## Scenario 1: `resolved_scope` does not leak capability scope for custom id

### Preconditions
- `TaskExecutionStep` with `id="benchmark_eval"` (not registered in
  `sdlc_conventions.yaml`), `required_capability=Some("qa_testing")`
  (registered with `scope: item`), `scope=None`

### Goal
Verify that a custom step id whose capability defaults to Item still
resolves to Task — i.e. the capability fallback does not bleed through
for unknown ids.

### Steps
1. Construct the step described above
2. Call `step.resolved_scope()`

### Expected
- Returns `StepScope::Task`

### Verification
```bash
cargo test -p orchestrator-config \
    -- config::execution::tests::resolved_scope_does_not_leak_capability_scope_for_custom_id
```

---

## Scenario 2: Known SDLC step id still uses capability-derived scope

### Preconditions
- `TaskExecutionStep` with `id="qa"` (registered with `scope: item`),
  `required_capability=Some("qa")`, `scope=None`

### Goal
Positive lock: Scenario 1's narrowing must not over-restrict — known
ids continue to honor their convention scope.

### Steps
1. Construct the step
2. Call `step.resolved_scope()`

### Expected
- Returns `StepScope::Item` (from the id-based convention, no fallback
  needed)

### Verification
```bash
cargo test -p orchestrator-config \
    -- config::execution::tests::resolved_scope_still_uses_capability_fallback_for_known_id
```

---

## Scenario 3: Explicit scope survives full config↔spec round trip

### Preconditions
- `WorkflowStepSpec` with `id="benchmark_eval"`, `step_type="qa_testing"`,
  `scope=Some("task")`

### Goal
Verify that the explicit `scope: task` is preserved across
`workflow_step_spec_to_config → workflow_config_to_spec`, and that the
final `TaskExecutionStep::resolved_scope()` returns `Task`.

### Steps
1. Run `workflow_step_spec_to_config(&original)` → confirm
   `config.scope == Some(StepScope::Task)`
2. Wrap in a `WorkflowConfig` and call `workflow_config_to_spec(&wf)`
3. Inspect the resulting `WorkflowStepSpec.scope` — must be
   `Some("task")`
4. Convert back to `WorkflowStepConfig` and build a `TaskExecutionStep`
5. Call `resolved_scope()`

### Expected
- Step 1: `Some(Task)`
- Step 3: `Some("task")` (NOT `None`)
- Step 5: `StepScope::Task`

### Verification
```bash
cargo test -p agent-orchestrator --lib \
    -- resource::workflow::workflow_convert::tests::workflow_explicit_scope_survives_round_trip_for_custom_step_id
```

---

## Scenario 4: `create_task_impl` with explicit-task benchmark_eval step does not explode

### Preconditions
- `TestState` with default `echo` agent (`capabilities=["qa"]`)
- A workflow named `benchmark_eval_only` whose only step is
  `make_step("benchmark_eval", None, Some("qa"))` with
  `scope = Some(StepScope::Task)`
- No `target_files` in the create payload (forcing seed selection by
  scope-based strategy)

### Goal
End-to-end dry-run equivalent of "rerun the D1 benchmark":
verify that `create_task_impl` materializes exactly **one** synthetic
anchor item, not 180.

### Steps
1. Build the workflow + register it via `TestState::with_workflow`
2. Call `create_task_impl` with the empty payload
3. Read back `total_items` and inspect `task_items.qa_file_path`

### Expected
- `result.total_items == 1`
- The single item has `qa_file_path == "__UNASSIGNED__"`
- `target_files_json` is empty (the synthetic-anchor branch persists no
  target files)

### Verification
```bash
cargo test -p agent-orchestrator --lib \
    -- task_ops::tests::create_task_with_explicit_task_scope_qa_testing_step_does_not_explode
```

---

## Scenario 5: `create_task_impl` after a deliberate workflow_config_to_spec round trip

### Preconditions
- Same workflow shape as Scenario 4
- The workflow is first put through
  `workflow_config_to_spec → workflow_spec_to_config` to simulate the
  daemon's reload path

### Goal
Strongest end-to-end assertion that Fix 2 is in place: even after the
workflow has been serialized to spec and reparsed, task creation must
still yield 1 item.

### Steps
1. Build `benchmark_workflow()` (helper from `task_ops::tests`)
2. `workflow_config_to_spec` → `workflow_spec_to_config` round trip
3. Register the round-tripped config under a different workflow name
4. Call `create_task_impl`

### Expected
- `result.total_items == 1`

### Verification
```bash
cargo test -p agent-orchestrator --lib \
    -- task_ops::tests::create_task_after_workflow_config_round_trip_does_not_explode
```

---

## Scenario 6: QaDirectoryScan emits a diagnostic event

### Preconditions
- `TestState` with a workflow whose only step is a real `qa` step (Item
  scope by convention)
- One QA file seeded under `docs/qa/scenario.md`
- No `target_files` in payload (so `QaDirectoryScan` is selected)

### Goal
Verify that when the resolver legitimately picks `QaDirectoryScan`, the
task creator emits a `qa_directory_scan_triggered` event whose payload
identifies the trigger step id and item count.  Below the oversize
threshold (50), the `qa_directory_scan_oversize` warning must NOT be
emitted.

### Steps
1. Register the qa-only workflow
2. Seed a single QA file
3. Call `create_task_impl`
4. Query the `events` table for `event_type='qa_directory_scan_triggered'`
   on the new task id
5. Parse the JSON payload
6. Query for `event_type='qa_directory_scan_oversize'` (must be 0 rows)

### Expected
- Exactly one `qa_directory_scan_triggered` row exists
- Payload contains `trigger_step_id="qa"`, `materialized_count=1`,
  `level="info"`
- Zero `qa_directory_scan_oversize` rows (count is below the threshold)

### Verification
```bash
cargo test -p agent-orchestrator --lib \
    -- task_ops::tests::create_task_emits_qa_directory_scan_event_when_triggered
```

---

## Workspace-level verification

After all six scenarios pass individually, the full workspace test suite
and clippy lint must also stay green:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Both commands have been verified to pass with the FR-094 changes
applied.

## Related

- FR-094 design: `docs/design_doc/orchestrator/step-scope-roundtrip-leak.md`
- Triggering report: `results/benchmark-report-retest-v3.md` §6
- Upstream FR-092 (Closed) — `artifacts_dir` workspace path
- Sibling FR-093 (Closed) — sandbox readable paths
