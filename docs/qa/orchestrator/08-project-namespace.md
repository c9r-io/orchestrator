# Orchestrator - Project Namespace

**Module**: orchestrator
**Scope**: Project namespace for resource isolation, similar to Kubernetes namespace
**Scenarios**: 5
**Priority**: High

---

## Background

The orchestrator now supports a Project concept to constrain resource naming spaces, similar to Kubernetes namespace. A project can contain multiple workspaces, and workspaces within the same project can share project-level workflows and agents.

Entry point: `./core/target/release/agent-orchestrator <command>`

### Config Model

```yaml
defaults:
  project: default
  workspace: default
  workflow: test_capability

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

Resource resolution priority:
1. Project-level resources first
2. Fall back to global resources if not found in project

---

## Scenario 1: Task Creation with Project

### Preconditions

- Orchestrator binary built at `./core/target/release/agent-orchestrator`
- Default project/workspace/workflow already initialized in SQLite config
- Use `./scripts/orchestrator.sh` (wrapper) for all commands, not the direct binary

### Goal

Validate task creation with explicit project specification stores project_id in database.

### Steps

1. Create task with explicit project:
   ```bash
   ./core/target/release/agent-orchestrator task create \
     --name "test-project-task" \
     --goal "Test project namespace" \
     --project default \
     --workspace default \
     --workflow qa_fix_retest \
     --no-start
   ```

2. Get task details to verify project_id:
   ```bash
   ./core/target/release/agent-orchestrator task info {task_id}
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

- Global workflow "test_capability" exists in config
- Default project exists without custom workflows
- Use `./scripts/orchestrator.sh` (wrapper) for all commands, not the direct binary

### Goal

Validate that when project doesn't define a workflow, global workflow is used.

### Steps

1. Create task without explicit workflow (should use default):
   ```bash
   ./core/target/release/agent-orchestrator task create \
     --name "test-fallback-workflow" \
     --goal "Test fallback" \
     --project default
   ```

2. Verify task uses global workflow:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT workflow_id FROM tasks WHERE name = 'test-fallback-workflow';"
   ```

### Expected

- Task created successfully
- workflow_id is "test_capability" (from global config)

---

## Scenario 3: Project-Level Workspace Resolution

### Preconditions

- Project with multiple workspaces configured
- Use `./scripts/orchestrator.sh` (wrapper) for all commands, not the direct binary

### Goal

Validate workspace resolution within project context.

### Steps

1. List workspaces via API:
   ```bash
   ./core/target/release/agent-orchestrator workspace list
   ```

2. Check get_create_task_options returns project workspaces:
   ```bash
   # This tests the API response structure
   curl -s http://localhost:1423/api/task-options | jq '.'
   ```

### Expected

- Workspace list shows workspaces from default project
- Project workspaces are included in available options

---

## Scenario 4: CLI Project Flag

### Preconditions

- Orchestrator CLI available
- Use `./scripts/orchestrator.sh` (wrapper) for all commands, not the direct binary

### Goal

Validate CLI --project flag is recognized and passed correctly.

### Steps

1. Test project flag with help:
   ```bash
   ./core/target/release/agent-orchestrator task create --help
   ```

2. Create task with project flag:
   ```bash
   ./core/target/release/agent-orchestrator task create \
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
   ./scripts/orchestrator.sh config validate fixtures/two-projects.yaml
   ```

### Expected

- Config validates successfully (exit code 0)
- Two projects (project-a, project-b) are recognized with project-level workspaces and agents
- Validator resolves project-level workspaces for `defaults.workspace` reference
- Validator resolves project-level agents for workflow step capability matching

---

## General Scenario: Config Defaults Project Field

### Goal

Validate that defaults.project is required and defaults to "default".

### Steps

1. Check current config:
   ```bash
   ./core/target/release/agent-orchestrator config view | grep -A5 "defaults:"
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
