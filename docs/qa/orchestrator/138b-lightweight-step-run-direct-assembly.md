---
lifecycle: active
self_referential_safe: true
---

# Orchestrator - Lightweight Step Run Direct Assembly

**Module**: Orchestrator
**Scope**: Initial variables, synchronous run, RunStep RPC, and regression gates
**Scenarios**: 5
**Priority**: High

## Scenario 1: initial_vars Are Injected Into pipeline_vars

### Steps
1. Run `rg 'initial_vars' crates/orchestrator-scheduler/src/scheduler/runtime.rs`.

### Expected
- `initial_vars_json` is parsed and merged without overwriting existing runtime variables.

## Scenario 2: orchestrator run Is Available

### Steps
1. Run `cargo run -- run --help`.

### Expected
- Help includes `--workflow`, `--step`, `--set`, `--detach`, `--template`, `--agent-capability`, and `--profile`.

## Scenario 3: RunStep RPC Is Registered

### Steps
1. Run `rg 'RunStep' crates/proto/orchestrator.proto`.
2. Run `rg 'run_step' crates/daemon/src/server/mod.rs`.

### Expected
- The RPC exists in proto and is dispatched by the daemon.

## Scenario 4: Direct Assembly Validates Template And Capability

### Steps
1. Run `rg 'step template.*not found|no agent.*has capability' core/src/task_ops.rs`.

### Expected
- Missing templates and unsupported capabilities fail with explicit errors.

## Scenario 5: Workspace Gates Pass

### Steps
1. Run `cargo test --workspace`.
2. Run `cargo clippy --workspace --all-targets -- -D warnings`.

### Expected
- Tests pass and clippy reports no warnings.

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | initial_vars are injected into pipeline_vars | ☐ | | | |
| 2 | orchestrator run is available | ☐ | | | |
| 3 | RunStep RPC is registered | ☐ | | | |
| 4 | Direct assembly validates template and capability | ☐ | | | |
| 5 | Workspace gates pass | ☐ | | | |
