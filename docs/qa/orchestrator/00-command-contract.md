# Orchestrator - QA Command Contract

**Module**: orchestrator  
**Scope**: Canonical CLI contract for all QA documents  
**Scenarios**: 4  
**Priority**: Critical

---

## Purpose

This document is the single source of truth for command syntax used by QA docs.
All files under `docs/qa/orchestrator/` must align with this contract.

Runtime state source of truth is SQLite. YAML is used as import/export/edit artifact when needed.

Entry point: `./scripts/orchestrator.sh <command>` (recommended) or `./core/target/release/agent-orchestrator <command>`.

---

## Scenario 1: Valid Top-Level Command Surface

### Preconditions

- CLI binary is available.

### Steps

1. Show help:
   ```bash
   ./scripts/orchestrator.sh --help
   ```

2. Confirm top-level commands exist:
   - `init`
   - `apply`
   - `get`
   - `describe`
   - `task`
   - `workspace`
   - `agent`
   - `workflow`
  - `manifest`
   - `edit`
   - `db`
   - `qa`
   - `completion`
   - `debug`
   - `exec`

### Expected Result

- Help output includes all commands above.
- `task worker` subcommands are visible under `task --help`.
- Deprecated/removed command groups are not documented in QA steps.

---

## Scenario 2: Parameter Contract Check

### Preconditions

- Database initialized and config applied from a YAML file that defines a `default` workspace.
  The `init` command only creates the DB schema; it does **not** load config or create workspaces.
  You must run `apply -f <manifest.yaml>` so that
  the `default` workspace is present in SQLite before running any workspace/task commands.

### Steps

1. Apply manifest environment (if not already done):
   ```bash
   ./scripts/orchestrator.sh init
   ./scripts/orchestrator.sh apply -f <manifest.yaml>
   ```

2. (Recommended for isolated QA reruns) Reset only the scenario project:
   ```bash
   ./scripts/orchestrator.sh qa project reset <qa-project-id> --keep-config --force
   ```

3. Validate workspace info positional argument:
   ```bash
   ./scripts/orchestrator.sh workspace info default
   ```

4. Validate output format flags:
   ```bash
   ./scripts/orchestrator.sh task list -o json
   ./scripts/orchestrator.sh task info {task_id} -o yaml
   ./scripts/orchestrator.sh get workspaces -o yaml
   ```

5. Validate task create does not depend on `--format`:
   ```bash
   ./scripts/orchestrator.sh task create --project <qa-project-id> --name "contract-check" --goal "check" --no-start
   ```

6. Validate new scheduling flags and worker commands:
   ```bash
   ./scripts/orchestrator.sh task create --help | rg -- "--detach"
   ./scripts/orchestrator.sh task start --help | rg -- "--detach"
   ./scripts/orchestrator.sh task worker --help
   ```
7. Validate task edit and exec command families:
   ```bash
   ./scripts/orchestrator.sh task edit --help
   ./scripts/orchestrator.sh exec --help
   ```

### Expected Result

- `workspace info` accepts positional workspace id.
- Output format flags work for commands that support `-o`.
- `task create --format ...` is never required in QA docs.
- `--detach` flags and `task worker` command family are part of the CLI contract.
- `task edit` and `exec` are part of the CLI contract.

---

## Scenario 3: kubectl-Style Surface Contract

### Preconditions

- Database initialized.

### Steps

1. Validate list-style get:
   ```bash
   ./scripts/orchestrator.sh get workspaces
   ./scripts/orchestrator.sh get agents
   ./scripts/orchestrator.sh get workflows
   ```

2. Validate label selector syntax:
   ```bash
   ./scripts/orchestrator.sh get workspaces -l env=dev
   ```

3. Validate stdin apply contract:
   ```bash
   cat fixtures/manifests/bundles/output-formats.yaml | ./scripts/orchestrator.sh apply -f -
   ```

4. Validate create command surfaces:
   ```bash
   ./scripts/orchestrator.sh workspace create --help
   ./scripts/orchestrator.sh agent create --help
   ./scripts/orchestrator.sh workflow create --help
   ```

### Expected Result

- `get <resource-type>` syntax works.
- `-l key=value[,k=v]` is accepted on list get commands.
- `apply -f -` reads from stdin.
- `workspace/agent/workflow create` subcommands are exposed.

---

## Scenario 4: Banned Patterns Guard

### Preconditions

- QA lint script is available.

### Steps

1. Run QA doc lint:
   ```bash
   ./scripts/qa-doc-lint.sh
   ```

2. Confirm no banned patterns in docs:
   - `cd orchestrator`
   - `--workspace-id`
   - `orchestrator agent health`
   - `orchestrator/config/default.yaml`
   - `config bootstrap --from`

### Expected Result

- Lint exits with code `0`.
- QA docs remain aligned with current repository structure and CLI surface.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Valid Top-Level Command Surface | ✅ | 2026-02-23 | opencode | |
| 2 | Parameter Contract Check | ✅ | 2026-02-23 | opencode | |
| 3 | kubectl-Style Surface Contract | ✅ | 2026-02-23 | opencode | |
| 4 | Banned Patterns Guard | ✅ | 2026-02-23 | opencode | |
