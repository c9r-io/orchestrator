# Self-Bootstrap - Binary Snapshot Unit Tests

**Module**: self-bootstrap
**Scope**: Verify snapshot_binary() and restore_binary_snapshot() functions work correctly with temp directories
**Scenarios**: 5
**Priority**: High

---

## Background

The binary snapshot functions provide a safety mechanism to backup and restore the release binary. This document tests the unit behavior of:

- `snapshot_binary(workspace_root: &Path) -> Result<PathBuf>` - copies release binary to `.stable`
- `restore_binary_snapshot(workspace_root: &Path) -> Result<()>` - restores `.stable` back to release binary path

Module: `core/src/scheduler/safety.rs`

---

## Common Preconditions

```rust
use std::path::PathBuf;
use tokio::fs;
```

All tests use temporary directories created via `tempfile::tempdir()` (or std::env::temp_dir for simpler cases).

---

## Scenario 1: snapshot_binary Creates Stable Copy

### Preconditions
- Temporary workspace directory exists
- `target/release/orchestratord` does NOT exist in workspace
- A test file at `test_binary` with known content

### Goal
Verify that snapshot_binary copies the binary to `.stable` when binary exists.

### Steps
1. Create temp workspace dir
2. Create `core/target/release/` directory structure
3. Write test content to `target/release/orchestratord`
4. Call `snapshot_binary(&workspace_root).await`
5. Verify `.stable` file exists

### Expected
- `snapshot_binary` returns `Ok(path_to_stable)`
- `.stable` file exists in workspace root
- Content matches original binary

### Expected Data State
```rust
// Verify file contents match
let original = fs::read(workspace_root.join("target/release/orchestratord")).await?;
let stable = fs::read(workspace_root.join(".stable")).await?;
assert_eq!(original, stable);
```

---

## Scenario 2: snapshot_binary Errors When Binary Missing

### Preconditions
- Temporary workspace directory exists
- No release binary in `core/target/release/`

### Goal
Verify that snapshot_binary returns an error when the release binary doesn't exist.

### Steps
1. Create temp workspace dir (empty, no binary)
2. Call `snapshot_binary(&workspace_root).await`

### Expected
- Error with message containing "release binary not found"
- No `.stable` file created

---

## Scenario 3: restore_binary_snapshot Restores Binary

### Preconditions
- Temporary workspace directory exists
- `.stable` file exists with known content

### Goal
Verify that restore_binary_snapshot copies `.stable` back to release binary path.

### Steps
1. Create temp workspace dir
2. Create `.stable` file with test content
3. Ensure `core/target/release/` directory exists
4. Call `restore_binary_snapshot(&workspace_root).await`
5. Verify binary path contains restored content

### Expected
- `restore_binary_snapshot` returns `Ok(())`
- `target/release/orchestratord` exists
- Content matches `.stable`

---

## Scenario 4: restore_binary_snapshot Errors When Stable Missing

### Preconditions
- Temporary workspace directory exists
- No `.stable` file exists

### Goal
Verify that restore_binary_snapshot returns an error when `.stable` doesn't exist.

### Steps
1. Create temp workspace dir (no `.stable` file)
2. Call `restore_binary_snapshot(&workspace_root).await`

### Expected
- Error with message containing "no .stable binary snapshot found"

---

## Scenario 5: Snapshot/Restore Cycle Preserves Content

### Preconditions
- Temporary workspace directory exists
- Release binary exists with known content

### Goal
Verify that content integrity is maintained through a full snapshot/restore cycle.

### Steps
1. Create temp workspace dir
2. Create `target/release/orchestratord` with known content: "MOCK_BINARY_v1.0"
3. Call `snapshot_binary(&workspace_root).await`
4. Modify original binary (write different content)
5. Call `restore_binary_snapshot(&workspace_root).await`
6. Read restored binary content

### Expected
- Restored binary content equals original: "MOCK_BINARY_v1.0"

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | snapshot_binary Creates Stable Copy | ✅ | 2026-03-04 | claude-sonnet-4-6 | `test_snapshot_binary_success` passes |
| 2 | snapshot_binary Errors When Binary Missing | ✅ | 2026-03-04 | claude-sonnet-4-6 | `test_snapshot_binary_missing_release` passes |
| 3 | restore_binary_snapshot Restores Binary | ✅ | 2026-03-04 | claude-sonnet-4-6 | `test_restore_binary_snapshot_success` passes (v1 compat path) |
| 4 | restore_binary_snapshot Errors When Stable Missing | ✅ | 2026-03-04 | claude-sonnet-4-6 | `test_restore_binary_snapshot_missing_stable` passes |
| 5 | Snapshot/Restore Cycle Preserves Content | ✅ | 2026-03-04 | claude-sonnet-4-6 | `test_snapshot_restore_content_integrity` passes |
