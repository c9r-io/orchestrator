# Design Doc 59: Core Crate Split Phase 1 — orchestrator-config Extraction

## Context

The `core` crate (`agent-orchestrator`) was a monolith of ~90K LOC. FR-047 proposed extracting config models and loading into a separate workspace member to improve compilation granularity and code organization.

## Decision

Extract pure data-model modules into `crates/orchestrator-config` while keeping runtime-coupled code (config_load, CRD projection, CEL evaluation) in core. Use `pub use` re-exports and extension traits to maintain backward-compatible paths.

## What Moved

| Source (core) | Target (orchestrator-config) |
|---|---|
| `config/` (25 files) | `config/` |
| `cli_types.rs` | `cli_types.rs` |
| `env_resolve.rs` | `env_resolve.rs` |
| `crd/types.rs` data structs | `crd_types.rs` |
| `crd/scope.rs` | `crd_scope.rs` |
| `crd/store.rs` (ResourceStore, ApplyResult) | `resource_store.rs` |
| `metrics.rs` (SelectionStrategy, SelectionWeights) | `selection.rs` |
| `dynamic_orchestration/adaptive.rs` (AdaptivePlannerConfig, AdaptiveFallbackMode) | `adaptive.rs` |
| `dynamic_orchestration/step_pool.rs` (DynamicStepConfig struct) | `dynamic_step.rs` |

## What Stays in Core

| Module | Reason |
|---|---|
| `config_load/` (build, persist, state, normalize, validate) | Deep dependencies on db, persistence, dto, prehook, CEL, self_referential_policy |
| `crd/projection.rs` (CrdProjectable trait + impls) | Depends on resource converters |
| `ResourceStore::project_map()`/`project_singleton()` | Depends on CrdProjectable — moved to `ResourceStoreExt` trait |
| `OrchestratorConfig::runtime_policy()` | Depends on CrdProjectable — moved to `OrchestratorConfigExt` trait |
| `DynamicStepConfig::matches()` | Depends on CEL prehook evaluation — moved to `DynamicStepConfigExt` trait |

## Compatibility Layer

### Re-exports in core/src/lib.rs

```rust
pub use orchestrator_config::config;
pub use orchestrator_config::cli_types;
pub use orchestrator_config::env_resolve;
```

All existing `crate::config::*` paths continue to resolve through these re-exports. CLI and daemon crates required zero source changes.

### Extension Traits

Three extension traits keep runtime-dependent methods available:

1. **`ResourceStoreExt`** (`core/src/crd/store.rs`) — adds `project_map<T>()` and `project_singleton<T>()` requiring `CrdProjectable`
2. **`OrchestratorConfigExt`** (`core/src/config_ext.rs`) — adds `runtime_policy()` returning `RuntimePolicyProjection`
3. **`DynamicStepConfigExt`** (`core/src/dynamic_orchestration/step_pool.rs`) — adds `matches()` requiring CEL trigger evaluation

### Facade Modules

Core retains thin facade modules that re-export from orchestrator-config:

- `crd/types.rs` → `pub use orchestrator_config::crd_types::*;`
- `crd/scope.rs` → `pub use orchestrator_config::crd_scope::*;`
- `crd/store.rs` → `pub use orchestrator_config::resource_store::*;` + `ResourceStoreExt`
- `resource/mod.rs` → `pub use orchestrator_config::resource_store::ApplyResult;`
- `metrics.rs` → `pub use orchestrator_config::selection::{SelectionStrategy, SelectionWeights};`
- `dynamic_orchestration/adaptive.rs` → `pub use orchestrator_config::adaptive::{…};`
- `dynamic_orchestration/step_pool.rs` → `pub use orchestrator_config::dynamic_step::DynamicStepConfig;`

## Dependencies

`orchestrator-config` has minimal dependencies (no tokio, rusqlite, async-trait):

- anyhow, chrono (serde), glob, serde (derive), serde_json, serde_yml, tracing, uuid (serde, v4), walkdir

## Trade-offs

- **config_load not extracted**: Originally planned but dependency analysis revealed deep coupling with db, persistence, CEL, and prehook modules. Extracting would require a much larger refactor with diminishing returns.
- **Extension traits add import friction**: Callers of `runtime_policy()`, `project_map()`, etc. must import the extension trait. This is a small cost for clean crate boundaries.
- **Orphan rule workaround**: Cannot add inherent `impl` blocks to types defined in orchestrator-config when used in core; extension traits are the idiomatic Rust solution.
