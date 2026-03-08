# Self-Bootstrap - Binary Snapshot Verification & Integration Test

**Module**: self-bootstrap
**Scope**: Verify new binary snapshot verification function and end-to-end integration test for snapshot → modify → restore → verify workflow
**Scenarios**: 5
**Priority**: High

---

## Background

The binary snapshot mechanism provides a safety layer for self-referential workspaces. This document tests the new verification function that ensures binary integrity after snapshot/restore cycles.

### New Functions Being Tested

- `verify_binary_snapshot(workspace_root: &Path) -> Result<BinaryVerificationResult>` - Verifies that the release binary matches the `.stable` snapshot
- `BinaryVerificationResult` struct - Contains verification results (verified, checksums, paths)

Module: `core/src/scheduler/safety.rs`

---

## Scenario 1: verify_binary_snapshot Returns Match When Binaries Are Identical

### Preconditions
- Temporary workspace directory exists
- Release binary exists at `target/release/orchestratord`
- `.stable` file exists with identical content

### Goal
Verify that `verify_binary_snapshot` returns a successful result indicating binaries match when no changes have been made.

### Steps
1. Create temp workspace dir
2. Create `target/release/orchestratord` with content: "BINARY_v1.0"
3. Call `snapshot_binary(&workspace_root).await`
4. Call `verify_binary_snapshot(&workspace_root).await`
5. Check the returned `BinaryVerificationResult`

### Expected
- `verify_binary_snapshot` returns `Ok(result)`
- `result.verified` is `true`
- `result.original_checksum` equals `result.current_checksum`

---

## Scenario 2: verify_binary_snapshot Detects Modified Binary

### Preconditions
- Temporary workspace directory exists
- `.stable` file exists with original content
- Release binary has been modified after snapshot

### Goal
Verify that `verify_binary_snapshot` correctly detects when the release binary differs from the `.stable` snapshot.

### Steps
1. Create temp workspace dir
2. Create `target/release/orchestratord` with content: "ORIGINAL_BINARY"
3. Call `snapshot_binary(&workspace_root).await`
4. Modify the release binary: write "MODIFIED_BINARY"
5. Call `verify_binary_snapshot(&workspace_root).await`
6. Check the returned verification result

### Expected
- `verify_binary_snapshot` returns `Ok(result)`
- `result.verified` is `false`
- `result.original_checksum` differs from `result.current_checksum`

---

## Scenario 3: Integration Test - Full Snapshot → Modify → Restore → Verify Cycle

### Preconditions
- Temporary workspace directory exists
- Release binary with known content exists

### Goal
End-to-end verification that the entire snapshot/restore workflow maintains binary integrity, and the verification function correctly reports the result.

### Steps
1. Create temp workspace dir
2. Create `target/release/orchestratord` with test content: `vec![0xDE, 0xAD, 0xBE, 0xEF]`
3. Call `snapshot_binary(&workspace_root).await` - creates `.stable`
4. Modify the release binary: write `vec![0xCA, 0xFE, 0xBA, 0xBE]`
5. Call `verify_binary_snapshot(&workspace_root).await` - should report mismatch
6. Call `restore_binary_snapshot(&workspace_root).await` - restores original
7. Call `verify_binary_snapshot(&workspace_root).await` - should report match

### Expected
- After step 3: `.stable` file exists with original content
- After step 5: `result.verified` is `false` (binary was modified)
- After step 7: `result.verified` is `true` (binary restored to original)
- Final binary content equals original: `vec![0xDE, 0xAD, 0xBE, 0xEF]`

### Expected Data State
```rust
// Step 3: After snapshot
let stable_content = fs::read(workspace_root.join(".stable")).await?;
assert_eq!(stable_content, vec![0xDE, 0xAD, 0xBE, 0xEF]);

// Step 5: After modification, verify reports mismatch
let result = verify_binary_snapshot(&workspace_root).await?;
assert!(!result.verified);

// Step 7: After restore, verify reports match
let result = verify_binary_snapshot(&workspace_root).await?;
assert!(result.verified);
```

---

## Scenario 4: verify_binary_snapshot Errors When Stable Missing

### Preconditions
- Temporary workspace directory exists
- Release binary exists at `target/release/orchestratord`
- No `.stable` file exists in workspace

### Goal
Verify that `verify_binary_snapshot` returns an error when the `.stable` snapshot file is missing.

### Steps
1. Create temp workspace dir
2. Create `target/release/orchestratord` with test content
3. Ensure no `.stable` file exists
4. Call `verify_binary_snapshot(&workspace_root).await`

### Expected
- Returns error with message containing "no .stable binary snapshot found"

---

## Scenario 5: verify_binary_snapshot Errors When Binary Missing

### Preconditions
- Temporary workspace directory exists
- No release binary exists at `target/release/orchestratord`
- `.stable` file may or may not exist

### Goal
Verify that `verify_binary_snapshot` returns an error when the release binary is missing.

### Steps
1. Create temp workspace dir (empty, no binary)
2. Call `verify_binary_snapshot(&workspace_root).await`

### Expected
- Returns error with message containing "release binary not found"

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | verify_binary_snapshot Returns Match When Binaries Are Identical | ✅ | 2026-03-04 | claude-sonnet-4-6 | test_verify_binary_snapshot_matches |
| 2 | verify_binary_snapshot Detects Modified Binary | ✅ | 2026-03-04 | claude-sonnet-4-6 | test_verify_binary_snapshot_mismatch |
| 3 | Integration Test - Full Snapshot → Modify → Restore → Verify Cycle | ✅ | 2026-03-04 | claude-sonnet-4-6 | binary_snapshot_smoke_verify_integration (integration_test.rs) |
| 4 | verify_binary_snapshot Errors When Stable Missing | ✅ | 2026-03-04 | claude-sonnet-4-6 | test_verify_binary_snapshot_missing_stable |
| 5 | verify_binary_snapshot Errors When Binary Missing | ✅ | 2026-03-04 | claude-sonnet-4-6 | test_verify_binary_snapshot_missing_binary |
