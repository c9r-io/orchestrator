# Design Doc 92: Daemon Configuration Hot Reload

## FR Reference

FR-086

## Problem

After `orchestrator apply -f <manifest>`, the running daemon must reflect new resources (triggers, workflows, agents) in its in-memory state without requiring a restart.

## Design Decision: ArcSwap Atomic Config Snapshot

The daemon already implements hot reload via an **ArcSwap-based atomic config snapshot** mechanism. No new code was required.

### Architecture

```
CLI apply
  -> gRPC ApplyRequest
    -> apply_manifests()                  [core/src/service/resource.rs:16]
      -> persist_config_and_reload()      [core/src/config_load/persist.rs:93]
        -> SQLite persist
        -> set_config_runtime_snapshot()  [core/src/state.rs:184]
           (ArcSwap::store — atomic, lock-free)
      -> notify_trigger_reload()          [core/src/trigger_engine.rs:869]
           (mpsc signal to trigger engine for cron schedule rebuild)
    -> gRPC ApplyResponse (config_version included)
```

### Key Components

1. **`config_runtime: ArcSwap<ConfigRuntimeSnapshot>`** (`core/src/state.rs:71`)
   - Central in-memory config cache shared across all daemon subsystems
   - `ArcSwap::store()` is atomic and lock-free — readers never block
   - Any `ArcSwap::load()` after a `store()` sees the new value (memory ordering guaranteed)

2. **`persist_config_and_reload()`** (`core/src/config_load/persist.rs:93-121`)
   - Called synchronously during `apply_manifests()`
   - Persists to SQLite, then atomically swaps `config_runtime`
   - Returns before the gRPC response is sent to the CLI

3. **`notify_trigger_reload()`** (`core/src/trigger_engine.rs:869-881`)
   - Sends async reload event to trigger engine via `mpsc::Sender`
   - Trigger engine rebuilds cron schedule from updated `config_runtime`
   - Also notifies filesystem watcher to re-evaluate watched paths

### Webhook Direct-Fire Path

The webhook handler (`crates/daemon/src/webhook.rs`) reads config via `read_active_config()` which loads from the ArcSwap. This path does **not** go through the trigger engine and is **not** subject to stabilization delay.

```
HTTP POST /webhook/{trigger_name}
  -> read_active_config()     [webhook.rs:76]  — reads ArcSwap
  -> fire_trigger()           [webhook.rs:147] — reads ArcSwap again
  -> create task + return 200
```

### Trigger Stabilization (Cron/Event Only)

The trigger engine applies a one-cycle stabilization delay to newly applied triggers (`trigger_engine.rs:192-209`). This is intentional: it prevents agent-applied triggers from immediately spawning parasitic tasks. This delay only affects:
- Cron-scheduled triggers
- Event-driven triggers (task_completed, task_failed)

It does **not** affect:
- Webhook direct-fire (`fire_trigger()`)
- CLI `trigger fire` command

## Acceptance Criteria Mapping

| Criterion | How Met |
|-----------|---------|
| AC1: config_runtime reflects changes within 5s | ArcSwap update is synchronous (< 1ms), happens before gRPC response |
| AC2: apply returns daemon acknowledgment | `config_version` in ApplyResponse; connection failure = daemon not running |
| AC3: No task disruption during reload | ArcSwap is lock-free; readers hold Arc to old snapshot until done |

## Files

- `core/src/state.rs` — `InnerState.config_runtime`, `set_config_runtime_snapshot()`
- `core/src/config_load/persist.rs` — `persist_config_and_reload()`
- `core/src/service/resource.rs` — `apply_manifests()`, `fire_trigger()`
- `core/src/trigger_engine.rs` — `TriggerEngine`, `notify_trigger_reload()`
- `crates/daemon/src/webhook.rs` — HTTP webhook handler
