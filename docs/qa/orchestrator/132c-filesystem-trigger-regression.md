---
self_referential_safe: true
---

# QA-132c: Filesystem Trigger — Regression

Continuation of [QA-132: Filesystem Trigger](132-filesystem-trigger.md). Covers path safety guards and event payload format.

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

## Checklist

- [x] Scenario 11: Path safety — root_path fence and .git exclusion
- [x] Scenario 12: Event payload format
