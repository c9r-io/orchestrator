# Async Lock Model Alignment

**Related FR**: `FR-016`  
**Related QA**: `docs/qa/orchestrator/69-async-lock-model-alignment.md`

## Background and Goals

The orchestrator runs on `tokio`, but the runtime state previously mixed async execution with `std::sync::RwLock` guards in the config and telemetry paths. That created two architectural problems:

- async scheduler/service paths could still block a runtime thread on shared-state reads
- lock implementation details leaked through helpers like `read_active_config()` and forced callers to reason about guard lifetimes instead of state semantics

FR-016 aligns the runtime state model around two explicit boundary types:

- immutable config snapshots for sync + async readers
- `tokio::sync::RwLock` for mutation-heavy async telemetry maps

Goals:

- remove `std::sync::RwLock` from async main-path config and telemetry state
- stop returning lock guards from state/config helpers
- preserve existing CLI, gRPC, manifest, and task behavior
- document the few synchronous boundaries that remain intentionally synchronous
- prevent future regressions back to blocking lock semantics in async-owned state

Non-goals:

- converting every mutex in the repository to async primitives
- introducing a new actor subsystem for metrics, health, or control-plane protection
- changing proto contracts, CLI flags, or manifest schemas
- converting the remaining documented sync exceptions into async interfaces within FR-016

## Scope

In scope:

- `core/src/state.rs` config runtime state
- `core/src/config_load/state.rs` config access helpers
- scheduler, guard, log, store, and service call sites that consumed config or telemetry guards
- agent health and agent metrics shared maps
- design and QA governance artifacts for the retained synchronous exceptions

Out of scope:

- `event_sink` replacement with an async interface
- `crates/daemon/src/protection.rs` counter/limiter internals
- a broader channel/actor redesign of health or metrics propagation

## Interfaces and Data Changes

Internal runtime-state contract changes:

- `InnerState` now stores config runtime state as `ArcSwap<ConfigRuntimeSnapshot>`
- `ConfigRuntimeSnapshot` holds:
  - `active_config: Arc<ActiveConfig>`
  - `active_config_error: Option<String>`
  - `active_config_notice: Option<ConfigSelfHealReport>`
- `read_active_config()` / `read_loaded_config()` now return config snapshots, not `RwLock*Guard`
- config mutation now goes through snapshot replacement helpers:
  - `set_config_runtime_snapshot`
  - `update_config_runtime`
  - `replace_active_config`
  - `replace_active_config_status`

Telemetry contract changes:

- `agent_health` and `agent_metrics` are now `tokio::sync::RwLock<HashMap<...>>`
- scheduler and health code no longer use generic `write_agent_*` or `read_agent_*` guard helpers
- health updates are explicit async operations (`increment_consecutive_errors`, `mark_agent_diseased`, etc.)

External behavior is unchanged:

- no gRPC wire changes
- no CLI surface changes
- no workflow/config schema changes

## Governance Rules

All new shared state in `core` must be classified before implementation:

- config-like state shared across sync + async readers: immutable snapshot, exposed as domain data
- async-hot mutable state: `tokio::sync::{Mutex,RwLock}` with short lock scopes
- ordered event flow or ownership-sensitive mutation: channel / actor style message passing
- synchronous-only bounded critical sections: explicit sync exception with documentation and tests

The repository now enforces a lightweight governance gate through `scripts/check-async-lock-governance.sh`.
That gate rejects new `std::sync::RwLock` and leaked `RwLock*Guard` usage outside the approved exception set.

Approved exceptions are intentionally narrow:

- `core/src/state.rs` `event_sink`, plus its bootstrap/test/runtime construction sites
- `crates/daemon/src/protection.rs`

Adding a new sync exception requires all of the following in the same change:

- a code comment at the retained sync boundary
- design doc update
- QA doc update
- governance-script whitelist update
- a test proving the boundary behavior still matches the documented exception

## Key Design and Tradeoffs

### Config state uses immutable snapshots instead of async locks

This was the key decoupling decision.

Why:

- config is read from both sync and async entrypoints
- most readers only need a stable snapshot, not a live mutable guard
- replacing sync `RwLock` with async `RwLock` directly would have forced broad async signature changes into service/resource/task creation paths

The chosen model makes reads cheap and lock-free from the caller perspective, while writes remain explicit snapshot replacement events.

### Telemetry maps use `tokio::sync::RwLock`

`agent_health` and `agent_metrics` are async-hot and mutation-heavy. Snapshot replacement would cause unnecessary `HashMap` cloning on every phase result. `tokio::sync::RwLock` keeps the behavior local to async paths and avoids runtime-thread blocking from synchronous `RwLock`.

### Two synchronous exceptions remain by design

- `event_sink` stays behind a synchronous lock because `emit_event()` is intentionally callable from both sync and async contexts without making the event-sink interface async
- `crates/daemon/src/protection.rs` keeps synchronous `Mutex<HashMap<...>>` counters because those critical sections are bounded, local to the protection layer, and do not cross `.await`

These are documented exceptions, not hidden leftovers.

### Governance is enforced with a repository gate, not reviewer memory

The async lock model is now a maintained invariant. Relying on code review alone would drift over time, especially in test helpers and bootstrap code where `std::sync::RwLock::new(...)` can look harmless. A lightweight static check is sufficient here because the anti-pattern is syntactic and the approved exception surface is small.

## Risks and Mitigations

Risk: snapshot replacement could drift from previous config error/notice semantics.  
Mitigation: the snapshot type carries config, error, and notice together so runnable/non-runnable state is still evaluated atomically from the caller point of view.

Risk: converting telemetry helpers to async could accidentally hold locks across `.await`.  
Mitigation: call sites only mutate/read under short local scopes; DB writes, process waits, and event publication stay outside the lock scope.

Risk: tests previously relied on `RwLock` poisoning behavior.  
Mitigation: poison-based tests were replaced by explicit snapshot/update/reset contract tests. The only retained poison recovery coverage is for the intentionally synchronous `event_sink`.

## Observability and Operations

- agent health change events are still emitted on consecutive-error, disease, and reset transitions
- config runnable vs non-runnable behavior is preserved via `active_config_error`
- no new runtime flags or migrations are required
- documented synchronous exceptions:
  - `event_sink` is a sync observability boundary
  - control-plane protection counters remain sync and should only be revisited under demonstrated contention

## Testing and Acceptance

Acceptance is satisfied when:

- async main-path config access no longer returns `std::sync::RwLock` guards
- agent health/metrics state is backed by `tokio::sync::RwLock`
- scheduler/service/store/log paths still pass existing regression tests
- the two synchronous exceptions are documented and covered by QA
- `scripts/check-async-lock-governance.sh` passes locally and in CI
- workspace verification passes:
  - `./scripts/check-async-lock-governance.sh`
  - `cargo test -p agent-orchestrator`
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo fmt --all --check`

Executable verification lives in `docs/qa/orchestrator/69-async-lock-model-alignment.md`.
