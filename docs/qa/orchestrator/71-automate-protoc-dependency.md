# QA #71: Automate protoc Dependency (FR-020)

## Scope

Verify that the workspace builds successfully both with and without a system `protoc`, and that the `PROTOC` environment variable override works correctly.

## Scenarios

### S-01: Build without PROTOC env var

**Steps**:
1. Unset `PROTOC` environment variable
2. Run `cargo check -p orchestrator-proto`

**Expected**: Build succeeds. Cargo output includes warning `Using protobuf-src protoc at ...`

### S-02: Build with explicit PROTOC env var

**Steps**:
1. Set `PROTOC` to a valid protoc binary path (e.g., `/opt/homebrew/bin/protoc` or `/usr/bin/protoc`)
2. Run `cargo check -p orchestrator-proto`

**Expected**: Build succeeds. No `protobuf-src` warning in output.

### S-03: PROTOC pointing to non-existent path

**Steps**:
1. Set `PROTOC=/nonexistent/protoc`
2. Run `cargo check -p orchestrator-proto`

**Expected**: Build falls back to `protobuf-src` and succeeds. Warning shows protobuf-src path.

### S-04: Full workspace build

**Steps**:
1. Run `cargo build --workspace`

**Expected**: All crates compile successfully.

### S-05: Clippy clean

**Steps**:
1. Run `cargo clippy -p orchestrator-proto --all-targets -- -D warnings`

**Expected**: No warnings or errors.

### S-06: CI workflow PROTOC propagation

**Steps**:
1. Review `.github/workflows/ci.yml`
2. Verify clippy job passes `PROTOC: /usr/bin/protoc` env
3. Verify test job passes `PROTOC: /usr/bin/protoc` env
4. Verify cross-compile job detects protoc path dynamically

**Expected**: All CI jobs have explicit PROTOC env var set to avoid protobuf-src compilation.

## Result

All scenarios verified locally on 2026-03-12.

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
