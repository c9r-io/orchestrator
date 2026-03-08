# Orchestrator - CLI Config and Database

**Module**: orchestrator
**Scope**: Validate configuration update and database reset flows
**Scenarios**: 4
**Priority**: High

---

## Background

This document validates config lifecycle commands and database reset behavior.

Entry points:
- `./scripts/run-cli.sh apply|manifest <command>`
- `./scripts/run-cli.sh qa project reset`

> **Note**: `apply` and `manifest validate` accept multi-document YAML with
> `apiVersion`/`kind`/`metadata`/`spec` resources. The flat config format
> (runner/defaults/workspaces/…) is the internal serialization and is **not**
> accepted by these commands. If any resource in a manifest has a validation
> error, the entire apply is aborted and no changes are persisted.

---

## Scenario 1: Manifest Apply - Update Configuration

### Preconditions

- Runtime initialized and config applied (see QA doc `01-cli-agent-orchestration.md` Scenario 1 preconditions).

### Steps

1. Apply an existing valid manifest bundle:
   ```bash
   ./scripts/run-cli.sh apply -f fixtures/manifests/bundles/echo-workflow.yaml
   ```

2. Verify:
   ```bash
   ./scripts/run-cli.sh manifest export -o yaml
   ./scripts/run-cli.sh get workflows
   ./scripts/run-cli.sh get agents
   ```

### Expected

- Config update succeeds (prints `configuration version: N`).
- Workflow and agent lists include newly configured entries.

---

## Scenario 2: Manifest Apply - Invalid Configuration

### Preconditions

- Runtime initialized and config applied (see QA doc `01-cli-agent-orchestration.md` Scenario 1 preconditions).

### Steps

1. Create invalid manifest (empty workspace name):
   ```bash
   cat > /tmp/invalid-config.yaml << 'EOF2'
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: ""
   spec:
     root_path: .
     qa_targets: [docs/qa]
     ticket_dir: fixtures/ticket
   EOF2
   ```

2. Apply invalid manifest:
   ```bash
   ./scripts/run-cli.sh apply -f /tmp/invalid-config.yaml
   ```

3. Verify existing config is unchanged:
   ```bash
   ./scripts/run-cli.sh workspace list
   ```

### Expected

- Command fails with validation error (e.g. `metadata.name cannot be empty`).
- Existing runtime config remains unchanged.

---

## Scenario 3: Manifest Apply - Add New Workspace

### Preconditions

- Runtime initialized and config applied (see QA doc `01-cli-agent-orchestration.md` Scenario 1 preconditions).
- Config must be applied: `./scripts/run-cli.sh apply -f fixtures/manifests/bundles/echo-workflow.yaml`
- Current config contains `default` workspace.

### Steps

1. Export current config:
   ```bash
   ./scripts/run-cli.sh manifest export -f /tmp/base-config.yaml
   ```

2. Create manifest that adds a new workspace:
   ```bash
   mkdir -p /tmp/new-workspace
   cat > /tmp/add-workspace.yaml << 'EOF2'
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: default
   spec:
     root_path: .
     qa_targets: [docs/qa]
     ticket_dir: fixtures/ticket
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: new-workspace
   spec:
     root_path: /tmp/new-workspace
     qa_targets: [docs/qa]
     ticket_dir: fixtures/ticket
   ---
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: mock
   spec:
     capabilities: [qa]
     command: "echo '{\"confidence\":0.9,\"quality_score\":0.86,\"artifacts\":[{\"kind\":\"analysis\",\"findings\":[{\"title\":\"qa-sample\",\"description\":\"qa sample\",\"severity\":\"info\"}]}]}'"
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: qa_only
   spec:
     steps:
       - id: qa
         type: qa
         enabled: true
     loop:
       mode: once
   EOF2
   ./scripts/run-cli.sh apply -f /tmp/add-workspace.yaml
   ```

3. Verify workspace list:
   ```bash
   ./scripts/run-cli.sh workspace list
   ./scripts/run-cli.sh workspace info new-workspace
   ```

### Expected

- New workspace is persisted (`workspace/new-workspace created`).
- Existing workspace remains available.

---

## Scenario 4: Project Reset Clears Task State

### Preconditions

- At least one task exists in the target project.

### Steps

1. Prepare a project with at least one task:
   ```bash
   QA_PROJECT="qa-db-reset-test"
   ./scripts/run-cli.sh qa project reset "${QA_PROJECT}" --keep-config --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   ./scripts/run-cli.sh qa project create "${QA_PROJECT}" --from-workspace default --workflow qa_only --force
   ./scripts/run-cli.sh task create --project "${QA_PROJECT}" --name "reset-test" --goal "reset test" --no-start
   ```

   > **Note**: `task create --project` requires the project workspace to contain at
   > least one QA markdown file for item-scoped workflows. `qa project create` now
   > copies `.md` files from the source workspace's QA target directories. If the
   > project still has no files, either specify `--workflow` with a task-scoped workflow
   > or provide explicit `--workspace` and `--workflow` flags.

2. Verify task exists in project:
   ```bash
   ./scripts/run-cli.sh task list -o json | jq '[.[] | select(.project_id == "'"${QA_PROJECT}"'")]'
   ```

   > **Note**: `task list` does not support a `--project` filter flag. Use
   > `task list -o json | jq '[.[] | select(.project_id == "...")]'` or
   > `sqlite3 data/agent_orchestrator.db "SELECT id, name FROM tasks WHERE project_id = '...'"`.

3. Reset the project:
   ```bash
   ./scripts/run-cli.sh qa project reset "${QA_PROJECT}" --force
   ```

4. Verify task records within the project are cleared:
   ```bash
   ./scripts/run-cli.sh task list -o json | jq '[.[] | select(.project_id == "'"${QA_PROJECT}"'")] | length'
   # Expected: 0
   ```

### Expected

- Project reset succeeds with `--force`.
- Task records within the target project are cleared (count = 0).
- Other project data is unaffected.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Manifest Apply - Update Configuration | ☐ | | | |
| 2 | Manifest Apply - Invalid Configuration | ☐ | | | |
| 3 | Manifest Apply - Add New Workspace | ☐ | | | |
| 4 | Project Reset Clears Task State | ☐ | | | |
