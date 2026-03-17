---
self_referential_safe: false
---

# Async Lock Model Alignment

**Scope**: Verify FR-016 runtime-state alignment for config snapshots, async telemetry locks, and the two retained synchronous exceptions.

## Scenarios

1. Run state and config runtime contract tests:

   ```bash
   cargo test -p agent-orchestrator state::tests config_load::state::tests -- --nocapture
   ```

   Expected:

   - config reads use snapshot helpers instead of `std::sync::RwLock` guards
   - non-runnable config still returns the stored error
   - config snapshot replacement works without direct guard access

2. Run health and phase-runner regressions:

   ```bash
   cargo test -p agent-orchestrator health::tests scheduler::phase_runner::tests -- --nocapture
   cargo test -p agent-orchestrator scheduler::item_executor::tests -- --nocapture
   ```

   Expected:

   - agent health and metrics updates still preserve disease, consecutive error, and capability scoring semantics
   - guard/phase-runner selection and metrics load accounting remain stable after moving to async locks

3. Run scheduler/runtime and store/log regressions:

   ```bash
   cargo test -p agent-orchestrator scheduler::runtime::tests service::store::tests scheduler::query::log_stream::tests -- --nocapture
   ```

   Expected:

   - runtime context loading still reads current config correctly
   - store execution and prune paths still resolve custom resources from active config
   - log streaming still derives redaction patterns from the loaded config snapshot

4. Verify documented synchronous exceptions remain deliberate:

   ```bash
   cargo test -p agent-orchestrator state::tests::poisoned_event_sink_recovers_with_tracing_sink -- --nocapture
   cargo test -p orchestratord protection::tests -- --nocapture
   ```

   Expected:

   - `event_sink` remains the only retained poison-recovery state path in `core`
   - control-plane protection counters and limits still work with the existing synchronous limiter implementation

5. Run the governance gate:

   ```bash
   ./scripts/check-async-lock-governance.sh
   ```

   Expected:

   - the script passes without reporting new `std::sync::RwLock` usage in `core`
   - only the documented `event_sink` and `protection` exceptions remain on the whitelist
   - no `RwLockReadGuard` / `RwLockWriteGuard` helper leakage is reported

6. Run workspace verification:

   ```bash
   ./scripts/check-async-lock-governance.sh
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   cargo fmt --all --check
   ```

   Expected:

   - no regressions across scheduler, service, daemon, or CLI paths
   - no warning-cleanliness regressions after the async lock migration

## Failure Notes

- If config snapshot reads fail, inspect `core/src/state.rs` and `core/src/config_load/state.rs`
- If telemetry behavior regresses, inspect `core/src/health.rs` and `core/src/scheduler/phase_runner/record.rs`
- If store or log paths regress, inspect `core/src/service/store.rs`, `core/src/scheduler/item_executor/apply.rs`, and `core/src/scheduler/query/log_stream.rs`
- If control-plane protection regresses, inspect `crates/daemon/src/protection.rs`
- If the governance gate fails, inspect `scripts/check-async-lock-governance.sh` and the reported sync-lock call sites

## Checklist

| # | Scenario | Status | Notes |
|---|----------|--------|-------|
| 1 | State and config runtime contract tests | ☐ | |
| 2 | Health and phase-runner regressions | ☐ | |
| 3 | Scheduler/runtime and store/log regressions | ☐ | |
| 4 | Documented synchronous exceptions remain deliberate | ☐ | |
| 5 | Governance gate | ☐ | |
| 6 | Workspace verification | ☐ | |
