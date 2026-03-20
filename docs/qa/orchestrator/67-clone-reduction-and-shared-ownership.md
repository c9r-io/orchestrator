---
self_referential_safe: true
---

# Clone Reduction and Shared Ownership

**Scope**: Verify FR-015 clone reduction on scheduler runtime context sharing, daemon owned-summary mapping, workflow conversion, generic builtin execution, and trace reconstruction regressions.

## Self-Referential Safety

This document is safe for self-referential full-QA runs. Verification is limited to unit tests,
code review, and workspace gates; no live task queue or daemon lifecycle interaction is required.

## Scenarios

1. Run runtime-context sharing regression:

   ```bash
   cargo test -p orchestrator-scheduler scheduler::runtime::tests::load_task_runtime_context_clone_shares_heavy_fields -- --nocapture
   ```

   Expected:

   - cloned `TaskRuntimeContext` instances share `execution_plan`, `dynamic_steps`, `adaptive`, `safety`, and `pinned_invariants`
   - no behavior change in runtime-context loading

2. Run scheduler and graph ownership regressions:

   ```bash
   cargo test -p orchestrator-scheduler scheduler::loop_engine::tests -- --nocapture
   ```

   Expected:

   - scope segmentation, graph execution, and item execution tests continue to pass
   - adaptive-planner and parallel-item paths work with shared runtime context fields

3. Run workflow conversion regressions:

   ```bash
   cargo test -p agent-orchestrator resource::workflow::workflow_convert -- --nocapture
   ```

   Expected:

   - workflow spec/config round-trips remain stable
   - builtin-step detection and safety-field round-trips remain unchanged

4. Run daemon and workspace verification:

   ```bash
   cargo test -p orchestratord server::tests -- --nocapture
   cargo test --workspace --lib
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   ```

   Expected:

   - daemon server tests still pass with owned-summary transport mapping
   - full workspace tests and clippy pass

5. Run trace reconstruction regressions:

   ```bash
   cargo test -p orchestrator-scheduler scheduler::trace::tests -- --nocapture
   ```

   Expected:

   - task trace ordering, anomaly detection, and graph replay reconstruction remain stable
   - borrow-first event sorting does not change rendered trace content

## Failure Notes

- If runtime-context sharing regresses, inspect `core/src/config/execution.rs` and `core/src/scheduler/runtime.rs`
- If item fan-out or graph execution starts deep-cloning runtime state again, inspect `core/src/scheduler/loop_engine/segment.rs` and `core/src/scheduler/loop_engine/graph.rs`
- If task list/info/watch responses regress, inspect `crates/daemon/src/server/mapping.rs` and `crates/daemon/src/server/task.rs`
- If trace output or anomaly detection regresses, inspect `core/src/scheduler/trace/{builder,anomaly,time}.rs`

## Checklist

| # | Scenario | Status | Notes |
|---|----------|--------|-------|
| 1 | Runtime-context sharing regression | ✅ | `TaskRuntimeContext` clone now shallow-shares heavy readonly fields |
| 2 | Scheduler and graph ownership regressions | ✅ | Segment, item executor, and dynamic DAG tests validate unchanged behavior |
| 3 | Workflow conversion regressions | ✅ | Round-trip tests cover builtin/safety/config conversion stability |
| 4 | Daemon and workspace verification | ✅ | Server tests, workspace tests, and clippy remain green |
| 5 | Trace reconstruction regressions | ✅ | Borrow-first event ordering preserves trace and anomaly behavior |
