---
self_referential_safe: true
---

# QA-132: Filesystem Trigger

Validates FR-085: native filesystem change detection as a trigger source, with lazy watcher lifecycle, path safety, and CEL filter integration.

## Scenario 1: Compilation and tests

**Steps:**
```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

**Expected:** All tests pass, no clippy warnings.

## Scenario 2: `source: filesystem` accepted in manifest validation

**Steps:**
```bash
cat <<'YAML' | orchestrator manifest validate -f -
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: fs-test
spec:
  event:
    source: filesystem
    filesystem:
      paths:
        - docs/
      events:
        - create
      debounce_ms: 500
  action:
    workflow: test-wf
    workspace: default
YAML
```

**Expected:** Validation passes (no `event.source` error).

## Scenario 3: filesystem trigger requires filesystem block

**Steps:**
```bash
cat <<'YAML' | orchestrator manifest validate -f -
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: bad-fs
spec:
  event:
    source: filesystem
  action:
    workflow: wf
    workspace: ws
YAML
```

**Expected:** Validation fails with "requires a 'filesystem' configuration block".

## Scenario 4: filesystem trigger rejects empty paths

**Steps:**
```bash
cat <<'YAML' | orchestrator manifest validate -f -
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: bad-fs
spec:
  event:
    source: filesystem
    filesystem:
      paths: []
  action:
    workflow: wf
    workspace: ws
YAML
```

**Expected:** Validation fails with "paths must not be empty".

## Scenario 5: filesystem trigger rejects invalid event types

**Steps:**
```bash
cat <<'YAML' | orchestrator manifest validate -f -
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: bad-fs
spec:
  event:
    source: filesystem
    filesystem:
      paths:
        - src/
      events:
        - rename
  action:
    workflow: wf
    workspace: ws
YAML
```

**Expected:** Validation fails with "filesystem.events must be one of".

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

## Scenario 11: Path safety — root_path fence and .git exclusion

**Steps:**
```bash
rg "outside root_path" crates/daemon/src/fs_watcher.rs
rg "skipping .git path" crates/daemon/src/fs_watcher.rs
rg "skipping daemon data directory" crates/daemon/src/fs_watcher.rs
```

**Expected:** All three safety checks present in the watcher reload logic.

## Scenario 12: Event payload format

**Steps:**
```bash
rg '"path":|"filename":|"dir":|"event_type":|"timestamp":' crates/daemon/src/fs_watcher.rs
```

**Expected:** Payload JSON includes all five fields: path, filename, dir, event_type, timestamp.
