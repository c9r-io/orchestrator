---
self_referential_safe: false
self_referential_safe_scenarios: [S4, S5, S6]
---

# QA 101: Core Crate Split Phase 1 — orchestrator-config

Verifies FR-047: extraction of config models into `crates/orchestrator-config`.

## Verification Scenarios

### S-01: Independent Compilation

```bash
cargo build -p orchestrator-config
```

**Expected**: Compiles with zero errors. No dependency on tokio, rusqlite, or async-trait.

### S-02: Full Workspace Build

```bash
cargo build --workspace
```

**Expected**: All crates (orchestrator-config, agent-orchestrator, orchestrator-cli, orchestratord, orchestrator-integration-tests) build without errors.

### S-03: Full Workspace Tests

```bash
cargo test --workspace
```

**Expected**: All unit tests, integration tests, and doctests pass.

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

### S-07: Extension Traits Functional

- `ResourceStoreExt::project_map()` and `project_singleton()` work via trait import
- `OrchestratorConfigExt::runtime_policy()` returns correct `RuntimePolicyProjection`
- `DynamicStepConfigExt::matches()` evaluates CEL triggers correctly

**Verified by**: Existing unit tests in `core/src/crd/store.rs`, `core/src/resource/runtime_policy.rs`, and `core/src/dynamic_orchestration/step_pool.rs` all pass.

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
