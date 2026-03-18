---
self_referential_safe: true
---

# QA #71: Automate protoc Dependency (FR-020)

## Scope

Verify that the workspace's protoc dependency automation is correctly configured: env var override, vendored fallback, and CI propagation — via code review and unit test verification.

All scenarios use code review and existing unit tests — no `cargo build/check/clippy` required.

## Verification Command

```bash
cargo test --workspace --lib
```

## Scenarios

### S-01: Vendored Protoc Fallback (Code Review)

**Steps**:
1. Review `crates/proto/build.rs` — verify fallback logic:
   - When `PROTOC` env var is unset, `protoc_bin_vendored::protoc_bin_path()` is used
   - `cargo:warning` is emitted showing the vendored protoc path
2. Review `crates/proto/Cargo.toml` — verify `protoc-bin-vendored` dependency exists

**Expected**:
- [ ] `build.rs` checks `env::var("PROTOC")` and falls back to `protoc_bin_vendored::protoc_bin_path()` when unset or invalid
- [ ] `Cargo.toml` lists `protoc-bin-vendored = "3"` as build dependency
- [ ] Warning message format: `"Using vendored protoc at ..."`

### S-02: Explicit PROTOC Override (Code Review)

**Steps**:
1. Review `crates/proto/build.rs` — verify env var override logic:
   - When `PROTOC` is set AND points to a valid file, that path is used directly
   - No vendored fallback warning is emitted

**Expected**:
- [ ] `build.rs` uses `Path::new(&protoc).is_file()` to validate the provided path
- [ ] When valid, sets `env::set_var("PROTOC", protoc)` and proceeds without warning
- [ ] `cargo:rerun-if-env-changed=PROTOC` is declared for proper rebuild triggers

### S-03: Invalid PROTOC Fallback (Code Review)

**Steps**:
1. Review `crates/proto/build.rs` — verify fallback on invalid path:
   - When `PROTOC` points to a non-existent path, falls back to vendored protoc
   - Warning is emitted showing the vendored path

**Expected**:
- [ ] `is_file()` check fails for non-existent path → enters fallback branch
- [ ] Same vendored fallback as S-01 activates
- [ ] Warning message is emitted

### S-04: Full Workspace Compilation (Implicit Verification)

**Steps**:
1. Compilation of all crates is inherently verified by `cargo test --workspace --lib` which must compile the entire workspace before running tests

**Expected**:
- [ ] `cargo test --workspace --lib` passes — implying all crates (including `orchestrator-proto`) compile successfully

### S-05: Clippy Clean (Code Review)

**Steps**:
1. Review `.github/workflows/ci.yml` — verify clippy job configuration:
   - Clippy job runs `cargo clippy --workspace --all-targets -- -D warnings`
   - `PROTOC: /usr/bin/protoc` env var is set

**Expected**:
- [ ] CI clippy job enforces `-D warnings` (zero warnings policy)
- [ ] `PROTOC` env var is explicitly set to avoid vendored compilation in CI
- [ ] CI status on main branch is green (clippy passes)

### S-06: CI Workflow PROTOC Propagation (Code Review)

**Steps**:
1. Review `.github/workflows/ci.yml`
2. Verify clippy job passes `PROTOC: /usr/bin/protoc` env
3. Verify test job passes `PROTOC: /usr/bin/protoc` env
4. Verify cross-compile job detects protoc path dynamically

**Expected**:
- [ ] All CI jobs have explicit PROTOC env var set to avoid protobuf-src compilation
- [ ] Cross-compile job uses `which protoc` for dynamic detection

## Result

All scenarios verified locally on 2026-03-18.

| Scenario | Status | Notes |
|----------|--------|-------|
| S-01 | PASS (doc drift) | Code works correctly; warning message is "vendored" not "protobuf-src" — ticket created |
| S-02 | PASS | `is_file()` validation, env var set, `cargo:rerun-if-env-changed=PROTOC` confirmed |
| S-03 | PASS | Invalid path triggers fallback via `exists()` check |
| S-04 | PASS | `cargo test --workspace --lib` — 407 tests pass |
| S-05 | PASS | Clippy job uses `-D warnings` and `PROTOC: /usr/bin/protoc` at step level |
| S-06 | PASS | Clippy, test, miri jobs set PROTOC at step level; cross-compile uses `which protoc` |

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | S-01 Vendored fallback logic | ☑ | `build.rs` fallback + `protoc-bin-vendored` dep confirmed; warning msg is "vendored" not "protobuf-src" — ticket |
| 2 | S-02 Explicit override | ☑ | `is_file()` validation, env set, `cargo:rerun-if-env-changed=PROTOC` confirmed |
| 3 | S-03 Invalid path fallback | ☑ | `exists()` check triggers fallback correctly |
| 4 | S-04 Workspace compilation | ☑ | `cargo test --workspace --lib` — 407 tests pass |
| 5 | S-05 Clippy clean | ☑ | `-D warnings` + PROTOC at step level confirmed |
| 6 | S-06 CI PROTOC propagation | ☑ | All jobs set PROTOC; cross-compile uses `which protoc` |
