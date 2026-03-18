---
self_referential_safe: true
---

# QA #76: config_load Module Split and Responsibility Segregation (FR-025)

## Scope

Verify that the `config_load` refactor remains an internal-only reorganization: public APIs stay intact, validation and normalization behavior do not regress, and the split modules stay below the FR soft limits for production code.

## Scenarios

### S-01: `validate` is split into focused production modules

**Steps**:
1. Run `wc -l core/src/config_load/validate.rs core/src/config_load/validate/*.rs`
2. Exclude `tests.rs` from the soft-limit check
3. Confirm there are at least 4 production submodules under `core/src/config_load/validate/`

**Expected**:
- `validate.rs` is a thin entry module
- At least 4 production submodules exist
- Each production submodule remains under the 500-line soft limit

### S-02: `normalize` is split into focused production modules

**Steps**:
1. Run `wc -l core/src/config_load/normalize.rs core/src/config_load/normalize/*.rs`
2. Exclude `tests.rs` from the soft-limit check
3. Confirm there are at least 3 production submodules under `core/src/config_load/normalize/`

**Expected**:
- `normalize.rs` is a thin entry module
- At least 3 production submodules exist
- Each production submodule remains under the 500-line soft limit

### S-03: Public API remains stable

**Steps**:
1. Search the repo for `crate::config_load::validate_workflow_config`, `normalize_workflow_config`, `normalize_config`, `validate_agent_env_store_refs`, and `ensure_within_root`
2. Confirm call sites compile without edits outside the refactored module tree

**Expected**:
- Existing call sites still import through `crate::config_load::*`
- No user-visible CLI or API changes are required

### S-04: Validation and normalization regression coverage stays green

**Steps**:
1. Code review confirms test modules exist: `core/src/config_load/validate/tests.rs` (65+ tests) and `core/src/config_load/normalize/tests.rs`
2. Run `cargo test --lib -p agent-orchestrator -- config_load` (safe: lib-only, does not affect running daemon)
3. Verify all config_load tests pass

**Expected**:
- All existing validation and normalization tests pass
- No behavior regression is introduced by the module split

### S-05: Lint gate stays clean

**Steps**:
1. Code review confirms no new `#[allow]` annotations in `core/src/config_load/` diff
2. CI enforces `cargo clippy --workspace --all-targets -- -D warnings` on every push
3. Verify `.github/workflows/ci.yml` contains clippy job with `-D warnings`

**Expected**:
- No new warnings or lint suppressions are required
- CI gate confirms clippy compliance

## Result

Verified on 2026-03-12:

- `validate` production modules: 10 submodules, all under 500 lines
- `normalize` production modules: 3 submodules, all under 500 lines
- `cargo test --workspace`: passed
- `cargo clippy --workspace --all-targets -- -D warnings`: passed

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☑ | S-01–S-05 PASS (2026-03-19); S-04/S-05 rewritten as safe (code review + cargo test --lib + CI gate) |
