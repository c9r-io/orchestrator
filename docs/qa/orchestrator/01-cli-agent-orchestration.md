# Orchestrator - CLI Agent Orchestration Testing

**Module**: orchestrator
**Scope**: Validate CLI interface and agent orchestration with structured JSON mock outputs
**Scenarios**: 5
**Priority**: High

---

## Background

The Agent Orchestrator CLI provides kubectl-like command interface for task orchestration. This document tests the CLI interface using structured JSON mock outputs (including sleep-delayed outputs) to validate the full agent orchestration pipeline under strict phase validation.

Entry point: `orchestrator <command>` (CLI) or `orchestratord` (daemon)

### Test Agent Configuration

For testing purposes, use mock agents with structured JSON outputs:

```yaml
agents:
  mock_echo:
    metadata:
      name: mock_echo
    capabilities:
    - qa
    - fix
    - retest
    templates:
      qa: "echo '{\"confidence\":0.93,\"quality_score\":0.9,\"artifacts\":[{\"kind\":\"analysis\",\"findings\":[{\"title\":\"qa-pass\",\"description\":\"qa passed for {rel_path}\",\"severity\":\"info\"}]}]}'"
      fix: "echo '{\"confidence\":0.84,\"quality_score\":0.8,\"artifacts\":[{\"kind\":\"code_change\",\"files\":[\"{rel_path}\"]}]}'"
      retest: "echo '{\"confidence\":0.9,\"quality_score\":0.88,\"artifacts\":[{\"kind\":\"test_result\",\"passed\":1,\"failed\":0}]}'"
  mock_sleep:
    metadata:
      name: mock_sleep
    capabilities:
    - qa
    - fix
    templates:
      qa: "sleep 0.1 && echo '{\"confidence\":0.9,\"quality_score\":0.86,\"artifacts\":[{\"kind\":\"analysis\",\"findings\":[{\"title\":\"qa-complete\",\"description\":\"qa complete\",\"severity\":\"info\"}]}]}'"
      fix: "sleep 0.1 && echo '{\"confidence\":0.8,\"quality_score\":0.76,\"artifacts\":[{\"kind\":\"code_change\",\"files\":[\"sleep-fix.out\"]}]}'"
```

---

## Scenario 1: CLI Task Lifecycle - Create and Start

### Preconditions

- Orchestrator binary built and available at `./target/release/orchestrator`
- Runtime initialized and mock config applied into SQLite:
   ```bash
   QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
   orchestrator apply -f fixtures/manifests/bundles/output-formats.yaml
   orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   orchestrator apply -f fixtures/manifests/bundles/output-formats.yaml --project "${QA_PROJECT}"
   ```

### Goal

Validate task creation and execution with mock bash agent completes successfully.

### Steps

1. Create a new task with mock echo agent:
   ```bash
   ./target/release/orchestrator task create \
     --name "test-task-echo" \
     --goal "Test agent orchestration" \
     --project "${QA_PROJECT}" \
     --workspace default \
     --workflow qa_only
   ```

2. List tasks to verify creation:
   ```bash
   ./target/release/orchestrator task list
   ```

3. Get task details:
   ```bash
   ./target/release/orchestrator task info {task_id}
   ```

### Expected

- Task created successfully with status "created" (tasks start in "created" status and only transition to "pending" when enqueued via `task start`)
- Task list shows the new task
- Task details show correct name, goal, workspace, and workflow

---

## Scenario 2: CLI Task List with Status Filter

### Preconditions

- Project scaffold is freshly recreated before running this scenario: `delete project/<name> --force` + `rm -rf "workspace/${QA_PROJECT}"` + `apply -f <fixture> --project`
- At least one task exists in the system

### Goal

Validate task list filtering by status works correctly.

### Steps

1. Create multiple tasks with different scenarios:
   ```bash
   ./target/release/orchestrator task create --project "${QA_PROJECT}" --name "task-1" --goal "test1" --no-start
   ./target/release/orchestrator task create --project "${QA_PROJECT}" --name "task-2" --goal "test2" --no-start
   ```

2. List all tasks:
   ```bash
   ./target/release/orchestrator task list
   ```

3. Filter by status (if tasks exist):
   ```bash
   ./target/release/orchestrator task list --status created
   ./target/release/orchestrator task list --status completed
   ```

4. Test output formats:
   ```bash
   ./target/release/orchestrator task list -o json
   ./target/release/orchestrator task list -o yaml
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
   ./target/release/orchestrator get workspaces
   ```

2. Get workspace details:
   ```bash
   ./target/release/orchestrator describe workspace default
   ```

3. View current configuration:
   ```bash
   ./target/release/orchestrator manifest export
   ./target/release/orchestrator manifest export -o json
   ```

4. List available workflows:
   ```bash
   ./target/release/orchestrator get workflows
   ```

5. List available agents:
   ```bash
   ./target/release/orchestrator get agents
   ```

### Expected

- Workspace list shows all configured workspaces
- Workspace details show root_path, qa_targets, ticket_dir
- Manifest export shows full configuration bundle in YAML/JSON format
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
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: test-workspace
   spec:
     root_path: /tmp/test-ws
     qa_targets:
       - docs/qa
     ticket_dir: fixtures/ticket
   EOF
   ```

2. Apply with dry-run (should not persist):
   ```bash
   ./target/release/orchestrator apply -f fixtures/manifests/bundles/test-workspace.yaml --dry-run
   ```

3. Verify workspace was NOT created:
   ```bash
   ./target/release/orchestrator get workspaces
   ```

4. Apply without dry-run:
   ```bash
   ./target/release/orchestrator apply -f fixtures/manifests/bundles/test-workspace.yaml
   ```

5. Verify workspace WAS created:
   ```bash
   ./target/release/orchestrator describe workspace test-workspace
   ```

### Expected

- Dry-run shows what would be created but doesn't persist
- After dry-run, workspace list doesn't contain test-workspace
- After real apply, workspace info shows the new workspace

---

## Scenario 5: CLI Manifest Validate

### Preconditions

- None (tests configuration validation)

### Goal

Validate configuration validation catches invalid configurations.

> **Note**: `manifest validate` accepts multi-document YAML with
> `apiVersion`/`kind`/`metadata`/`spec` (the same format used by `apply`).
> The flat config format (runner/defaults/workspaces/…) is the internal
> serialization format and is **not** accepted by `manifest validate`.

### Steps

1. Create invalid manifest (empty root_path):
   ```bash
   cat > /tmp/invalid-config.yaml << 'EOF'
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: invalid-ws
   spec:
     root_path: ""
     qa_targets: []
     ticket_dir: fixtures/ticket
   EOF
   ```

2. Validate the invalid manifest:
   ```bash
   ./target/release/orchestrator manifest validate -f /tmp/invalid-config.yaml
   ```

3. Validate a known-good manifest:
   ```bash
   ./target/release/orchestrator manifest validate -f fixtures/manifests/bundles/echo-workflow.yaml
   ```

### Expected

- Invalid manifest validation fails with `workspace.spec.root_path cannot be empty`
- Valid manifest validation succeeds with `Manifest is valid`

---

## General Scenario: Task Delete with Force

### Preconditions

- At least one task exists

### Goal

Validate task deletion requires --force flag.

### Steps

1. Create a task:
   ```bash
   ./target/release/orchestrator task create --project "${QA_PROJECT}" --name "delete-me" --goal "test" --no-start
   ```

2. Try to delete without force (should prompt):
   ```bash
   ./target/release/orchestrator task delete {task_id}
   ```

3. Delete with force:
   ```bash
   ./target/release/orchestrator task delete {task_id} --force
   ```

4. Verify deletion:
   ```bash
   ./target/release/orchestrator task list
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
| 5 | CLI Manifest Validate | ☐ | | | |
| G1 | Task Delete with Force | ☐ | | | |
