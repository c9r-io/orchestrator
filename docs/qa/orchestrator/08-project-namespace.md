---
self_referential_safe: true
---

# Orchestrator - Project Namespace

**Module**: orchestrator
**Scope**: Project namespace for resource isolation, similar to Kubernetes namespace
**Scenarios**: 5
**Priority**: High

---

## Background

The orchestrator now supports a Project concept to constrain resource naming spaces, similar to Kubernetes namespace. A project can contain multiple workspaces, and workspaces within the same project can share project-level workflows and agents.

Entry point: `orchestrator <command>` (CLI)

### Config Model

```yaml
projects:
  my-project:
    description: "My AI Project"
    workspaces:
      dev:
        root_path: /path/to/dev
      staging:
        root_path: /path/to/staging
    agents:
      my-agent:
        capabilities: [qa]
        templates:
          qa: "echo '{\"confidence\":0.9,\"quality_score\":0.86,\"artifacts\":[{\"kind\":\"analysis\",\"findings\":[{\"title\":\"qa-project\",\"description\":\"project qa\",\"severity\":\"info\"}]}]}'"
    workflows:
      my-workflow:
        steps: [...]
```

Resource resolution:
- All resources are **project-scoped** — `--project` resolves against `config.projects[<name>]` only.
- There is **no fallback** to top-level global config. If the project doesn't exist in `config.projects`, the command fails with `"project not found"`.
- The built-in `default` project is only an identifier convention; the project entry must still exist in `config.projects` before project-scoped commands succeed.

---

## Scenario 1: Task Creation with Project

### Preconditions

- Repository root is the current working directory.
- Rust toolchain is available.

### Goal

Verify that `apply_to_project` correctly routes resources into project scope and stores project_id, via unit tests and code review.

### Steps

1. Run apply_to_project unit tests covering project routing:
   ```bash
   cargo test -p agent-orchestrator --lib -- apply_to_project --nocapture
   ```

2. Verify project_id is stored in task creation path:
   ```bash
   rg -n "project_id\|project_name" core/src/task_repository/ core/src/service/task.rs
   ```

### Expected

- `apply_to_project_auto_creates_project_entry` passes — project created on first apply
- `apply_to_project_routes_agent_to_project_scope` passes — agents routed correctly
- `apply_to_project_routes_workspace_to_project_scope` passes — workspaces routed correctly
- `apply_to_project_routes_workflow_to_project_scope` passes — workflows routed correctly
- Code review confirms `project_id` is persisted in tasks table

---

## Scenario 2: Explicit Workflow Resolution Inside a Project

### Preconditions

- Repository root is the current working directory.
- Rust toolchain is available.

### Goal

Verify workflows are resolved from project scope (no fallback to global), via unit tests and code review.

### Steps

1. Run apply_to_project tests covering workflow routing:
   ```bash
   cargo test -p agent-orchestrator --lib -- apply_to_project_routes_workflow --nocapture
   ```

2. Code review: confirm project-scoped workflow resolution has no global fallback:
   ```bash
   rg -n "project.*workflow\|resolve.*workflow\|no fallback" core/src/resource/apply.rs core/src/service/task.rs
   ```

3. Run CRD scope serde tests:
   ```bash
   cargo test -p orchestrator-config --lib -- crd_scope --nocapture
   ```

### Expected

- `apply_to_project_routes_workflow_to_project_scope` passes — workflow routed to project
- Code review confirms no global fallback for project-scoped commands
- 3 CRD scope tests pass (default_is_cluster, serde_round_trip, deserializes_from_snake_case)

---

## Scenario 3: Project-Level Workspace Resolution

### Preconditions

- Repository root is the current working directory.
- Rust toolchain is available.

### Goal

Verify workspace is routed to project scope and that non-existent projects produce clear errors, via unit tests and code review.

### Steps

1. Run workspace routing unit test:
   ```bash
   cargo test -p agent-orchestrator --lib -- apply_to_project_routes_workspace --nocapture
   ```

2. Code review: verify project-not-found error handling:
   ```bash
   rg -n "project not found\|project.*not.*exist\|ProjectNotFound" core/src/resource/ core/src/service/
   ```

3. Run unchanged detection test (ensures identical re-apply is idempotent):
   ```bash
   cargo test -p agent-orchestrator --lib -- apply_to_project_returns_unchanged --nocapture
   ```

### Expected

- `apply_to_project_routes_workspace_to_project_scope` passes
- Code review confirms `"project not found"` error for non-existent projects
- `apply_to_project_returns_unchanged_for_identical` passes — idempotent apply

---

## Scenario 4: CLI Project Flag

### Preconditions

- Repository root is the current working directory.
- Rust toolchain is available.

### Goal

Verify `--project` flag is defined in CLI and routed through to apply/task commands, via code review and unit tests.

### Steps

1. Code review: verify `--project` flag is defined in CLI argument parsing:
   ```bash
   rg -n "project\|--project" crates/cli/src/commands/task.rs crates/cli/src/commands/apply.rs | head -20
   ```

2. Run apply_to_project routing test for runtime policy:
   ```bash
   cargo test -p agent-orchestrator --lib -- apply_to_project_routes_runtime_policy --nocapture
   ```

3. Verify all 6 apply_to_project tests pass:
   ```bash
   cargo test -p agent-orchestrator --lib -- apply_to_project --nocapture
   ```

### Expected

- Code review confirms `--project` is a recognized CLI argument
- `apply_to_project_routes_runtime_policy_through_generic_path` passes
- All 6 apply_to_project tests pass

---

## Scenario 5: Multi-Project Isolation

### Preconditions

- Two or more projects configured (if testing custom config)

### Goal

Validate that project resources are isolated from each other.

### Steps

1. Validate the two-projects fixture (projects define their own workspaces and agents as separate project-scoped groups):
   ```bash
   orchestrator manifest validate -f fixtures/manifests/bundles/two-projects.yaml
   ```

### Expected

- Validation passes (exit code 0) — `"Manifest is valid"`
- Two project-tagged resource groups (project-a, project-b) are correctly parsed and accepted structurally
- Each project has its own workspace, agent, and workflow — no cross-project leakage

> **Note**: The self-referential safety policy is only triggered when `self_referential: true`
> is explicitly set in the workspace spec. `root_path: "."` alone does **not** trigger the
> policy. The `two-projects.yaml` fixture does not set `self_referential: true`, so validation
> passes. This is correct behavior — the scenario validates multi-project structural isolation,
> not self-referential safety.

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| SELF_REF_POLICY_VIOLATION on validate | Workspace has `self_referential: true` without safety settings | Add `checkpoint_strategy`, `auto_rollback`, and `self_test` step to the workflow, or remove `self_referential: true` if not needed |
| Validation passes but expected failure | `root_path: "."` does not auto-trigger self-referential policy; only explicit `self_referential: true` does | Set `self_referential: true` in workspace spec if self-referential safety checks are intended |

---

## General Scenario: Explicit Project Entry Exists

### Goal

Validate that `apply_to_project` auto-creates project entries when they don't exist.

### Steps

1. Run the auto-create unit test:
   ```bash
   cargo test -p agent-orchestrator --lib -- apply_to_project_auto_creates_project_entry --nocapture
   ```

2. Code review: verify project auto-creation in apply path:
   ```bash
   rg -n "auto.*create.*project\|ensure.*project\|create_project" core/src/resource/apply.rs
   ```

### Expected

- `apply_to_project_auto_creates_project_entry` passes — project created on first `--project` use
- Code review confirms projects are created lazily via apply

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Task Creation with Project | ✅ PASS | 2026-03-20 | Claude | All 6 apply_to_project tests pass; project_id persisted in task_ops.rs:182 |
| 2 | Explicit Workflow Resolution | ✅ PASS | 2026-03-20 | Claude | Workflow routed to project scope via apply_to_store; 3 CRD scope tests pass |
| 3 | Project-Level Workspace Resolution | ✅ PASS | 2026-03-20 | Claude | Workspace routed to project scope; idempotent apply passes; "project not found" error confirmed |
| 4 | CLI Project Flag | ✅ PASS | 2026-03-20 | Claude | --project flag confirmed in CLI (task.rs:15,22,33,43); all 6 apply_to_project tests pass |
| 5 | Multi-Project Isolation | ✅ PASS | 2026-03-18 | Claude | `two-projects.yaml` validates with exit 0; project-a and project-b each have own workspace/agent/workflow |
| G1 | Explicit Project Entry Exists | ✅ PASS | 2026-03-20 | Claude | Auto-create test passes; ensure_project() called at apply.rs:26 |
