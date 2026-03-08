# Orchestrator - Runner Security Boundary and Observability

**Module**: orchestrator
**Scope**: Validate runner execution boundary controls, log/output redaction, and task execution metrics observability
**Scenarios**: 5
**Priority**: High

---

## Background

This document validates the runner boundary and observability coverage from the phase2/phase3 refactor:

- Runner policy model (`unsafe` / `allowlist`; `legacy` accepted as alias for `unsafe`) and runtime enforcement
- Pluginized runner entry (`spawn_with_runner`) behavior
- Sensitive text redaction for task logs and structured output
- Persistent task execution metrics (`task_execution_metrics`)
- `qa doctor` observability exposure for execution metrics

Default-policy initialization and backward-compatibility checks are covered in `docs/qa/orchestrator/31-runner-policy-defaults-compatibility.md`.

Entry point: `./scripts/run-cli.sh`

---

## Scenario 1: Allowlist Policy Schema Validation

### Preconditions

- CLI built from latest source.

### Goal

Ensure `policy=allowlist` is rejected when `allowed_shells` or `allowed_shell_args` is empty.

### Steps

1. Create invalid config:
   ```bash
   cat > /tmp/runner-allowlist-invalid.yaml << 'YAML'
   apiVersion: orchestrator.dev/v2
   kind: RuntimePolicy
   metadata:
     name: runtime
   spec:
     runner:
       policy: allowlist
       executor: shell
       shell: /bin/bash
       shell_arg: -lc
       allowed_shells: []
       allowed_shell_args: []
     resume:
       auto: false
   YAML
   ```
2. Validate config:
   ```bash
   ./scripts/run-cli.sh manifest validate -f /tmp/runner-allowlist-invalid.yaml
   ```

### Expected

- Command exits non-zero.
- Output includes validation errors for:
  - `runner.allowed_shells cannot be empty when policy=allowlist`
  - `runner.allowed_shell_args cannot be empty when policy=allowlist`

---

## Scenario 2: Runtime Policy Blocks Disallowed Shell

### Preconditions

- Runtime initialized.
- A project-scoped config is applied where:
  - `runner.policy=allowlist`
  - `runner.shell=/bin/sh`
  - `runner.allowed_shells=[/bin/bash]`
  - `runner.allowed_shell_args=[-lc]`

### Goal

Ensure run-phase command execution is denied by runner policy before process spawn.

### Steps

1. Prepare isolated project and apply policy config:
   ```bash
   QA_PROJECT="qa-runner-deny"
   ./scripts/run-cli.sh qa project reset "${QA_PROJECT}" --keep-config --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   ./scripts/run-cli.sh qa project create "${QA_PROJECT}" --force
   cat > /tmp/runner-policy-deny.yaml << 'YAML'
   runner:
     policy: allowlist
     executor: shell
     shell: /bin/sh
     shell_arg: -lc
     allowed_shells: [/bin/bash]
     allowed_shell_args: [-lc]
     redaction_patterns: [SECRET_TOKEN_ABC]
   resume:
     auto: false
   defaults:
     workspace: default
     workflow: qa_only
   workspaces:
     default:
       root_path: .
       qa_targets: [docs/qa]
       ticket_dir: fixtures/ticket
   agents:
     mock:
       metadata:
         name: mock
       capabilities: [qa]
       templates:
         qa: "echo '{\"confidence\":0.9,\"quality_score\":0.86,\"artifacts\":[]}'"
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
           stop_when_no_unresolved: true
       finalize:
         rules: []
   YAML
   ./scripts/run-cli.sh apply --project "${QA_PROJECT}" -f /tmp/runner-policy-deny.yaml
   ```
2. Create and start task:
   ```bash
   TASK_ID=$(./scripts/run-cli.sh task create --project "${QA_PROJECT}" --name "runner-policy-deny" --goal "policy deny" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/run-cli.sh task start "${TASK_ID}" || true
   ```
3. Inspect result:
   ```bash
   ./scripts/run-cli.sh task info "${TASK_ID}" -o json
   ```

### Expected

- Task does not complete successfully.
- Failure details include runner policy deny message (`runner.shell ... is not in runner.allowed_shells`).

---

## Scenario 3: Structured Output and Log Redaction

### Preconditions

- Runtime initialized.
- Config has `runner.redaction_patterns` containing `SECRET_TOKEN_ABC`.
- QA template emits `SECRET_TOKEN_ABC` in command output.

### Goal

Ensure sensitive token is redacted in persisted structured output and in `task logs` output.

### Steps

1. Apply redaction-enabled config and run task:
   ```bash
   # Apply runner config with redaction patterns
   cat > /tmp/runner-redaction-config.yaml << 'YAML'
   runner:
     policy: unsafe
     executor: shell
     shell: /bin/bash
     shell_arg: -lc
     redaction_patterns: [SECRET_TOKEN_ABC]
   resume:
     auto: false
   YAML
   ./scripts/run-cli.sh apply -f /tmp/runner-redaction-config.yaml

   # Apply CRD resources
   cat > /tmp/runner-redaction-resources.yaml << 'YAML'
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: default
   spec:
     root_path: "."
     qa_targets: [docs/qa]
     ticket_dir: fixtures/ticket
   ---
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: mock
   spec:
     capabilities: [qa]
     command: "echo '{\"confidence\":0.9,\"quality_score\":0.86,\"artifacts\":[],\"message\":\"SECRET_TOKEN_ABC\"}'"
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: qa_only
   spec:
     steps:
       - id: qa
         required_capability: qa
         enabled: true
     loop:
       mode: once
     finalize:
       rules: []
   YAML
   ./scripts/run-cli.sh apply -f /tmp/runner-redaction-resources.yaml

   TASK_ID=$(./scripts/run-cli.sh task create --project default --name "runner-redaction" --goal "redaction" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/run-cli.sh task start "${TASK_ID}" || true
   ```
2. Verify redaction in logs and DB:
   ```bash
   ./scripts/run-cli.sh task logs "${TASK_ID}" | rg "SECRET_TOKEN_ABC|REDACTED" -n
   sqlite3 data/agent_orchestrator.db "SELECT output_json FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='${TASK_ID}') ORDER BY started_at DESC LIMIT 1;"
   ```

### Expected

- Raw token `SECRET_TOKEN_ABC` is not visible in `task logs`.
- Persisted `output_json` contains `[REDACTED]` where token appeared in stdout/stderr payload.

### Expected Data State

```sql
SELECT output_json
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}')
ORDER BY started_at DESC
LIMIT 1;
-- Expected: output_json does not contain SECRET_TOKEN_ABC and contains [REDACTED]
```

---

## Scenario 4: task_execution_metrics Persistence

### Preconditions

- At least one task run reaches terminal status.

### Goal

Ensure scheduler terminal path persists execution metrics.

### Steps

1. Create and run task:
   ```bash
   TASK_ID=$(./scripts/run-cli.sh task create --project default --name "metrics-persist" --goal "metrics persist" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/run-cli.sh task start "${TASK_ID}" || true
   ```
2. Query metrics table:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT task_id, status, current_cycle, unresolved_items, total_items, failed_items, command_runs FROM task_execution_metrics WHERE task_id='${TASK_ID}' ORDER BY created_at DESC LIMIT 1;"
   ```

### Expected

- At least one row exists in `task_execution_metrics` for the task.
- `status` is terminal (`completed` or `failed`).
- `command_runs` is non-negative and reflects executed runs.

### Expected Data State

```sql
SELECT COUNT(*)
FROM task_execution_metrics
WHERE task_id = '{task_id}';
-- Expected: >= 1
```

---

## Scenario 5: QA Doctor Exposes Observability Metrics

### Preconditions

- `task_execution_metrics` contains records from prior runs.

### Goal

Ensure `qa doctor` exposes new metrics fields in JSON and table outputs.

### Steps

1. Run doctor in JSON mode:
   ```bash
   ./scripts/run-cli.sh qa doctor -o json
   ```
2. Run doctor in table mode:
   ```bash
   ./scripts/run-cli.sh qa doctor
   ```

### Expected

- JSON includes:
  - `observability.task_execution_metrics_total`
  - `observability.task_execution_metrics_last_24h`
  - `observability.task_completion_rate`
- Table output includes corresponding lines with non-error values.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Allowlist Policy Schema Validation | ✅ | 2026-03-02 | cursor | validate returns runner allowlist empty errors |
| 2 | Runtime Policy Blocks Disallowed Shell | ✅ | 2026-03-02 | cursor | `runner.shell '/bin/sh' is not in runner.allowed_shells`; task transitions to `failed` |
| 3 | Structured Output and Log Redaction | ✅ | 2026-03-02 | cursor | `policy: unsafe` + redaction: logs/output_json contain `[REDACTED]`, secret token absent |
| 4 | task_execution_metrics Persistence | ✅ | 2026-03-02 | cursor | row persisted with terminal status and command_runs count |
| 5 | QA Doctor Exposes Observability Metrics | ✅ | 2026-03-02 | cursor | json contains total/last_24h/completion_rate fields |
