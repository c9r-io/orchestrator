# Design Doc #37: config_load Module Split and Responsibility Segregation (FR-025)

## Status

Implemented

## Context

`core/src/config_load/validate.rs` and `core/src/config_load/normalize.rs` had grown into the largest non-test source files in the repository. Validation for workflow steps, loop policy, dynamic steps, adaptive workflow, self-referential safety, and env-store references lived in one file. Workflow normalization, step behavior defaults, and config snapshot/store rebuild logic lived in another. This made local reasoning, merge conflict resolution, and targeted test maintenance unnecessarily expensive.

FR-025 required an internal-only refactor: split both modules into focused submodules, preserve the public `crate::config_load::*` API, avoid semantic behavior changes, and keep the full workspace test and lint gates green.

## Decision

Split `config_load` into focused submodules while keeping the module entrypoints stable:

### `validate`

`core/src/config_load/validate.rs` is now a thin coordinator that preserves the existing public entrypoints and delegates to:

- `validate/common.rs` for shared `AgentLookup`
- `validate/workflow_steps.rs` for step-level validation
- `validate/loop_policy.rs` for loop guard and cycle checks
- `validate/finalize_rules.rs` for finalize rule validation
- `validate/dynamic_steps.rs` for CEL trigger validation
- `validate/adaptive_workflow.rs` for adaptive planner validation
- `validate/execution_profiles.rs` for project-scoped execution profile checks
- `validate/agent_env.rs` for env store reference validation
- `validate/probe.rs` for probe-profile validation
- `validate/self_referential.rs` for self-referential workspace safety checks
- `validate/root_path.rs` for workspace path containment validation

### `normalize`

`core/src/config_load/normalize.rs` is now a thin facade that preserves the existing exports and delegates to:

- `normalize/workflow.rs` for workflow-level normalization
- `normalize/steps.rs` for step behavior defaults and recursive execution-mode normalization
- `normalize/config.rs` for whole-config normalization and CRD/store rebuild logic

Existing module tests moved into `validate/tests.rs` and `normalize/tests.rs`, keeping behavior coverage intact while removing test bulk from the production entry files.

## API Compatibility

Public behavior was preserved by keeping these entrypoints stable:

- `validate_workflow_config`
- `validate_workflow_config_for_project`
- `validate_agent_env_store_refs`
- `validate_agent_env_store_refs_for_project`
- `validate_self_referential_safety`
- `ensure_within_root`
- `normalize_workflow_config`
- `normalize_config`
- `normalize_step_execution_mode_recursive`

Call sites continue to import through `crate::config_load::*`; no external caller changes were required.

## Trade-offs

1. Small coordinator modules over deeper abstractions: the split improves locality without introducing new traits or framework layers beyond the existing `AgentLookup` helper.
2. Dedicated test files over inline tests: production files stay small, at the cost of keeping large test modules nearby as separate files.
3. Directory-local cohesion over flatter naming: grouping submodules under `validate/` and `normalize/` makes ownership boundaries explicit and avoids cluttering `config_load/` with many peer files.

## Verification

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`

Both gates passed on 2026-03-12.
