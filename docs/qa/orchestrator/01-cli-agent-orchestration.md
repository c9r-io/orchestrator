# Orchestrator - CLI Agent Orchestration Testing

**Module**: orchestrator
**Scope**: Validate CLI interface and agent orchestration with mock bash commands
**Scenarios**: 5
**Priority**: High

---

## Background

The Agent Orchestrator CLI provides kubectl-like command interface for task orchestration. This document tests the CLI interface using simple bash commands (echo, sleep) as mock agents to validate the full agent orchestration pipeline.

Entry point: `./orchestrator/scripts/orchestrator.sh <command>` or `./orchestrator/src-tauri/target/release/agent-orchestrator <command>`

### Test Agent Configuration

For testing purposes, use mock agents with bash commands:

```yaml
agents:
  mock_echo:
    templates:
      qa: "echo 'qa-phase: {rel_path}'"
      fix: "echo 'fix-phase: {ticket_paths}'"
      retest: "echo 'retest-phase: {rel_path}'"
      init_once: "echo 'init-phase'"
      loop_guard: "echo 'continue'"
  mock_sleep:
    templates:
      qa: "sleep 0.1 && echo 'qa-complete'"
      fix: "sleep 0.1 && echo 'fix-complete'"
      retest: "sleep 0.1 && echo 'retest-complete'"
```

---

## Scenario 1: CLI Task Lifecycle - Create and Start

### Preconditions

- Orchestrator binary built and available at `./orchestrator/src-tauri/target/release/agent-orchestrator`
- Test workspace exists with QA targets configured
- Mock agent configured in `config/default.yaml`

### Goal

Validate task creation and execution with mock bash agent completes successfully.

### Steps

1. Create a new task with mock echo agent:
   ```bash
   cd /Volumes/Yotta/ai_native_sdlc/orchestrator
   ./src-tauri/target/release/agent-orchestrator task create \
     --name "test-task-echo" \
     --goal "Test agent orchestration" \
     --workspace default \
     --workflow qa_only
   ```

2. List tasks to verify creation:
   ```bash
   ./src-tauri/target/release/agent-orchestrator task list
   ```

3. Get task details:
   ```bash
   ./src-tauri/target/release/agent-orchestrator task info {task_id}
   ```

### Expected

- Task created successfully with status "pending"
- Task list shows the new task
- Task details show correct name, goal, workspace, and workflow

---

## Scenario 2: CLI Task List with Status Filter

### Preconditions

- At least one task exists in the system

### Goal

Validate task list filtering by status works correctly.

### Steps

1. Create multiple tasks with different scenarios:
   ```bash
   ./src-tauri/target/release/agent-orchestrator task create --name "task-1" --goal "test1" --no-start
   ./src-tauri/target/release/agent-orchestrator task create --name "task-2" --goal "test2" --no-start
   ```

2. List all tasks:
   ```bash
   ./src-tauri/target/release/agent-orchestrator task list
   ```

3. Filter by status (if tasks exist):
   ```bash
   ./src-tauri/target/release/agent-orchestrator task list --status pending
   ./src-tauri/target/release/agent-orchestrator task list --status completed
   ```

4. Test output formats:
   ```bash
   ./src-tauri/target/release/agent-orchestrator task list -o json
   ./src-tauri/target/release/agent-orchestrator task list -o yaml
   ```

### Expected

- All tasks appear in default list
- Status filter correctly filters tasks
- JSON/YAML output contains proper structure

---

## Scenario 3: CLI Workspace and Config Management

### Preconditions

- Orchestrator initialized with default workspace

### Goal

Validate workspace listing and configuration viewing work correctly.

### Steps

1. List workspaces:
   ```bash
   ./src-tauri/target/release/agent-orchestrator workspace list
   ```

2. Get workspace details:
   ```bash
   ./src-tauri/target/release/agent-orchestrator workspace info default
   ```

3. View current configuration:
   ```bash
   ./src-tauri/target/release/agent-orchestrator config view
   ./src-tauri/target/release/agent-orchestrator config view -o json
   ```

4. List available workflows:
   ```bash
   ./src-tauri/target/release/agent-orchestrator config list-workflows
   ```

5. List available agents:
   ```bash
   ./src-tauri/target/release/agent-orchestrator config list-agents
   ```

### Expected

- Workspace list shows all configured workspaces
- Workspace details show root_path, qa_targets, ticket_dir
- Config view shows full configuration in YAML/JSON format
- Workflow list shows all configured workflows
- Agent list shows all agents with their phase templates

---

## Scenario 4: CLI Apply with Dry-Run

### Preconditions

- Valid YAML manifest file available

### Goal

Validate apply command with dry-run mode doesn't persist changes.

### Steps

1. Create a test workspace manifest:
   ```bash
   cat > /tmp/test-workspace.yaml << 'EOF'
   apiVersion: orchestrator.dev/v1
   kind: Workspace
   metadata:
     name: test-workspace
   spec:
     root_path: /tmp/test-ws
     qa_targets:
       - docs/qa
     ticket_dir: docs/ticket
   EOF
   ```

2. Apply with dry-run (should not persist):
   ```bash
   ./src-tauri/target/release/agent-orchestrator apply -f /tmp/test-workspace.yaml --dry-run
   ```

3. Verify workspace was NOT created:
   ```bash
   ./src-tauri/target/release/agent-orchestrator workspace list
   ```

4. Apply without dry-run:
   ```bash
   ./src-tauri/target/release/agent-orchestrator apply -f /tmp/test-workspace.yaml
   ```

5. Verify workspace WAS created:
   ```bash
   ./src-tauri/target/release/agent-orchestrator workspace info test-workspace
   ```

### Expected

- Dry-run shows what would be created but doesn't persist
- After dry-run, workspace list doesn't contain test-workspace
- After real apply, workspace info shows the new workspace

---

## Scenario 5: CLI Config Validate

### Preconditions

- None (tests configuration validation)

### Goal

Validate configuration validation catches invalid configurations.

### Steps

1. Create invalid config (empty root_path):
   ```bash
   cat > /tmp/invalid-config.yaml << 'EOF'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   resume:
     auto: true
   defaults:
     workspace: default
     workflow: qa_only
   workspaces:
     invalid-ws:
       root_path: ""
       qa_targets: []
       ticket_dir: docs/ticket
   agents: {}
   agent_groups: {}
   workflows:
     test:
       steps:
         - id: qa
           type: qa
           enabled: true
           agent_group_id: none
       loop:
         mode: once
       finalize:
         rules: []
   EOF
   ```

2. Validate the invalid config:
   ```bash
   ./src-tauri/target/release/agent-orchestrator config validate /tmp/invalid-config.yaml
   ```

3. Create valid config:
   ```bash
   cat > /tmp/valid-config.yaml << 'EOF'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   resume:
     auto: true
   defaults:
     workspace: default
     workflow: qa_only
   workspaces:
     default:
       root_path: /tmp/test
       qa_targets:
         - docs/qa
       ticket_dir: docs/ticket
   agents: {}
   agent_groups: {}
   workflows:
     qa_only:
       steps:
         - id: qa
           type: qa
           enabled: false
       loop:
         mode: once
       finalize:
         rules: []
   EOF
   ```

4. Validate the valid config:
   ```bash
   ./src-tauri/target/release/agent-orchestrator config validate /tmp/valid-config.yaml
   ```

### Expected

- Invalid config validation fails with error message
- Valid config validation succeeds and shows normalized YAML

---

## General Scenario: Task Delete with Force

### Preconditions

- At least one task exists

### Goal

Validate task deletion requires --force flag.

### Steps

1. Create a task:
   ```bash
   ./src-tauri/target/release/agent-orchestrator task create --name "delete-me" --goal "test" --no-start
   ```

2. Try to delete without force (should prompt):
   ```bash
   ./src-tauri/target/release/agent-orchestrator task delete {task_id}
   ```

3. Delete with force:
   ```bash
   ./src-tauri/target/release/agent-orchestrator task delete {task_id} --force
   ```

4. Verify deletion:
   ```bash
   ./src-tauri/target/release/agent-orchestrator task list
   ```

### Expected

- Without --force, command exits without deleting
- With --force, task is deleted
- Deleted task no longer appears in list

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | CLI Task Lifecycle - Create and Start | ☐ | | | |
| 2 | CLI Task List with Status Filter | ☐ | | | |
| 3 | CLI Workspace and Config Management | ☐ | | | |
| 4 | CLI Apply with Dry-Run | ☐ | | | |
| 5 | CLI Config Validate | ☐ | | | |
| G1 | Task Delete with Force | ☐ | | | |
