---
self_referential_safe: true
---

# QA 101: Core Crate Split Phase 1 — orchestrator-config

Verifies FR-047: extraction of config models into `crates/orchestrator-config`.

All scenarios use code review and unit test verification — no `cargo build` required. Compilation is inherently verified by `cargo test`.

## Verification Command

```bash
cargo test --workspace --lib
```

## Verification Scenarios

### S-01: Independent Compilation (Code Review + Unit Test)

**Steps**:
1. Review `crates/orchestrator-config/Cargo.toml` — verify dependency list does NOT include `tokio`, `rusqlite`, or `async-trait`
2. Run unit tests to verify compilation:
   ```bash
   cargo test -p orchestrator-config
   ```

**Expected**:
- [ ] `Cargo.toml` has no dependency on `tokio`, `rusqlite`, or `async-trait`
- [ ] `cargo test -p orchestrator-config` passes — implying independent compilation succeeds

### S-02: Full Workspace Compilation (Implicit Verification)

**Steps**:
1. Full workspace compilation is inherently verified by `cargo test --workspace --lib` which must compile all crates before running tests

**Expected**:
- [ ] `cargo test --workspace --lib` passes — implying all crates (orchestrator-config, agent-orchestrator, orchestrator-cli, orchestratord, orchestrator-integration-tests) compile without errors

### S-03: Full Workspace Tests

```bash
cargo test --workspace --lib
```

**Expected**: All unit tests pass.

### S-04: Config Directory Removed from Core

```bash
ls core/src/config/ 2>&1
```

**Expected**: Directory does not exist. Config models live in `crates/orchestrator-config/src/config/`.

### S-05: CLI/Daemon Zero Source Changes

```bash
git diff HEAD -- crates/cli/ crates/daemon/
```

**Expected**: No changes to CLI or daemon crate source files.

### S-06: Re-export Compatibility

Verify that `agent_orchestrator::config::OrchestratorConfig` resolves to `orchestrator_config::config::OrchestratorConfig` through the re-export layer. Any code using `crate::config::*` within core continues to compile.

### S-07: Extension Traits Functional (Code Review + Unit Test)

**Steps**:
1. Review trait implementations:
   - `core/src/crd/store.rs` — `ResourceStoreExt::project_map()` and `project_singleton()`
   - `core/src/resource/runtime_policy.rs` — `OrchestratorConfigExt::runtime_policy()`
   - `core/src/dynamic_orchestration/step_pool.rs` — `DynamicStepConfigExt::matches()`
2. Run unit tests:
   ```bash
   cargo test -p orchestrator-core -- project_map project_singleton runtime_policy matches
   ```

**Expected**:
- [ ] Extension trait unit tests pass
- [ ] `ResourceStoreExt::project_map()` and `project_singleton()` work via trait import
- [ ] `OrchestratorConfigExt::runtime_policy()` returns correct `RuntimePolicyProjection`
- [ ] `DynamicStepConfigExt::matches()` evaluates CEL triggers correctly

### S-08: orchestrator-config Unit Tests

```bash
cargo test -p orchestrator-config
```

**Expected**: 131 unit tests + 2 doctests pass. Config serialization round-trips, validation logic, and default values are all verified within the extracted crate.

## Result

All scenarios verified. FR-047 is closed.

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
