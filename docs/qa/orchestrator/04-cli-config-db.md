# Orchestrator - CLI Config and Database

**Module**: orchestrator
**Scope**: Validate configuration management and database operations
**Scenarios**: 4
**Priority**: High

---

## Background

This document tests configuration update and database management commands.

Entry points: 
- `orchestrator config set <file>`
- `orchestrator db reset`

---

## Scenario 1: Config Set - Update Configuration

### Preconditions

- Valid configuration YAML file available

### Steps

1. Create updated configuration:
   ```bash
   cat > /tmp/updated-config.yaml << 'EOF'
   runner:
     shell: /bin/zsh
     shell_arg: -lc
   resume:
     auto: true
   defaults:
     workspace: default
     workflow: qa_only
   workspaces:
      default:
        root_path: /path/to/project
        qa_targets:
          - docs/qa
        ticket_dir: docs/ticket
    agents:
      mock:
        metadata:
          name: mock
        capabilities:
        - qa
        templates:
          qa: "echo 'test'"
    workflows:
      qa_only:
        steps:
          - id: qa
            required_capability: qa
            enabled: true
            repeatable: false
        loop:
          mode: once
        finalize:
          rules: []
    EOF
    ```

2. Apply configuration:
   ```bash
   orchestrator config set /tmp/updated-config.yaml
   ```

3. Verify configuration was updated:
   ```bash
   orchestrator config view
   orchestrator config list-workflows
   ```

### Expected

- Configuration is updated successfully
- New workflows and agents are visible

---

## Scenario 2: Config Set - Invalid Configuration

### Preconditions

- None

### Steps

1. Try to set invalid configuration:
   ```bash
   cat > /tmp/invalid-config.yaml << 'EOF'
   runner:
     shell: /bin/bash
   defaults:
     workspace: nonexistent
   workspaces: {}
   agents: {}
   workflows: {}
   EOF
   orchestrator config set /tmp/invalid-config.yaml
   ```

### Expected

- Error message about invalid configuration
- Configuration is not changed

---

## Scenario 3: Config Set - Add New Workspace

### Preconditions

- Existing workspace configured

### Steps

1. Add new workspace via config:
   ```bash
   cat > /tmp/add-workspace.yaml << 'EOF'
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
        root_path: /path/to/project
        qa_targets:
          - docs/qa
        ticket_dir: docs/ticket
      new-workspace:
        root_path: /path/to/new
        qa_targets:
          - docs/qa
        ticket_dir: docs/ticket
    agents:
      mock:
        metadata:
          name: mock
        capabilities:
        - qa
        templates:
          qa: "echo test"
    agent_groups: {}
    workflows:
      qa_only:
        steps:
          - id: qa
            required_capability: qa
            enabled: false
            repeatable: false
        loop:
          mode: once
        finalize:
          rules: []
    EOF
    orchestrator config set /tmp/add-workspace.yaml
    ```

2. Verify new workspace:
   ```bash
   orchestrator workspace list
   orchestrator workspace info new-workspace
   ```

### Expected

- New workspace is added
- Existing workspace is preserved

---

## Scenario 4: Database Reset

### Preconditions

- Tasks exist in database

### Steps

1. Check current tasks:
   ```bash
   orchestrator task list
   ```

2. Try reset without force (should prompt):
   ```bash
   orchestrator db reset
   ```

3. Reset with force:
   ```bash
   orchestrator db reset --force
   ```

4. Verify database is reset:
   ```bash
   orchestrator task list
   orchestrator workspace list Expected

- Without
   ```

### --force, command prompts for confirmation
- With --force, database is cleared
- Task and workspace data is removed

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Config Set - Update | ☐ | | | |
| 2 | Config Set - Invalid | ☐ | | | |
| 3 | Config Set - Add Workspace | ☐ | | | |
| 4 | Database Reset | ☐ | | | |
