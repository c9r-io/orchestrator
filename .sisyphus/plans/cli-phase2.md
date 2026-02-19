# CLI Phase 2: apply, edit, db reset

## TL;DR

> **Quick Summary**: Add three kubectl-style CLI commands to the orchestrator: `db reset` (clear runtime data), `apply -f <yaml>` (declarative resource management), and `edit <resource>` (interactive editing). Implements a compile-time Resource trait system for extensible resource definitions.

> **Deliverables**:
> - `orchestrator db reset --force` command
> - `orchestrator apply -f <file>` with --dry-run support
> - `orchestrator edit <resource>` with $EDITOR workflow
> - Resource trait + 4 resource kind implementations (Workspace, Agent, AgentGroup, Workflow)
> - k8s-style YAML format (apiVersion/kind/metadata/spec)

> **Estimated Effort**: Medium
> **Parallel Execution**: YES - 3 waves
> **Critical Path**: Test infrastructure → Resource trait → apply → edit/db-reset

---

## Context

### Original Request
User requested:
1. Clear current database content
2. Add `apply -f <yaml>` feature (like kubectl)
3. Add `edit <resource>` feature (like kubectl)

### Interview Summary
**Key Discussions**:
- User wants **CRD-like extensible resource system** — decided on compile-time Rust trait registry
- **k8s-style YAML format** with apiVersion/kind/metadata/spec
- **Multi-document YAML** supported (--- separator)
- **Create-or-update semantics** for apply
- **$EDITOR workflow** for edit (re-open on validation error)
- **TDD** with 90% coverage required
- **kubectl-style output**: "workspace/default configured", "agent/opencode created"

### Metis Review
**Identified Gaps** (addressed):
- **Task as Resource**: DROP from v1 — different lifecycle (UUID-keyed, runtime state). Add later if needed.
- **db reset scope**: Must only clear runtime data (tasks/items/events), NOT config
- **Test infrastructure**: Need InnerState test helper factory FIRST — all TDD depends on it
- **main.rs constraint**: All config types are non-public; Resource trait must live inside main.rs crate

---

## Work Objectives

### Core Objective
Extend the kubectl-style CLI with three new commands and a reusable Resource abstraction layer.

### Concrete Deliverables
- [ ] `orchestrator db reset --force` — clears runtime data, requires --force flag
- [ ] `orchestrator apply -f <yaml>` — create-or-update resources from YAML
- [ ] `orchestrator apply -f <yaml> --dry-run` — preview without applying
- [ ] `orchestrator edit <resource>` — edit config resources in $EDITOR
- [ ] Resource trait system with 4 kinds: Workspace, Agent, AgentGroup, Workflow

### Definition of Done
- [ ] `./scripts/orchestrator.sh db reset --force` clears tasks but preserves config
- [ ] `./scripts/orchestrator.sh apply -f workspace.yaml` creates/updates Workspace
- [ ] `./scripts/orchestrator.sh apply -f multi.yaml` handles --- separator
- [ ] `./scripts/orchestrator.sh apply -f file.yaml --dry-run` shows preview only
- [ ] `./scripts/orchestrator.sh edit workspace/default` opens in $EDITOR
- [ ] All acceptance criteria executable as shell commands
- [ ] 90% test coverage maintained

### Must Have
- k8s-style YAML parsing (apiVersion/kind/metadata/spec)
- Multi-document YAML support
- --dry-run for apply
- --force for db reset
- kubectl-style output messages
- TDD with tests-first approach
- 90% coverage threshold

### Must NOT Have (Guardrails)
- ❌ Task as Resource kind (different lifecycle)
- ❌ Delete semantics in apply (--prune)
- ❌ UI integration for new commands
- ❌ Refactor main.rs structure
- ❌ Change internal config format (flat OrchestratorConfig stays)
- ❌ More than 1-2 new dependencies
- ❌ db reset that clears config (only runtime data)

---

## Verification Strategy

### Test Decision
- **Infrastructure exists**: NO (need to create InnerState test helper)
- **Automated tests**: TDD (tests-first approach)
- **Framework**: cargo test (Rust built-in)
- **Coverage target**: 90% (per project requirements)

### QA Policy
Every task MUST include agent-executed QA scenarios. Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

| Deliverable Type | Verification Tool | Method |
|------------------|-----------------|--------|
| CLI command | Bash | Run command, parse output, check exit code |
| YAML parsing | Bash | Apply valid/invalid YAML, verify behavior |
| Edit workflow | Bash | Mock EDITOR="cat" for non-interactive testing |
| DB operations | Bash | Query SQLite before/after reset |

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Foundation — sequential dependency):
├── T1: InnerState test helper factory [BLOCKS ALL OTHER TESTS]
├── T2: k8s-style YAML types (ApiVersion, ResourceManifest, ResourceSpec)
├── T3: Resource trait definition + registry
└── T4: Resource impls for Workspace, Agent, AgentGroup, Workflow

Wave 2 (apply + db-reset — MAX PARALLEL):
├── T5: db reset CLI command [depends: T1]
├── T6: apply --dry-run implementation [depends: T3, T4]
├── T7: apply create-or-update logic [depends: T6]
├── T8: apply multi-document support [depends: T7]
└── T9: apply CLI integration (cli.rs + cli_handler.rs) [depends: T6]

Wave 3 (edit + final integration):
├── T10: edit export to YAML [depends: T3, T4]
├── T11: edit validation + re-open loop [depends: T10]
├── T12: edit CLI integration [depends: T11]
├── T13: Integration tests for all commands [depends: T5, T9, T12]
└── T14: Coverage verification (90% threshold) [depends: T13]
```

### Dependency Matrix

| Task | Depends On | Blocks | Wave |
|------|------------|--------|------|
| T1 | — | 2-14 | 1 |
| T2 | T1 | 3 | 1 |
| T3 | T2 | 4-14 | 1 |
| T4 | T3 | 6-14 | 1 |
| T5 | T1 | 13 | 2 |
| T6 | T4 | 7-9 | 2 |
| T7 | T6 | 8-9 | 2 |
| T8 | T7 | 9 | 2 |
| T9 | T6 | 13 | 2 |
| T10 | T4 | 11-12 | 3 |
| T11 | T10 | 12-13 | 3 |
| T12 | T11 | 13 | 3 |
| T13 | T5, T9, T12 | 14 | 3 |
| T14 | T13 | — | 3 |

---

## TODOs

- [x] 1. **Create InnerState test helper factory**

  **What to do**:
  - Create test utilities module for creating temp directories, temp SQLite DB, and seeded InnerState
  - Implement `TestState` builder pattern with methods: `with_workspace()`, `with_agent()`, `with_workflow()`
  - Ensure cleanup (temp dir deletion) after tests
  - This is BLOCKING for all other TDD tasks

  **Must NOT do**:
  - Don't modify production InnerState creation
  - Don't add new production dependencies

  **Recommended Agent Profile**:
  > **Category**: `deep`
  > - Reason: Test infrastructure requires understanding existing state creation patterns
  > **Skills**: [`rust-conventions`]
  > - `rust-conventions`: Ensure test code follows Rust best practices

  **Parallelization**:
  - **Can Run In Parallel**: NO (sequential foundation task)
  - **Parallel Group**: Wave 1
  - **Blocks**: ALL other tasks (T2-T14)
  - **Blocked By**: None

  **References**:
  - `src-tauri/src/main.rs:4000-4200` - InnerState initialization pattern
  - `src-tauri/src/main.rs:100-200` - Config loading from default.yaml
  - Use `std::env::temp_dir()` for temp paths

  **Acceptance Criteria**:
  - [ ] Test module compiles: `cargo test --no-run`
  - [ ] `TestState::new().with_workspace("test", path).build()` creates valid InnerState
  - [ ] Temp directory is cleaned up after test

  **QA Scenarios**:
  ```
  Scenario: TestState creates valid InnerState with workspace
    Tool: Bash
    Preconditions: None
    Steps:
      1. Run: cargo test test_state_workspace -- --nocapture
    Expected Result: Test passes, InnerState has workspace
    Evidence: .sisyphus/evidence/task-1-test-state.{ext}
  ```

  **Commit**: YES
  - Message: `test(orchestrator): add InnerState test helper factory`
  - Files: `src-tauri/src/test_utils.rs`

---

- [x] 2. **Define k8s-style YAML types**

  **What to do**:
  - Create `OrchestratorResource`, `ResourceMetadata`, `ResourceSpec` structs
  - Support apiVersion field (e.g., "orchestrator.dev/v1")
  - Support kind: Workspace | Agent | AgentGroup | Workflow
  - Support metadata.name for resource identity
  - Use serde for YAML deserialization

  **Must NOT do**:
  - Don't change internal config format
  - Don't implement business logic

  **Recommended Agent Profile**:
  > **Category**: `quick`
  > - Reason: Pure data structure definition, no complex logic

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on T1)
  - **Parallel Group**: Wave 1
  - **Blocks**: T3 (Resource trait)
  - **Blocked By**: T1

  **References**:
  - `config/default.yaml` - current flat config format
  - serde_yaml documentation for multi-document parsing

  **Acceptance Criteria**:
  - [ ] Valid k8s-style YAML parses to OrchestratorResource
  - [ ] Invalid YAML produces clear error
  - [ ] ApiVersion and Kind are validated

  **QA Scenarios**:
  ```
  Scenario: Parse valid Workspace YAML
    Tool: Bash
    Preconditions: Cargo compiles
    Steps:
      1. cargo test parse_workspace_yaml
    Expected Result: Test passes
    Evidence: .sisyphus/evidence/task-2-parse-yaml.{ext}

  Scenario: Reject invalid apiVersion
    Tool: Bash
    Preconditions: Cargo compiles
    Steps:
      1. cargo test invalid_apiversion
    Expected Result: Test fails with clear error
    Evidence: .sisyphus/evidence/task-2-invalid-api.{ext}
  ```

  **Commit**: YES
  - Message: `feat(cli): add k8s-style YAML types`
  - Files: `src-tauri/src/cli_types.rs`

---

- [ ] 3. **Define Resource trait with registry**

  **What to do**:
  - Define `Resource` trait with methods:
    - `kind() -> ResourceKind`
    - `name(&self) -> &str`
    - `validate(&self) -> Result<()>`
    - `apply(&self, config: &mut OrchestratorConfig) -> ApplyResult`
    - `to_yaml(&self) -> Result<String>`
    - `get_from(config: &OrchestratorConfig) -> Option<Self>`
  - Define `ResourceKind` enum: Workspace, Agent, AgentGroup, Workflow
  - Define `ApplyResult` enum: Created, Configured, Unchanged

  **Must NOT do**:
  - Don't implement apply logic (T4)
  - Don't add delete semantics

  **Recommended Agent Profile**:
  > **Category**: `deep`
  > - Reason: Trait design requires understanding how resources integrate with config

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on T2)
  - **Parallel Group**: Wave 1
  - **Blocks**: T4-T14
  - **Blocked By**: T2

  **References**:
  - `main.rs` - OrchestratorConfig structure (private, use crate::)
  - `cli_handler.rs:187-216` - existing config manipulation patterns

  **Acceptance Criteria**:
  - [ ] Resource trait compiles
  - [ ] ResourceKind enum has 4 variants
  - [ ] Trait methods cover all needed operations

  **QA Scenarios**:
  ```
  Scenario: Resource trait compiles with all methods
    Tool: Bash
    Preconditions: Cargo compiles
    Steps:
      1. cargo test resource_trait
    Expected Result: All trait method signatures valid
    Evidence: .sisyphus/evidence/task-3-trait.{ext}
  ```

  **Commit**: YES
  - Message: `feat(cli): add Resource trait and registry`
  - Files: `src-tauri/src/resource.rs`

---

- [ ] 4. **Implement Resource trait for 4 config kinds**

  **What to do**:
  - Implement Resource for WorkspaceConfig
  - Implement Resource for AgentConfig
  - Implement Resource for AgentGroupConfig
  - Implement Resource for WorkflowConfig
  - Each impl: validate(), apply(), to_yaml(), get_from()

  **Must NOT do**:
  - Don't implement Task (different lifecycle)
  - Don't change existing config structures

  **Recommended Agent Profile**:
  > **Category**: `deep`
  > - Reason: Each impl needs to understand config HashMap structure

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on T3)
  - **Parallel Group**: Wave 1
  - **Blocks**: T6-T14
  - **Blocked By**: T3

  **References**:
  - `main.rs` - WorkspaceConfig, AgentConfig, AgentGroupConfig, WorkflowConfig definitions
  - `persist_config_and_reload()` - existing save path to reuse

  **Acceptance Criteria**:
  - [ ] Workspace implements Resource
  - [ ] Agent implements Resource
  - [ ] AgentGroup implements Resource
  - [ ] Workflow implements Resource
  - [ ] All impls pass validate() for valid config
  - [ ] All impls fail validate() for invalid config

  **QA Scenarios**:
  ```
  Scenario: Workspace Resource apply creates new workspace
    Tool: Bash
    Preconditions: Test infrastructure ready
    Steps:
      1. cargo test workspace_resource_apply
    Expected Result: New workspace added to config
    Evidence: .sisyphus/evidence/task-4-workspace.{ext}

  Scenario: Agent Resource apply updates existing
    Tool: Bash
    Preconditions: Test infrastructure ready
    Steps:
      1. cargo test agent_resource_apply
    Expected Result: Existing agent updated
    Evidence: .sisyphus/evidence/task-4-agent.{ext}
  ```

  **Commit**: YES
  - Message: `feat(cli): implement Resource trait for 4 config kinds`
  - Files: `src-tauri/src/resource.rs`

---

- [ ] 5. **Implement db reset command**

  **What to do**:
  - Add `Db` variant to Commands enum in cli.rs
  - Add `DbCommands::Reset` with --force flag
  - Implement handler: delete all rows from tasks, task_items, command_runs, events tables
  - Preserve config tables (orchestrator_config, orchestrator_config_versions)
  - Require --force flag or error

  **Must NOT do**:
  - Don't reset config
  - Don't delete the DB file (truncate tables)
  - Don't fail if no tasks exist

  **Recommended Agent Profile**:
  > **Category**: `quick`
  > - Reason: Standalone command, clear scope

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on T1)
  - **Parallel Group**: Wave 2
  - **Blocks**: T13 (integration)
  - **Blocked By**: T1

  **References**:
  - `cli.rs:24-41` - Commands enum pattern
  - `cli_handler.rs:15-25` - command dispatch pattern

  **Acceptance Criteria**:
  - [ ] `db reset` without --force fails
  - [ ] `db reset --force` clears tasks but not config
  - [ ] `task list` returns empty after reset

  **QA Scenarios**:
  ```
  Scenario: db reset without --force fails
    Tool: Bash
    Preconditions: None
    Steps:
      1. cd orchestrator && ./scripts/orchestrator.sh db reset 2>&1
    Expected Result: exit code 1, message mentions --force
    Evidence: .sisyphus/evidence/task-5-no-force.{ext}

  Scenario: db reset --force clears runtime data
    Tool: Bash
    Preconditions: Has existing tasks
    Steps:
      1. cd orchestrator && ./scripts/orchestrator.sh task list -o json | jq 'length'
      2. cd orchestrator && ./scripts/orchestrator.sh db reset --force
      3. cd orchestrator && ./scripts/orchestrator.sh task list -o json | jq 'length'
    Expected Result: Before > 0, After = 0
    Evidence: .sisyphus/evidence/task-5-reset.{ext}

  Scenario: config survives db reset
    Tool: Bash
    Preconditions: None
    Steps:
      1. cd orchestrator && ./scripts/orchestrator.sh db reset --force
      2. cd orchestrator && ./scripts/orchestrator.sh config view -o json | jq '.workspaces | length'
    Expected Result: workspaces count > 0
    Evidence: .sisyphus/evidence/task-5-config-survives.{ext}
  ```

  **Commit**: YES
  - Message: `feat(cli): add db reset command`
  - Files: `src-tauri/src/cli.rs`, `src-tauri/src/cli_handler.rs`

---

- [ ] 6. **Implement apply --dry-run**

  **What to do**:
  - Add Apply variant to Commands enum
  - Add --dry-run flag to apply
  - Parse k8s-style YAML to OrchestratorResource
  - Validate resources without persisting
  - Output what would happen: "workspace/default would be created (dry run)"

  **Must NOT do**:
  - Don't persist changes in dry-run mode

  **Recommended Agent Profile**:
  > **Category**: `deep`
  > - Reason: YAML parsing + validation logic

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 2, with T7-T9)
  - **Parallel Group**: Wave 2
  - **Blocks**: T7
  - **Blocked By**: T4

  **References**:
  - `cli_handler.rs:187-216` - config command patterns
  - serde_yaml multi-document docs

  **Acceptance Criteria**:
  - [ ] --dry-run parses but doesn't persist
  - [ ] Output shows what would be created/configured

  **QA Scenarios**:
  ```
  Scenario: apply --dry-run shows preview
    Tool: Bash
    Preconditions: None
    Steps:
      1. echo 'apiVersion: orchestrator.dev/v1
  kind: Workspace
  metadata:
    name: dryrun-test
  spec:
    root_path: /tmp/dry
    qa_targets: []
    ticket_dir: docs/ticket' > /tmp/dry.yaml
      2. cd orchestrator && ./scripts/orchestrator.sh apply -f /tmp/dry.yaml --dry-run
    Expected Result: Contains "dryrun-test" and "dry run"
    Evidence: .sisyphus/evidence/task-6-dryrun.{ext}
  ```

  **Commit**: YES
  - Message: `feat(cli): add apply --dry-run`
  - Files: `src-tauri/src/cli.rs`, `src-tauri/src/cli_handler.rs`

---

- [ ] 7. **Implement apply create-or-update logic**

  **What to do**:
  - Check if resource exists in current config
  - If exists: update (Configured)
  - If not exists: create (Created)
  - Merge into OrchestratorConfig
  - Call persist_config_and_reload()
  - Output kubectl-style messages

  **Must NOT do**:
  - Don't delete unmentioned resources

  **Recommended Agent Profile**:
  > **Category**: `deep`
  > - Reason: Core business logic for apply

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 2, with T6, T8-T9)
  - **Parallel Group**: Wave 2
  - **Blocks**: T8
  - **Blocked By**: T6

  **References**:
  - `persist_config_and_reload()` - existing save path
  - `build_active_config()` - existing validation

  **Acceptance Criteria**:
  - [ ] Apply new workspace: "workspace/X created"
  - [ ] Apply existing workspace: "workspace/X configured"
  - [ ] Existing resources preserved after apply

  **QA Scenarios**:
  ```
  Scenario: apply creates new workspace
    Tool: Bash
    Preconditions: None
    Steps:
      1. cat > /tmp/apply-test.yaml << 'EOF'
  apiVersion: orchestrator.dev/v1
  kind: Workspace
  metadata:
    name: apply-test
  spec:
    root_path: /tmp/apply
    qa_targets: ["docs/qa"]
    ticket_dir: docs/ticket
  EOF
      2. cd orchestrator && ./scripts/orchestrator.sh apply -f /tmp/apply-test.yaml
    Expected Result: stdout contains "workspace/apply-test created"
    Evidence: .sisyphus/evidence/task-7-created.{ext}

  Scenario: apply updates existing workspace
    Tool: Bash
    Preconditions: apply-test workspace exists
    Steps:
      1. cd orchestrator && ./scripts/orchestrator.sh apply -f /tmp/apply-test.yaml
    Expected Result: stdout contains "workspace/apply-test configured"
    Evidence: .sisyphus/evidence/task-7-configured.{ext}
  ```

  **Commit**: YES
  - Message: `feat(cli): implement apply create-or-update`
  - Files: `src-tauri/src/cli_handler.rs`

---

- [ ] 8. **Implement apply multi-document support**

  **What to do**:
  - Parse YAML with --- separators
  - Process each document sequentially
  - Aggregate results
  - Output line for each resource

  **Must NOT do**:
  - Don't fail entire file on one document error

  **Recommended Agent Profile**:
  > **Category**: `quick`
  > - Reason: Extension of apply logic

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 2, with T6-T7, T9)
  - **Parallel Group**: Wave 2
  - **Blocks**: None
  - **Blocked By**: T7

  **References**:
  - serde_yaml multi-document parsing

  **Acceptance Criteria**:
  - [ ] File with --- applies all resources
  - [ ] Each resource gets separate output line

  **QA Scenarios**:
  ```
  Scenario: multi-document YAML applies all resources
    Tool: Bash
    Preconditions: None
    Steps:
      1. cat > /tmp/multi.yaml << 'EOF'
  apiVersion: orchestrator.dev/v1
  kind: Workspace
  metadata:
    name: ws-multi
  spec:
    root_path: /tmp/ws
    qa_targets: []
    ticket_dir: docs/ticket
  ---
  apiVersion: orchestrator.dev/v1
  kind: Agent
  metadata:
    name: agent-multi
  spec:
    templates:
      qa: echo test
  EOF
      2. cd orchestrator && ./scripts/orchestrator.sh apply -f /tmp/multi.yaml
    Expected Result: stdout contains both "workspace/ws-multi" and "agent/agent-multi"
    Evidence: .sisyphus/evidence/task-8-multi.{ext}
  ```

  **Commit**: YES
  - Message: `feat(cli): add multi-document YAML support`
  - Files: `src-tauri/src/cli_handler.rs`

---

- [ ] 9. **Integrate apply into CLI**

  **What to do**:
  - Add Apply variant to cli.rs Commands enum
  - Add -f (file) and --dry-run flags
  - Wire up handler in cli_handler.rs

  **Must NOT do**:
  - Don't add new functionality (T6-T8 have it)

  **Recommended Agent Profile**>
  > **Category**: `quick`
  > - Reason: Wiring existing logic into CLI

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 2, with T6-T8)
  - **Parallel Group**: Wave 2
  - **Blocks**: T13
  - **Blocked By**: T6

  **References**:
  - `cli.rs:24-41` - Commands enum pattern
  - `cli_handler.rs:15-25` - dispatch pattern

  **Acceptance Criteria**:
  - [ ] `./orchestrator.sh apply -f file.yaml` works
  - [ ] `--dry-run` flag recognized

  **QA Scenarios**:
  ```
  Scenario: apply command is registered
    Tool: Bash
    Preconditions: None
    Steps:
      1. cd orchestrator && ./scripts/orchestrator.sh apply --help
    Expected Result: Shows apply usage
    Evidence: .sisyphus/evidence/task-9-help.{ext}
  ```

  **Commit**: YES
  - Message: `feat(cli): wire apply into CLI`
  - Files: `src-tauri/src/cli.rs`

---

- [ ] 10. **Implement edit export to YAML**

  **What to do**:
  - Parse resource selector (e.g., "workspace/default")
  - Get resource from config via Resource::get_from()
  - Convert to k8s-style YAML via Resource::to_yaml()
  - Write to temp file

  **Must NOT do**:
  - Don't implement edit loop

  **Recommended Agent Profile**:
  > **Category**: `quick`
  > - Reason: Data transformation logic

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 3, with T11-T12)
  - **Parallel Group**: Wave 3
  - **Blocks**: T11
  - **Blocked By**: T4

  **References**:
  - T4 Resource impls for to_yaml()

  **Acceptance Criteria**:
  - [ ] Export produces valid k8s-style YAML
  - [ ] Nonexistent resource returns error

  **QA Scenarios**:
  ```
  Scenario: edit exports workspace as k8s YAML
    Tool: Bash
    Preconditions: None
    Steps:
      1. EDITOR="cat" cd orchestrator && ./scripts/orchestrator.sh edit workspace/default 2>/dev/null | head -5
    Expected Result: Contains apiVersion, kind, metadata, spec
    Evidence: .sisyphus/evidence/task-10-export.{ext}
  ```

  **Commit**: YES
  - Message: `feat(cli): add edit export functionality`
  - Files: `src-tauri/src/cli_handler.rs`

---

- [ ] 11. **Implement edit validation + re-open loop**

  **What to do**:
  - Spawn $EDITOR on temp file
  - Read back edited content
  - Validate YAML
  - If invalid: print error, re-open loop
  - If valid or empty: exit

  **Must NOT do**:
  - Don't apply without validation

  **Recommended Agent Profile**>
  > **Category**: `deep`
  > - Reason: Complex interactive loop

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 3, with T10, T12)
  - **Parallel Group**: Wave 3
  - **Blocks**: T12
  - **Blocked By**: T10

  **References**:
  - std::process::Command for EDITOR spawning

  **Acceptance Criteria**:
  - [ ] Invalid YAML shows error and re-opens
  - [ ] Ctrl+C aborts gracefully
  - [ ] Empty file aborts

  **QA Scenarios**:
  ```
  Scenario: edit with invalid YAML re-opens
    Tool: Bash
    Preconditions: None
    Steps:
      1. EDITOR="echo 'invalid: [' >" cd orchestrator && ./scripts/orchestrator.sh edit workspace/default 2>&1
    Expected Result: Error message about invalid YAML
    Evidence: .sisyphus/evidence/task-11-invalid.{ext}
  ```

  **Commit**: YES
  - Message: `feat(cli): add edit validation loop`
  - Files: `src-tauri/src/cli_handler.rs`

---

- [ ] 12. **Integrate edit into CLI**

  **What to do**:
  - Add Edit variant to cli.rs Commands enum
  - Add resource selector argument (workspace/<id>, agent/<id>, etc.)
  - Wire up handler

  **Must NOT do**:
  - Don't add new functionality (T10-T11 have it)

  **Recommended Agent Profile**:
  > **Category**: `quick`
  > - Reason: Wiring existing logic

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 3, with T10-T11)
  - **Parallel Group**: Wave 3
  - **Blocks**: T13
  - **Blocked By**: T11

  **References**:
  - cli.rs Commands pattern

  **Acceptance Criteria**:
  - [ ] `./orchestrator.sh edit workspace/default` works
  - [ ] `--help` shows usage

  **QA Scenarios**:
  ```
  Scenario: edit command is registered
    Tool: Bash
    Preconditions: None
    Steps:
      1. cd orchestrator && ./scripts/orchestrator.sh edit --help
    Expected Result: Shows edit usage
    Evidence: .sisyphus/evidence/task-12-help.{ext}
  ```

  **Commit**: YES
  - Message: `feat(cli): wire edit into CLI`
  - Files: `src-tauri/src/cli.rs`

---

- [ ] 13. **Integration tests for all commands**

  **What to do**:
  - Test db reset + task list round-trip
  - Test apply + config view round-trip
  - Test apply preserves existing resources
  - Test multi-document apply
  - Test edit export format

  **Must NOT do**:
  - Don't test in isolation (unit tests cover that)

  **Recommended Agent Profile**:
  > **Category**: `unspecified-high`
  > - Reason: End-to-end integration validation

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on T5, T9, T12)
  - **Parallel Group**: Wave 3
  - **Blocks**: T14
  - **Blocked By**: T5, T9, T12

  **Acceptance Criteria**:
  - [ ] All CLI commands work together
  - [ ] No regressions in existing commands

  **QA Scenarios**:
  ```
  Scenario: Full apply + edit round-trip
    Tool: Bash
    Preconditions: None
    Steps:
      1. cd orchestrator && ./scripts/orchestrator.sh apply -f /tmp/apply-test.yaml
      2. EDITOR="cat" cd orchestrator && ./scripts/orchestrator.sh edit workspace/apply-test 2>/dev/null | grep "name: apply-test"
    Expected Result: Edit shows the applied workspace
    Evidence: .sisyphus/evidence/task-13-roundtrip.{ext}

  Scenario: apply preserves existing resources
    Tool: Bash
    Preconditions: Default workspace exists
    Steps:
      1. cd orchestrator && ./scripts/orchestrator.sh config view -o json | jq '.workspaces | keys'
      2. cd orchestrator && ./scripts/orchestrator.sh apply -f /tmp/apply-test.yaml
      3. cd orchestrator && ./scripts/orchestrator.sh config view -o json | jq '.workspaces | keys'
    Expected Result: default still present after apply
    Evidence: .sisyphus/evidence/task-13-preserve.{ext}
  ```

  **Commit**: YES
  - Message: `test(orchestrator): add integration tests`
  - Files: Tests in existing test module

---

- [ ] 14. **Coverage verification (90% threshold)**

  **What to do**:
  - Run `cargo llvm-cov --fail-under-lines 90`
  - Fix any coverage gaps
  - Verify final threshold

  **Must NOT do**:
  - Don't lower threshold
  - Don't skip verification

  **Recommended Agent Profile**:
  > **Category**: `quick`
  > - Reason: Verification step

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on T13)
  - **Parallel Group**: Wave 3
  - **Blocks**: None
  - **Blocked By**: T13

  **References**:
  - `src-tauri/Makefile` - existing coverage config

  **Acceptance Criteria**:
  - [ ] `make -C src-tauri coverage` passes at 90%

  **QA Scenarios**:
  ```
  Scenario: Coverage meets threshold
    Tool: Bash
    Preconditions: All tests pass
    Steps:
      1. cd orchestrator/src-tauri && cargo llvm-cov --fail-under-lines 90
    Expected Result: PASS (90%+ lines covered)
    Evidence: .sisyphus/evidence/task-14-coverage.{ext}
  ```

  **Commit**: YES (if code changes needed)
  - Message: `chore: achieve 90% test coverage`

---

## Final Verification Wave

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Verify all Must Have items implemented, all Must NOT Have items absent.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **CLI Regression Tests** — `unspecified-high`
  Run existing CLI commands (task list, workspace list, config view) to ensure no regressions.
  Output: `Existing commands [N/N] | VERDICT`

- [ ] F3. **Manual QA** — `unspecified-high`
  Execute all acceptance criteria from tasks T1-T14.
  Output: `Scenarios [N/N pass] | VERDICT`

- [ ] F4. **Coverage Gate** — `deep`
  Verify 90% coverage threshold.
  Output: `Coverage [N%] | Threshold [90%] | VERDICT`

---

## Commit Strategy

| After Task | Message | Files |
|------------|---------|-------|
| T1 | `test(orchestrator): add InnerState test helper factory` | test_utils.rs |
| T2 | `feat(cli): add k8s-style YAML types` | cli_types.rs |
| T3 | `feat(cli): add Resource trait and registry` | resource.rs |
| T4 | `feat(cli): implement Resource trait for 4 config kinds` | resource.rs |
| T5 | `feat(cli): add db reset command` | cli.rs, cli_handler.rs |
| T6 | `feat(cli): add apply --dry-run` | cli.rs, cli_handler.rs |
| T7 | `feat(cli): implement apply create-or-update` | cli_handler.rs |
| T8 | `feat(cli): add multi-document YAML support` | cli_handler.rs |
| T9 | `feat(cli): wire apply into CLI` | cli.rs |
| T10 | `feat(cli): add edit export functionality` | cli_handler.rs |
| T11 | `feat(cli): add edit validation loop` | cli_handler.rs |
| T12 | `feat(cli): wire edit into CLI` | cli.rs |
| T13 | `test(orchestrator): add integration tests` | test modules |
| T14 | `chore: achieve 90% test coverage` | (if needed) |

---

## Success Criteria

### Verification Commands
```bash
# db reset
./scripts/orchestrator.sh db reset  # fails without --force
./scripts/orchestrator.sh db reset --force  # succeeds

# apply
./scripts/orchestrator.sh apply -f workspace.yaml
./scripts/orchestrator.sh apply -f multi.yaml --dry-run

# edit
EDITOR="cat" ./scripts/orchestrator.sh edit workspace/default

# coverage
make -C src-tauri coverage  # >= 90%
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] All tests pass
- [ ] 90% coverage threshold met
- [ ] kubectl-style output messages implemented
- [ ] Multi-document YAML supported
- [ ] --dry-run for apply works
- [ ] --force for db reset works
