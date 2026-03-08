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
defaults:
  project: default
  workspace: default
  workflow: qa_only

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
- There is **no fallback** to global config. If the project doesn't exist in `config.projects`, the command fails with `"project not found"`.
- The `defaults.project` field names the default project but does **not** create an implicit entry; it must be explicitly created via `apply --project <name>`.

---

## Scenario 1: Task Creation with Project

### Preconditions

- Orchestrator binary built at `./target/release/orchestrator`
- Default project/workspace/workflow already initialized in SQLite config
- Use `orchestrator` CLI for all commands

### Goal

Validate task creation with explicit project specification stores project_id in database.

### Steps

1. Create task with explicit project:
   ```bash
   ./target/release/orchestrator task create \
     --name "test-project-task" \
     --goal "Test project namespace" \
     --project default \
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
- Task details show project_id = "default"
- Database query returns project_id column with value "default"

---

## Scenario 2: Project Fallback - Global Workflow

### Preconditions

- At least one workflow exists in the global config
- `defaults.workflow` is set (auto-filled to `qa_only` if present, or the first workflow alphabetically)
- Default project exists without custom workflows
- Use `orchestrator` CLI for all commands

### Goal

Validate that when project doesn't define a workflow, the `defaults.workflow` from the global config is used.

### Steps

1. Check current default workflow:
   ```bash
   orchestrator manifest export | grep 'workflow:'
   ```

2. Create task without explicit workflow (should use default):
   ```bash
   orchestrator task create \
     --name "test-fallback-workflow" \
     --goal "Test fallback" \
     --project default \
     --no-start
   ```

3. Verify task uses the default workflow:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT workflow_id FROM tasks WHERE name = 'test-fallback-workflow';"
   ```

### Expected

- Task created successfully
- workflow_id matches the value shown in `defaults.workflow` (typically `qa_only`)

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

1. Validate the two-projects fixture (projects define their own workspaces and agents; global workspaces/agents are empty):
   ```bash
   orchestrator manifest validate -f fixtures/manifests/bundles/two-projects.yaml
   ```

### Expected

- Config validates successfully (exit code 0)
- Two project-tagged resource groups (project-a, project-b) are accepted in the manifest
- Validator accepts project-tagged workspaces via `metadata.project`
- Validator accepts project-tagged agents and workflow capability references

---

## General Scenario: Config Defaults Project Field

### Goal

Validate that defaults.project is required and defaults to "default".

### Steps

1. Check current config:
   ```bash
   ./target/release/orchestrator manifest export | grep -A5 "defaults:"
   ```

2. Verify project field exists in defaults

### Expected

- defaults.project field is present
- Default value is "default"

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
