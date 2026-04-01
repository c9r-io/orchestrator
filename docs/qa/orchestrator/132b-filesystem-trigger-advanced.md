---
self_referential_safe: true
---

# QA-132b: Filesystem Trigger — Advanced

Continuation of [QA-132: Filesystem Trigger](132-filesystem-trigger.md). Covers serde roundtrip, unit test validation, config types, watcher lifecycle, and trigger engine integration.

## Scenario 6: TriggerFilesystemSpec serde roundtrip

**Steps:**
```bash
cargo test -p agent-orchestrator -- trigger_yaml_roundtrip_filesystem
```

**Expected:** Test passes — filesystem config survives YAML parse → struct → assert cycle.

## Scenario 7: Unit tests for filesystem validation

**Steps:**
```bash
cargo test -p agent-orchestrator -- trigger_validate_accepts_filesystem
cargo test -p agent-orchestrator -- trigger_validate_filesystem_requires_paths
cargo test -p agent-orchestrator -- trigger_validate_filesystem_requires_block
cargo test -p agent-orchestrator -- trigger_validate_filesystem_rejects_invalid_events
```

**Expected:** All 4 tests pass.

## Scenario 8: FsWatcher config types exist

**Steps:**
```bash
rg "TriggerFilesystemSpec" crates/orchestrator-config/src/cli_types.rs
rg "TriggerFilesystemConfig" crates/orchestrator-config/src/config/trigger.rs
```

**Expected:** Both types are defined with `paths`, `events`, `debounce_ms` fields.

## Scenario 9: FsWatcher module exists with lazy lifecycle

**Steps:**
```bash
rg "fn reload_watches" crates/daemon/src/fs_watcher.rs
rg "watcher: Option" crates/daemon/src/fs_watcher.rs
rg "no active filesystem triggers, releasing watcher" crates/daemon/src/fs_watcher.rs
```

**Expected:** All three patterns found — confirms lazy init, optional watcher, and release logic.

## Scenario 10: Trigger engine notifies fs_watcher on reload

**Steps:**
```bash
rg "fs_watcher_reload_tx" core/src/trigger_engine.rs
```

**Expected:** `notify_trigger_reload` sends to `fs_watcher_reload_tx` in addition to trigger engine.

## Checklist

- [x] Scenario 6: TriggerFilesystemSpec serde roundtrip — **PASSED** (2026-04-01)
- [x] Scenario 7: Unit tests for filesystem validation — **ALL PASSED** (2026-04-01)
  - `trigger_validate_accepts_filesystem_source` — PASSED
  - `trigger_validate_filesystem_requires_paths` — PASSED
  - `trigger_validate_filesystem_requires_block` — PASSED
  - `trigger_validate_filesystem_rejects_invalid_events` — PASSED
- [x] Scenario 8: FsWatcher config types exist — **PASSED** (2026-04-01)
  - `TriggerFilesystemSpec` at cli_types.rs:502 with `paths`, `events`, `debounce_ms` — CONFIRMED
  - `TriggerFilesystemConfig` at config/trigger.rs:65 with `paths`, `events`, `debounce_ms` — CONFIRMED
- [x] Scenario 9: FsWatcher module exists with lazy lifecycle — **PASSED** (2026-04-01)
  - `fn reload_watches` at fs_watcher.rs:107 — CONFIRMED
  - `watcher: Option` at fs_watcher.rs:40 — CONFIRMED
  - "no active filesystem triggers, releasing watcher" at fs_watcher.rs:205 — CONFIRMED
- [x] Scenario 10: Trigger engine notifies fs_watcher on reload — **PASSED** (2026-04-01)
  - `fs_watcher_reload_tx` at trigger_engine.rs:876 — CONFIRMED
