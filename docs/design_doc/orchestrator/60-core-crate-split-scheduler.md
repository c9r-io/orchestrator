# Design Doc 60: Core Crate Split Phase 2 — orchestrator-scheduler Extraction

## Context

After FR-047 extracted config models into `orchestrator-config`, the core crate was still ~83K LOC. The `scheduler/` module (25K+ LOC) was the single largest module. FR-048 proposed extracting it into a separate workspace member to continue reducing core complexity.

## Decision

Use an **inverted dependency** model: the new `orchestrator-scheduler` crate depends on `agent-orchestrator` (core) rather than the other way around. This avoids the trait abstraction overhead that a forward dependency would require (~25+ trait methods for persistence, events, state, etc.).

The original plan called for a `SchedulerRuntime` trait with ~18 methods and generic parameters on all scheduler functions. Dependency analysis revealed 23 cross-module dependencies, making trait abstraction impractical. The inverted dependency approach achieves the same crate isolation with zero API surface changes.

## What Moved

| Source (core) | Target (orchestrator-scheduler) |
|---|---|
| `scheduler.rs` (1,698 LOC — entry point + re-exports) | `scheduler.rs` |
| `scheduler/` (23,774 LOC — 14 submodules) | `scheduler/` |
| `service/task.rs` (479 LOC — task CRUD service) | `service/task.rs` |
| `service/system.rs` (partial — `run_check`, `RenderedCheckReport`, `diagnostic_entry_from_check`) | `service/system.rs` |

**Total moved:** ~25,951 LOC

## What Stays in Core

| Module | Reason |
|---|---|
| `runner/` (1,776 LOC) | Reverse dependency: core modules (`config_load/validate`, `dynamic_orchestration/step_pool`, `output_capture`) depend on runner |
| `prehook/` (3,061 LOC) | Reverse dependency: core modules (`config_load/validate`) depend on prehook |
| `collab/`, `anomaly.rs`, `json_extract.rs`, `output_validation.rs`, `output_capture.rs`, `dynamic_orchestration/` | Shared by both core and scheduler; kept in core to avoid type duplication across crates |
| `state.rs`, `events.rs`, `db.rs`, `db_write.rs`, `config_load/`, `task_ops.rs`, `persistence/` | Core infrastructure; scheduler accesses via `agent_orchestrator::` imports |

## Dependency Architecture

```
orchestrator-scheduler
  ├── agent-orchestrator (core)   ← inverted: scheduler depends on core
  ├── orchestrator-config
  └── orchestrator-proto

daemon / cli / integration-tests
  ├── agent-orchestrator (core)
  └── orchestrator-scheduler       ← leaf crates use both
```

## Cross-Crate Boundary Handling

Three core-internal call sites previously used `crate::service::task::*` and `crate::scheduler::*`. Since these modules moved to the scheduler crate, core-local wrappers were introduced:

| Core call site | Original path | New core-local wrapper |
|---|---|---|
| `trigger_engine.rs` | `crate::service::task::create_task` | `crate::task_ops::create_task_as_service` |
| `trigger_engine.rs` | `crate::service::task::enqueue_task` | `crate::scheduler_service::enqueue_task_as_service` |
| `trigger_engine.rs` | `crate::scheduler::stop_task_runtime` | Inline `cancel_task_for_trigger()` function |
| `service/resource.rs` | `crate::service::task::create_task` | `crate::task_ops::create_task_as_service` |

## Import Rewriting

All scheduler source files had `crate::` prefixed imports rewritten:

| Pattern | Replacement | Count |
|---|---|---|
| `crate::state`, `crate::events`, `crate::db`, etc. | `agent_orchestrator::*` | ~493 |
| `crate::runner` | `agent_orchestrator::runner` | ~13 files |
| `crate::prehook` | `agent_orchestrator::prehook` | ~5 files |
| `crate::scheduler::` (self-refs) | `crate::scheduler::` (unchanged) | — |

## Results

| Metric | Before | After |
|---|---|---|
| Core LOC | 83,123 | 57,172 |
| Core reduction | — | 31.2% |
| Scheduler crate LOC | — | ~26K |
| Workspace test count | All pass | All pass |
| Runtime overhead | — | Zero (no traits, no dyn dispatch) |

## Trade-offs

| Pro | Con |
|---|---|
| No trait abstraction needed — zero API surface changes | Scheduler depends on core (cannot compile without core) |
| All existing call patterns preserved | runner/prehook remain in core (smaller extraction scope) |
| Incremental compilation: scheduler changes only rebuild scheduler + downstream | Core changes still rebuild scheduler |
| Simple, low-risk extraction | Need core-local wrappers for 3 cross-boundary calls |
