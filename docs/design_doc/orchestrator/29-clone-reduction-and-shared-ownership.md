# Clone Reduction and Shared Ownership Governance

**Related FR**: `FR-015`  
**Related QA**: `docs/qa/orchestrator/67-clone-reduction-and-shared-ownership.md`, `docs/qa/orchestrator/68-clone-reduction-follow-up.md`

## Background And Goals

The repository does not have a blanket `clone()` problem. The meaningful risk is concentrated in hot production paths where large readonly runtime state is deep-cloned to cross async boundaries or to satisfy borrowed/owned conversion seams.

This change governs the first high-value batch of FR-015:

- reduce deep cloning of scheduler runtime context during item fan-out and graph execution
- keep daemon/proto mapping at one final ownership transfer instead of extra DTO field copies
- trim repeated owned string creation in workflow spec/config conversion branches
- move trace reconstruction to borrow-first event ordering instead of cloning full `EventDto` snapshots

The follow-up batch extends the same boundary rules into the next hotspot tier:

- remove chain-step execution's temporary `TaskRuntimeContext` clone now that pipeline state already lives in the accumulator
- keep parallel item fan-out on shared task context without a second pre-spawn pipeline-vars clone
- add owned fast-paths in `DbWriteCoordinator` so phase-runner hot paths can move `NewCommandRun` and event vectors once
- centralize export metadata and secret-key audit event assembly so manifest/audit boundaries own data once instead of rebuilding identical string bundles at each branch
- convert graph execution queue/incoming-state tracking to borrowed node ids and remove `outgoing_edges()`' transient `Vec` allocation from replay/materialization hot paths

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
- graph replay keeps owned persisted/event payloads, but execution-time queueing, incoming-edge accounting, and edge traversal now borrow graph ids until the final replay/event boundary
- repository-level owned writes below `DbWriteCoordinator` remain an explicit keep-as-owned boundary because crossing async `tokio_rusqlite` closures with borrow-heavy APIs would add more coupling than this FR justifies

## Risks And Mitigations

Risk: shared ownership hides accidental mutation assumptions.  
Mitigation: only readonly runtime fields moved behind `Arc`; mutable per-cycle state remains plain owned data.

Risk: partial optimization leaves many cold-path clones untouched.  
Mitigation: this document treats FR-015 as hotspot governance, not style cleanup. Final owned copies at proto, event, YAML, DB, and async repository boundaries are retained by design.

Risk: behavior drift in scheduler/runtime tests.  
Mitigation: add regression coverage that cloned runtime contexts share the same heavy allocations while preserving existing execution semantics.

## Observability And Operations

Repeatable observation for this FR uses lightweight regression signals rather than a new benchmark framework:

- `load_task_runtime_context_clone_shares_heavy_fields` proves runtime-context clones are shallow for the governed fields
- scheduler and graph regression tests keep execution semantics stable after ownership changes
- daemon task RPC tests continue exercising list/info/watch paths after owned-summary mapping changes
- trace regression tests continue exercising event ordering, anomaly detection, and graph replay reconstruction after borrow-first refactor
- db-write, export, and secret-key lifecycle regressions continue exercising persistence, manifest generation, and audit behavior after the follow-up borrow/owned cleanup
- loop-engine graph regressions continue exercising adaptive fallback, replay persistence, and edge evaluation after borrowed queue traversal replaced intermediate node-id copies

No operational rollout changes are required.

## Testing And Acceptance

Acceptance for FR-015 is satisfied when:

- runtime-context clone regression confirms shared `Arc` ownership for heavy fields
- workspace tests and clippy pass with the new ownership model
- daemon task/info/watch flows still serialize the same payload shape
- workflow conversion round-trip tests stay green
- trace reconstruction tests stay green after removing full-event cloning
- graph replay and fallback tests stay green after removing intermediate node-id/edge-allocation churn
- remaining owned copies are either final boundary transfers or explicit keep-as-owned decisions

Executable verification lives in `docs/qa/orchestrator/67-clone-reduction-and-shared-ownership.md`.
Follow-up hotspot verification lives in `docs/qa/orchestrator/68-clone-reduction-follow-up.md`.
