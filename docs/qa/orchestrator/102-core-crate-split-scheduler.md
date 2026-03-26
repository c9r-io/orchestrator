---
self_referential_safe: true
---

# QA 102: Core Crate Split Phase 2+3 — orchestrator-scheduler Extraction and scheduler_service Decomposition

## Scope

Verify the scheduler module extraction from core to `crates/orchestrator-scheduler/` (Phase 2) and the subsequent decomposition of `scheduler_service.rs` via trait-based port inversion (Phase 3).

All scenarios use code review and unit test verification — no `cargo build`. Compilation correctness is implicitly verified by `cargo test`.

## Verification Command

```bash
cargo test --workspace --lib
```

## Scenarios

### S-01: Compilation verification (Code Review + Implicit Verification)

**Steps**:
1. Review `crates/orchestrator-scheduler/Cargo.toml` — verify crate exists and has correct dependencies
2. Compilation of all crates is inherently verified by `cargo test --workspace --lib`

**Expected**:
- `crates/orchestrator-scheduler/` directory exists with valid Cargo.toml
- `cargo test --workspace --lib` passes — implicitly verifies all crates compile

### S-02: Test verification

| Step | Expected |
|------|---------|
| `cargo test --workspace --lib` | All pass |
| `cargo test -p orchestrator-scheduler` | All scheduler tests pass (~425 tests) |
| `cargo test -p agent-orchestrator` | All core tests pass |

### S-03: Dependency direction verification

| Check | Expected |
|-------|---------|
| `orchestrator-scheduler` Cargo.toml depends on `agent-orchestrator` | Yes |
| `agent-orchestrator` Cargo.toml **does not** depend on `orchestrator-scheduler` | Yes (no cycle) |
| daemon/integration-tests depend on both core and scheduler | Yes |
| `core/src/scheduler_port.rs` defines `TaskEnqueuer` trait | Yes (cross-crate port) |
| `InnerState.task_enqueuer` is `Arc<dyn TaskEnqueuer>` | Yes (trait object) |

### S-04: scheduler_service.rs fully decomposed

| Check | Expected |
|-------|---------|
| `core/src/scheduler_service.rs` | Deleted |
| `core/src/lib.rs` has no `pub mod scheduler_service;` | Removed |
| `enqueue_task`, `claim_next_pending_task`, `next_pending_task_id` | Moved to `crates/orchestrator-scheduler/src/service/task.rs` |
| `pending_task_count`, `worker_stop_signal_path`, `clear_worker_stop_signal`, `signal_worker_stop` | Moved to `core/src/service/system.rs` |
| `enqueue_task_as_service` | Deleted — replaced by `TaskEnqueuer` trait port |
| `SchedulerTaskEnqueuer` struct in scheduler crate | Implements `TaskEnqueuer` trait |
| `core/src/trigger_engine.rs` | Uses `state.task_enqueuer.enqueue_task()` |

### S-05: Consumer import path verification

| Consumer | Component | Import Path |
|----------|-----------|-------------|
| daemon | scheduler types | `orchestrator_scheduler::scheduler::*` |
| daemon | task service | `orchestrator_scheduler::service::task::*` |
| daemon | system checks | `orchestrator_scheduler::service::system::run_check` |
| daemon | worker stop signals | `agent_orchestrator::service::system::{clear_worker_stop_signal, worker_stop_signal_path}` |
| daemon | claim pending task | `orchestrator_scheduler::service::task::claim_next_pending_task` |
| daemon | task enqueuer | `orchestrator_scheduler::service::task::SchedulerTaskEnqueuer` |
| daemon | state init | `agent_orchestrator::service::bootstrap::init_state_async_with_enqueuer` |
| core trigger_engine | enqueue dispatch | `state.task_enqueuer.enqueue_task()` (via `TaskEnqueuer` trait) |

## Regression Risk

- If core modifies `state.rs`, `events.rs`, `db_write.rs` or other modules referenced by the scheduler, the scheduler crate needs to stay in sync
- `trigger_engine.rs` uses `cancel_task_for_trigger()` (simplified `stop_task_runtime()`); if scheduler task cancellation logic changes, this inline function needs updating
- `TaskEnqueuer` trait introduces one `dyn` dispatch on the enqueue hot path (trigger_engine only); direct calls in the scheduler crate remain zero-cost

---

## Checklist

| # | Scenario | Status | Notes |
|---|----------|--------|-------|
| 1 | S-01 Compilation | ☑ | `cargo test --workspace --lib` passed (1451+425 tests) |
| 2 | S-02 Tests | ☑ | agent-orchestrator: 1451, orchestrator-scheduler: 425 |
| 3 | S-03 Dependencies | ☑ | scheduler→core (yes), core→scheduler (no), scheduler_port.rs trait, InnerState.task_enqueuer |
| 4 | S-04 Decomposition | ☑ | scheduler_service.rs deleted; all 8 functions relocated correctly |
| 5 | S-05 Import paths | ☑ | daemon: service::system for worker stop, scheduler::service::task for claim/enqueue, init_state_async_with_enqueuer |

## Verification Summary (2026-03-26)

| Scenario | Result | Details |
|----------|--------|---------|
| S-01 | PASS | `cargo test --workspace --lib` = 1876 tests passed |
| S-02 | PASS | scheduler: 425 tests, agent-orchestrator: 1451 tests |
| S-03 | PASS | scheduler→agent-orchestrator, no reverse dep; TaskEnqueuer trait port confirmed |
| S-04 | PASS | scheduler_service.rs deleted; enqueue/claim/next in scheduler crate; worker stop/pending in service/system.rs |
| S-05 | PASS | All import paths verified in daemon (SchedulerTaskEnqueuer, claim_next_pending_task, init_state_async_with_enqueuer) |
