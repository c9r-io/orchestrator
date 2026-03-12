# Clone Reduction and Shared Ownership Governance

**Related FR**: `FR-015`  
**Related QA**: `docs/qa/orchestrator/67-clone-reduction-and-shared-ownership.md`

## Background And Goals

The repository does not have a blanket `clone()` problem. The meaningful risk is concentrated in hot production paths where large readonly runtime state is deep-cloned to cross async boundaries or to satisfy borrowed/owned conversion seams.

This change governs the first high-value batch of FR-015:

- reduce deep cloning of scheduler runtime context during item fan-out and graph execution
- keep daemon/proto mapping at one final ownership transfer instead of extra DTO field copies
- trim repeated owned string creation in workflow spec/config conversion branches
- move trace reconstruction to borrow-first event ordering instead of cloning full `EventDto` snapshots

Non-goals:

- zero `clone()` across the repository
- public API or protobuf schema changes
- invasive lifetime-heavy refactors

## Scope And Interfaces

Internal interfaces changed only inside Rust implementation boundaries:

- `TaskRuntimeContext` now keeps large readonly fields behind `Arc`
  - `execution_plan`
  - `dynamic_steps`
  - `adaptive`
  - `safety`
- scheduler helpers read those fields by shared reference and continue to mutate only local execution state such as `pipeline_vars`, `current_cycle`, and accumulator state
- daemon summary mapping now consumes owned `TaskSummary` at the transport boundary
- builtin step execution now consumes an explicit pipeline-vars view instead of cloning the full task context in the generic builtin path
- trace builder/anomaly detection now operate on sorted borrowed event references

External behavior is unchanged:

- no CLI or gRPC wire changes
- no workflow/config schema changes

## Key Design And Tradeoffs

The chosen design favors shallow shared ownership over broad borrow-based rewrites.

Why:

- scheduler already needs `'static` ownership for parallel item execution
- `Arc` on a few heavy readonly fields removes repeated deep clones without changing most call signatures
- mutable runtime state remains explicit and local, so shared ownership does not blur write semantics

Specific decisions:

- `TaskRuntimeContext` is still an owned struct; only heavy readonly members are shared
- daemon mapping is optimized only to the boundary where owned DTOs are already available; proto messages still require one final owned copy
- workflow conversion keeps the same public shapes and only centralizes builtin-step classification helpers to avoid repeated branch-time string cloning
- trace reconstruction keeps owned trace output, but internal ordering/anomaly passes no longer duplicate the full input event list

## Risks And Mitigations

Risk: shared ownership hides accidental mutation assumptions.  
Mitigation: only readonly runtime fields moved behind `Arc`; mutable per-cycle state remains plain owned data.

Risk: partial optimization leaves many cold-path clones untouched.  
Mitigation: this document treats FR-015 as hotspot governance, not style cleanup. Remaining clones stay out of scope until backed by evidence.

Risk: behavior drift in scheduler/runtime tests.  
Mitigation: add regression coverage that cloned runtime contexts share the same heavy allocations while preserving existing execution semantics.

## Observability And Operations

Repeatable observation for this FR uses lightweight regression signals rather than a new benchmark framework:

- `load_task_runtime_context_clone_shares_heavy_fields` proves runtime-context clones are shallow for the governed fields
- scheduler and graph regression tests keep execution semantics stable after ownership changes
- daemon task RPC tests continue exercising list/info/watch paths after owned-summary mapping changes
- trace regression tests continue exercising event ordering, anomaly detection, and graph replay reconstruction after borrow-first refactor

No operational rollout changes are required.

## Testing And Acceptance

Acceptance for this batch is satisfied when:

- runtime-context clone regression confirms shared `Arc` ownership for heavy fields
- workspace tests and clippy pass with the new ownership model
- daemon task/info/watch flows still serialize the same payload shape
- workflow conversion round-trip tests stay green
- trace reconstruction tests stay green after removing full-event cloning

Executable verification lives in `docs/qa/orchestrator/67-clone-reduction-and-shared-ownership.md`.
