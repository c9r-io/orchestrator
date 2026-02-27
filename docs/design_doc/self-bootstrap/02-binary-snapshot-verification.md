# Self-Bootstrap - Binary Snapshot Verification Function

**Module**: self-bootstrap
**Status**: Approved
**Related Plan**: Add binary snapshot verification function using MD5 checksum comparison for integrity verification
**Related QA**: `docs/qa/self-bootstrap/03-survival-smoke-binary-snapshot-verification.md`
**Created**: 2026-02-27
**Last Updated**: 2026-02-27

## Background

The binary snapshot mechanism creates `.stable` copies of the release binary. The verification function provides runtime integrity checking to confirm the current binary matches the stored snapshot before critical operations.

## Goals

- Enable runtime verification that binary hasn't been tampered with or corrupted
- Provide checksum-based comparison (more robust than size comparison)
- Support integration test scenarios for full snapshot → modify → restore → verify lifecycle

## Non-goals

- Real-time monitoring (use watchdog for continuous checks)
- Automatic restoration on verification failure (manual or watchdog-triggered)

## Scope

- In scope: `verify_binary_snapshot()` function, `BinaryVerificationResult` return struct
- Out of scope: Auto-rollback based on verification, verification scheduling

## Key Design

### Function Signature

```rust
pub struct BinaryVerificationResult {
    pub verified: bool,
    pub original_checksum: String,  // MD5 of .stable content
    pub current_checksum: String,     // MD5 of current binary
    pub stable_path: PathBuf,
    pub binary_path: PathBuf,
}

pub async fn verify_binary_snapshot(workspace_root: &Path) -> Result<BinaryVerificationResult>
```

### Implementation Details

- Uses MD5 checksum for content comparison (via `md5` crate)
- Reads both `.stable` and current binary asynchronously
- Returns error if either file is missing
- Paths are relative to workspace root

## Alternatives And Tradeoffs

- Option A: Size comparison only
  - Pro: Faster; Con: Different content with same size passes
  - Why not: Less reliable for safety-critical verification
- Option B: SHA-256 instead of MD5
  - Pro: More secure; Con: Slightly slower, more code
  - Why not: MD5 sufficient for integrity (not security), existing code uses MD5

## Risks And Mitigations

- Risk: Large binary files cause slow verification
  - Mitigation: Use async I/O; verification is typically run infrequently
- Risk: File read errors mask actual verification
  - Mitigation: Distinct error messages for missing files vs read failures

## Observability

- Error messages include file paths for debugging
- Checksums logged for manual verification if needed

## Operations / Release

- No config changes required
- Pure function, no side effects on failure
- Returns error if `.stable` or binary missing

## Test Plan

- Unit tests: identical binaries, modified binary, missing .stable, missing binary
- Integration test: full snapshot → modify → restore → verify cycle

## QA Docs

- `docs/qa/self-bootstrap/03-survival-smoke-binary-snapshot-verification.md`

## Acceptance Criteria

- `verified: true` when checksums match
- `verified: false` when checksums differ
- Error returned when `.stable` missing
- Error returned when binary missing
- Integration test passes: snapshot → modify → verify(mismatch) → restore → verify(match)
