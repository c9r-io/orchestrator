---
lifecycle: active
self_referential_safe: true
---

# Orchestrator - Sandbox Readable Paths Regression

**Module**: Orchestrator
**Scope**: Environment propagation, validation, and workspace gates for FR-093
**Scenarios**: 4
**Priority**: Medium

## Scenario 1: ORCHESTRATOR_READABLE_PATHS Is Injected

### Steps
1. Run `rg 'ORCHESTRATOR_READABLE_PATHS' crates/orchestrator-scheduler/src/scheduler/phase_runner/setup.rs`.

### Expected
- `setup.rs` inserts the colon-joined value when `execution_profile.readable_paths` is non-empty.

## Scenario 2: Host Profiles Reject readable_paths

### Steps
1. Run `cargo test -p agent-orchestrator exec_profile_rejects_host_mode_with_readable_paths`.

### Expected
- The test passes with the sandbox-only fields rejection.

## Scenario 3: Full Workspace Tests Pass

### Steps
1. Run `cargo test --workspace`.

### Expected
- All workspace tests pass with zero failures.

## Scenario 4: Clippy Is Clean

### Steps
1. Run `cargo clippy --workspace --all-targets -- -D warnings`.

### Expected
- Clippy completes without warnings or errors.

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | ORCHESTRATOR_READABLE_PATHS is injected | ☐ | | | |
| 2 | Host profiles reject readable_paths | ☐ | | | |
| 3 | Full workspace tests pass | ☐ | | | |
| 4 | Clippy is clean | ☐ | | | |
