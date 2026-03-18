---
self_referential_safe: false
self_referential_safe_scenarios: [S5]
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

- Orchestrator binary built at `./target/release/orchestrator`
- A project with workspace and workflow must exist. The `default` project is only
  a naming convention — it must be explicitly created via `orchestrator apply`.
  Use a dedicated QA project to avoid depending on pre-existing global state:
  ```bash
  orchestrator init
  orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project qa-scenario1
  ```
- Use `orchestrator` CLI for all commands

### Goal

Validate task creation with explicit project specification stores project_id in database.

### Steps

1. Create task with explicit project:
   ```bash
   ./target/release/orchestrator task create \
     --name "test-project-task" \
     --goal "Test project namespace" \
     --project qa-scenario1 \
     --workspace default \
     --workflow qa_fix_retest \
     --no-start
   ```

2. Get task details to verify project_id:
   ```bash
   ./target/release/orchestrator task info {task_id}
   ```

3. Query database for project_id:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT id, project_id, workspace_id, workflow_id FROM tasks WHERE name = 'test-project-task';"
   ```

### Expected

- Task created successfully
- Task details show project_id = "qa-scenario1"
- Database query returns project_id column with value "qa-scenario1"

---

## Scenario 2: Explicit Workflow Resolution Inside a Project

### Preconditions

- A project exists in `config.projects`
- The project defines at least one workflow
- Use `orchestrator` CLI for all commands

### Goal

Validate that task creation resolves workflows from the selected project scope.

### Steps

1. Check project workflows:
   ```bash
   orchestrator get workflows --project default
   ```

2. Create task with explicit workflow in that project:
   ```bash
   orchestrator task create \
     --name "test-fallback-workflow" \
     --goal "Test project workflow resolution" \
     --project default \
     --workflow qa_only \
     --no-start
   ```

3. Verify task uses the selected project workflow:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT workflow_id FROM tasks WHERE name = 'test-fallback-workflow';"
   ```

### Expected

- Task created successfully
- workflow_id matches the workflow selected in the project-scoped command (for example `qa_only`)

---

## Scenario 3: Project-Level Workspace Resolution

### Preconditions

- An explicit project has been created via `apply --project` (the project must exist in `config.projects`).
- Use `orchestrator` CLI for all commands.

### Goal

Validate workspace resolution within an explicitly-created project context, and that referencing a non-existent project returns a clear error.

### Steps

1. Create project resources:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project ws-test
   ```

2. List workspaces scoped to the project:
   ```bash
   orchestrator get workspaces --project ws-test
   ```

3. Verify non-existent project returns clear error:
   ```bash
   orchestrator get workspaces --project nonexistent-project
   ```

4. Create task in the project without explicit workspace:
   ```bash
   orchestrator task create \
     --name "test-project-workspace-resolution" \
     --goal "verify project workspace resolution" \
     --project ws-test \
     --workflow qa_only \
     --no-start
   ```

### Expected

- Step 2: workspace list returns only the project's workspaces.
- Step 3: command fails with `"project not found: nonexistent-project"`.
- Step 4: task is created successfully; workspace is resolved from project config.

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| `project not found: default` | "default" project was never explicitly created via `apply --project default` | Apply resources with `--project default` first, or use a project name that has been applied |
| Empty workspace list | Resources were applied globally (without `--project`) | Re-apply manifests with `--project <name>` |

---

## Scenario 4: CLI Project Flag

### Preconditions

- Orchestrator CLI available
- Use `orchestrator` CLI for all commands

### Goal

Validate CLI --project flag is recognized and passed correctly.

### Steps

1. Test project flag with help:
   ```bash
   ./target/release/orchestrator task create --help
   ```

2. Create task with project flag:
   ```bash
   ./target/release/orchestrator task create \
     --project default \
     --name "test-cli-project-flag" \
     --goal "Test CLI flag"
   ```

3. Verify project was stored:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT project_id FROM tasks WHERE name = 'test-cli-project-flag';"
   ```

### Expected

- --project flag is recognized (no "unknown argument" error)
- project_id = "default" in database

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

Validate that project-scoped commands operate against an explicit project entry.

### Steps

1. Check current config:
   ```bash
   ./target/release/orchestrator manifest export | grep -A8 "^projects:"
   ```

2. Verify the target project exists under `projects:`

### Expected

- The exported manifest contains a concrete project entry under `projects:`
- Project-scoped commands should target that explicit project entry

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Task Creation with Project | ☐ | | | |
| 2 | Project Fallback - Global Workflow | ☐ | | | |
| 3 | Project-Level Workspace Resolution | ☐ | | | |
| 4 | CLI Project Flag | ☐ | | | |
| 5 | Multi-Project Isolation | ☐ | | | |
| G1 | Config Defaults Project Field | ☐ | | | |
