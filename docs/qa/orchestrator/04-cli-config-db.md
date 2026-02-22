# Orchestrator - CLI Config and Database

**Module**: orchestrator
**Scope**: Validate configuration update and database reset flows
**Scenarios**: 4
**Priority**: High

---

## Background

This document validates config lifecycle commands and database reset behavior.

Entry points:
- `./scripts/orchestrator.sh config <command>`
- `./scripts/orchestrator.sh db reset`

---

## Scenario 1: Config Set - Update Configuration

### Preconditions

- Runtime initialized and config bootstrapped (see QA doc `01-cli-agent-orchestration.md` Scenario 1 preconditions).

### Steps

1. Create a valid config file:
   ```bash
   cat > /tmp/updated-config.yaml << 'EOF2'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   resume:
     auto: false
   defaults:
     workspace: default
     workflow: qa_only
   workspaces:
     default:
       root_path: .
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
   workflows:
     qa_only:
       steps:
         - id: qa
           required_capability: qa
           enabled: true
       loop:
         mode: once
         guard:
           enabled: false
           stop_when_no_unresolved: false
       finalize:
         rules: []
   EOF2
   ```

2. Apply config:
   ```bash
   ./scripts/orchestrator.sh config set /tmp/updated-config.yaml
   ```

3. Verify:
   ```bash
   ./scripts/orchestrator.sh config view -o yaml
   ./scripts/orchestrator.sh config list-workflows
   ./scripts/orchestrator.sh config list-agents
   ```

### Expected

- Config update succeeds.
- Workflow and agent lists include newly configured entries.

---

## Scenario 2: Config Set - Invalid Configuration

### Preconditions

- Runtime initialized and config bootstrapped (see QA doc `01-cli-agent-orchestration.md` Scenario 1 preconditions).

### Steps

1. Create invalid config:
   ```bash
   cat > /tmp/invalid-config.yaml << 'EOF2'
   runner:
     shell: /bin/bash
   defaults:
     workspace: nonexistent
     workflow: missing
   workspaces: {}
   agents: {}
   workflows: {}
   EOF2
   ```

2. Apply invalid config:
   ```bash
   ./scripts/orchestrator.sh config set /tmp/invalid-config.yaml
   ```

### Expected

- Command fails with validation error.
- Existing runtime config remains unchanged.

---

## Scenario 3: Config Set - Add New Workspace

### Preconditions

- Runtime initialized and config bootstrapped (see QA doc `01-cli-agent-orchestration.md` Scenario 1 preconditions).
- Current config contains `default` workspace.

### Steps

1. Export current config:
   ```bash
   ./scripts/orchestrator.sh config export -f /tmp/base-config.yaml
   ```

2. Create new config that adds a workspace:
   ```bash
   cat > /tmp/add-workspace.yaml << 'EOF2'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   resume:
     auto: false
   defaults:
     workspace: default
     workflow: qa_only
   workspaces:
     default:
       root_path: .
       qa_targets: [docs/qa]
       ticket_dir: docs/ticket
     new-workspace:
       root_path: /tmp/new-workspace
       qa_targets: [docs/qa]
       ticket_dir: docs/ticket
   agents:
     mock:
       metadata:
         name: mock
       capabilities: [qa]
       templates:
         qa: "echo test"
   workflows:
     qa_only:
       steps:
         - id: qa
           required_capability: qa
           enabled: true
       loop:
         mode: once
         guard:
           enabled: false
           stop_when_no_unresolved: false
       finalize:
         rules: []
   EOF2
   ./scripts/orchestrator.sh config set /tmp/add-workspace.yaml
   ```

3. Verify workspace list:
   ```bash
   ./scripts/orchestrator.sh workspace list
   ./scripts/orchestrator.sh workspace info new-workspace
   ```

### Expected

- New workspace is persisted.
- Existing workspace remains available.

---

## Scenario 4: Database Reset

### Preconditions

- Tasks/config exist in database.

### Steps

1. Check current tasks:
   ```bash
   ./scripts/orchestrator.sh task list
   ```

2. Reset database with force:
   ```bash
   ./scripts/orchestrator.sh db reset --force
   ```

3. Verify reset state:
   ```bash
   ./scripts/orchestrator.sh task list
   ```

### Expected

- Reset command succeeds with `--force`.
- Existing task records are cleared.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Config Set - Update Configuration | ☐ | | | |
| 2 | Config Set - Invalid Configuration | ☐ | | | |
| 3 | Config Set - Add New Workspace | ☐ | | | |
| 4 | Database Reset | ☐ | | | |
