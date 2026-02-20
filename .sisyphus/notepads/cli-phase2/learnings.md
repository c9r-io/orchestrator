- Added `TestState` builder in `orchestrator/src-tauri/src/test_utils.rs` that seeds isolated app_root/config/db/logs under `std::env::temp_dir()` unique subdirectories and returns reusable `Arc<InnerState>` instances.
- Mirroring test config to temp `config/default.yaml` plus `init_schema` + `load_or_seed_config` + `build_active_config` + `backfill_legacy_data` gives parity with production initialization while keeping test DB isolated.
- Implemented `Drop` cleanup on `TestState` to recursively remove temp roots and added focused tests (`test_state_compiles`, `test_state_creates_workspace`, `test_state_cleanup`) for infrastructure guarantees.
- Created k8s-style YAML type system in `cli_types.rs` with `OrchestratorResource`, `ResourceKind` enum (4 variants: Workspace, Agent, AgentGroup, Workflow), `ResourceMetadata` (name, labels, annotations), and untagged `ResourceSpec` enum for kind-specific configuration.
- Used `#[serde(untagged)]` for `ResourceSpec` to allow discriminating by which variant deserializes successfully; apiVersion/kind are separate fields for discoverability and validation (follows k8s convention).
- `OrchestratorResource::validate_version()` enforces `apiVersion == "orchestrator.dev/v1"` and returns descriptive error if mismatched; called early in tests before accessing spec to avoid borrow checker issues.
- Each resource kind has dedicated struct: `WorkspaceSpec` (root_path, qa_targets, ticket_dir), `AgentSpec` (templates for init_once/qa/fix/retest/loop_guard), `AgentGroupSpec` (agents list), `WorkflowSpec` (steps, loop_policy, finalize).
- Tests cover all 4 resource kinds, annotations support, and explicit apiVersion rejection; borrowing pattern: validate_version() early, then reference-bind spec extraction with `&resource.spec` to avoid partial moves.
- No business logic (apply, edit) implemented — cli_types is INPUT-ONLY schema for declarative manifests; internal OrchestratorConfig structures remain unchanged for backward compatibility.

- Added `src-tauri/src/resource.rs` as the compile-time bridge layer: `Resource` trait + `ApplyResult` enum + `RegisteredResource` dispatch enum to represent Workspace/Agent/AgentGroup/Workflow without per-kind trait impls yet.
- Introduced explicit `resource_registry()` with typed constructor fns (`build_workspace`, `build_agent`, `build_agent_group`, `build_workflow`) so `dispatch_resource(OrchestratorResource)` validates kind/spec alignment before conversion.
- `Resource` is implemented on the dispatch enum only; `apply()` performs generic map upsert (`Created`/`Configured`/`Unchanged`) via serialization-equivalence, while deep per-kind business rules remain deferred to later tasks.
- Added reversible bridge conversions between YAML specs and runtime config structs (including workflow loop/finalize/step prehook shape mapping) to support `apply()` and `get_from()` round-tripping.
- Added focused tests in `resource.rs` for trait surface (`resource_trait_*`), apply result semantics (`apply_result_*`), and dispatch correctness (`resource_dispatch_*`) using `TestState` for isolated config snapshots.

- Added direct `Resource` trait impls for `WorkspaceResource`, `AgentResource`, `AgentGroupResource`, and `WorkflowResource`, with `RegisteredResource` now delegating to per-kind impls for `validate/apply/to_yaml`.
- Shared helpers (`validate_resource_name`, `metadata_with_name`, `manifest_yaml`) reduce duplicate k8s-manifest assembly and ensure each kind emits `apiVersion/kind/metadata/spec` YAML consistently.
- Per-kind validation now checks required fields at the wrapper level: workspace path/ticket dir non-empty, agent has at least one non-empty template string, agent group has non-empty members, workflow has non-empty steps with non-empty `id`/`type`.
- Added roundtrip tests for each kind (`workspace_resource_apply`, `agent_resource_apply`, `agent_group_resource_roundtrip`, `workflow_resource_roundtrip`) and a dedicated serialization test (`resource_to_yaml`) using `TestState` snapshots.

- Added top-level CLI command parsing for `apply -f <file> [--dry-run]` in `cli.rs`, including defaults (`dry_run = false`) and explicit `-f/--file` manifest targeting.
- Implemented `CliHandler::handle_apply` to parse YAML streams with `serde_yaml::Deserializer`, skip null docs, validate `apiVersion`, dispatch to `RegisteredResource`, run `resource.validate()`, and print kubectl-style dry-run messages without persistence.
- Used kind-specific `Resource::get_from` checks (`WorkspaceResource`, `AgentResource`, `AgentGroupResource`, `WorkflowResource`) against active config to classify each manifest as `created` vs `configured` during dry-run preview.
- Added CLI handler tests for `apply_dry_run` exit behavior and non-persistence guarantees, plus `multi_document` coverage proving `---` manifests are parsed and processed document-by-document.
- Implemented real apply persistence path in : clone active config, validate and merge each manifest via , print kubectl-style , and call  only when  is false and no validation errors occurred.
- Added  helper that uses kind-specific  existence checks to classify each resource as  or  while still invoking merge logic for deterministic in-memory state updates across multi-document apply runs.
- Added non-dry-run CLI tests , , and ; tests pre-create workspace dirs to satisfy config validation and verify dry-run does not bump config version while non-dry-run does.
- Task7 correction: handle_apply now clones active config, applies validated manifests, prints workspace/agent style created/configured messages, and persists merged config only in non-dry-run mode after all documents pass validation.
- Task7 correction: apply_resource checks existence via per-kind get_from before merge, then returns Created vs Configured classification while still calling merge logic on the working config.
- Task7 correction: added apply_create/apply_update/apply_persist tests covering create flow, update flow, and persistence version behavior (dry-run no version bump, non-dry-run version bump).

- T11:  must drop active-config read lock before persist/reload to avoid deadlock in tests and runtime loop.
- T11: Mocking  is reliable with executable shell scripts plus an env mutex to serialize  mutation across parallel tests.
- T11: Re-open loop validated by first writing invalid manifest then valid manifest; assert invocation count to prove retry path.

- F4 coverage gate verification: `make coverage` in `orchestrator/src-tauri` runs `cargo llvm-cov --lib --fail-under-lines 90` and currently reports TOTAL regions/functions/lines at 100.00% with branch metric shown as `0/0` (`-`), so threshold checks should treat branches as non-applicable when no branch data is emitted.

- T11: edit open must drop active-config read lock before persist/reload to avoid deadlock in tests and runtime loop.
- T11: Mocking $EDITOR is reliable with executable shell scripts plus an env mutex to serialize EDITOR mutation across parallel tests.
- T11: Re-open loop validated by first writing invalid manifest then valid manifest; assert invocation count to prove retry path.

## F3 Manual QA Execution (2026-02-20)

### Test Strategy
- T5 db reset: Tested via CLI wrapper (worked) + integration tests
- T6-T14 apply/edit: Discovered CLI routing bug, validated via unit/integration tests instead
- Coverage: Used cargo llvm-cov to verify thresholds

### Key Findings
1. **CLI routing incomplete**: main.rs:5403 missing Apply/Edit in match pattern
   - Commands defined in cli.rs ✅
   - Handlers implemented in cli_handler.rs ✅
   - But main() routes only Task/Workspace/Config/Db commands
   - Result: Apply/Edit commands hang waiting for Tauri init

2. **Test infrastructure excellent**: All functionality validated via:
   - 64 unit tests in main.rs
   - 10 integration tests
   - TestState helper enables isolated testing

3. **Coverage metrics**:
   - New CLI modules (resource.rs, cli_types.rs): 90%+ ✅
   - CLI handler: 65% (functional paths tested, error paths partially)
   - Main.rs: 29% (expected - mostly Tauri/UI/orchestration code)

### What Worked Well
- TDD approach caught issues early
- Integration tests validated end-to-end workflows
- Multi-document YAML parsing solid
- Resource trait abstraction clean

### What Needs Attention
- Fix CLI routing in main.rs (1-line change)
- Consider splitting main.rs into modules for testability
- Add error path coverage for cli_handler.rs

### Evidence Collected
- 8 evidence files capturing test outputs
- Summary document with detailed per-task results
