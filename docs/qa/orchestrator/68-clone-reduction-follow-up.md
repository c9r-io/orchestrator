# Clone Reduction Follow-Up

**Scope**: Verify FR-015 follow-up clone reduction on chain-step execution, graph replay/materialization, item fan-out, db-write owned fast-paths, manifest export helpers, and secret-key audit assembly.

## Scenarios

1. Run scheduler chain-step and fan-out regressions:

   ```bash
   cargo test -p agent-orchestrator scheduler::item_executor::tests -- --nocapture
   cargo test -p agent-orchestrator scheduler::loop_engine::tests -- --nocapture
   ```

   Expected:

   - chain steps still execute in order without cloning a temporary task context per child step
   - item fan-out still preserves per-item accumulator state and task-scoped pipeline propagation

2. Run graph materialization and replay regressions:

   ```bash
   cargo test -p agent-orchestrator scheduler::loop_engine::tests -- --nocapture
   cargo test -p agent-orchestrator scheduler::trace::tests -- --nocapture
   ```

   Expected:

   - adaptive fallback, node replay, and edge evaluation remain stable
   - borrowed node-id queueing and iterator-based edge traversal do not change replay or event output

3. Run db-write ownership regressions:

   ```bash
   cargo test -p agent-orchestrator db_write::tests -- --nocapture
   ```

   Expected:

   - owned `NewCommandRun` and event vectors persist correctly through `DbWriteCoordinator`
   - command-run updates, phase-result persistence, and event promotion remain unchanged

4. Run export and secret-key lifecycle regressions:

   ```bash
   cargo test -p agent-orchestrator resource::export::tests -- --nocapture
   cargo test -p agent-orchestrator secret_key_lifecycle::tests -- --nocapture
   ```

   Expected:

   - manifest export remains deterministic after shared metadata helpers
   - key rotation, revoke, and audit history behavior remain unchanged after audit-event helper cleanup

5. Run workspace verification:

   ```bash
   cargo test --workspace
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo fmt --all --check
   ```

   Expected:

   - no scheduler, persistence, or secret-store regressions across the workspace
   - lint and formatting checks stay green after the follow-up ownership cleanup

## Failure Notes

- If chain-step execution regresses, inspect `core/src/scheduler/item_executor/dispatch.rs`
- If graph replay or materialization regresses, inspect `core/src/scheduler/loop_engine/graph.rs` and `core/src/dynamic_orchestration/graph.rs`
- If item fan-out state propagation regresses, inspect `core/src/scheduler/loop_engine/segment.rs`
- If command-run/event persistence regresses, inspect `core/src/db_write.rs` and `core/src/scheduler/phase_runner/{setup,record}.rs`
- If manifest export or secret-key audit output regresses, inspect `core/src/resource/export.rs` and `core/src/secret_key_lifecycle.rs`

## Checklist

| # | Scenario | Status | Notes |
|---|----------|--------|-------|
| 1 | Scheduler chain-step and fan-out regressions | ✅ | Chain children now reuse the live task context and parallel fan-out avoids a redundant pipeline-vars clone |
| 2 | Graph materialization and replay regressions | ✅ | Borrowed node-id queueing and iterator-based edge traversal preserve replay and trace behavior |
| 3 | Db-write ownership regressions | ✅ | `DbWriteCoordinator` owned fast-paths preserve command-run and event persistence behavior |
| 4 | Export and secret-key lifecycle regressions | ✅ | Shared metadata and audit builders keep manifest and key lifecycle output stable |
| 5 | Workspace verification | ✅ | Workspace tests, clippy, and fmt remain green after the follow-up cleanup |
