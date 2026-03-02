# Orchestrator - Runner Security Boundary and Observability

**Module**: orchestrator
**Scope**: Validate runner execution boundary controls, log/output redaction, and task execution metrics observability
**Scenarios**: 8
**Priority**: High

---

## Background

This document validates phase2/phase3 refactor outcomes:

- Runner policy model (`unsafe` / `allowlist`; `legacy` accepted as alias for `unsafe`) and runtime enforcement
- Default policy is `allowlist` (secure-by-default); `unsafe` mode requires explicit declaration
- Pluginized runner entry (`spawn_with_runner`) behavior
- Sensitive text redaction for task logs and structured output
- Persistent task execution metrics (`task_execution_metrics`)
- `qa doctor` observability exposure for execution metrics

Entry point: `./scripts/orchestrator.sh`

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
   ./scripts/orchestrator.sh manifest validate -f /tmp/runner-allowlist-invalid.yaml
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
   ./scripts/orchestrator.sh db reset --force
   ./scripts/orchestrator.sh init --force
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
       ticket_dir: docs/ticket
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
   ./scripts/orchestrator.sh apply -f /tmp/runner-policy-deny.yaml
   ```
2. Create and start task:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create --project default --name "runner-policy-deny" --goal "policy deny" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/orchestrator.sh task start "${TASK_ID}" || true
   ```
3. Inspect result:
   ```bash
   ./scripts/orchestrator.sh task info "${TASK_ID}" -o json
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
   cat > /tmp/runner-redaction.yaml << 'YAML'
   runner:
     policy: unsafe
     executor: shell
     shell: /bin/bash
     shell_arg: -lc
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
       ticket_dir: docs/ticket
   agents:
     mock:
       metadata:
         name: mock
       capabilities: [qa]
       templates:
         qa: "echo '{\"confidence\":0.9,\"quality_score\":0.86,\"artifacts\":[],\"message\":\"SECRET_TOKEN_ABC\"}'"
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
   ./scripts/orchestrator.sh apply -f /tmp/runner-redaction.yaml
   TASK_ID=$(./scripts/orchestrator.sh task create --project default --name "runner-redaction" --goal "redaction" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/orchestrator.sh task start "${TASK_ID}" || true
   ```
2. Verify redaction in logs and DB:
   ```bash
   ./scripts/orchestrator.sh task logs "${TASK_ID}" | rg "SECRET_TOKEN_ABC|REDACTED" -n
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
   TASK_ID=$(./scripts/orchestrator.sh task create --project default --name "metrics-persist" --goal "metrics persist" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/orchestrator.sh task start "${TASK_ID}" || true
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
   ./scripts/orchestrator.sh qa doctor -o json
   ```
2. Run doctor in table mode:
   ```bash
   ./scripts/orchestrator.sh qa doctor
   ```

### Expected

- JSON includes:
  - `observability.task_execution_metrics_total`
  - `observability.task_execution_metrics_last_24h`
  - `observability.task_completion_rate`
- Table output includes corresponding lines with non-error values.

---

## Scenario 6: Default Initialization Produces Allowlist Policy

### Preconditions

- CLI built from latest source.
- No prior configuration in DB (clean state).

### Goal

Ensure that default initialization (without explicit RuntimePolicy in the manifest) produces an `allowlist` runner policy with populated shell/arg lists.

### Steps

1. Reset and initialize with a manifest that has no RuntimePolicy:
   ```bash
   ./scripts/orchestrator.sh db reset -f --include-config --include-history
   ./scripts/orchestrator.sh init -f
   ./scripts/orchestrator.sh apply -f docs/workflow/self-bootstrap.yaml
   ```
2. Export current configuration:
   ```bash
   ./scripts/orchestrator.sh manifest export -f /tmp/default-policy-export.yaml
   cat /tmp/default-policy-export.yaml | grep -A5 'policy:'
   ```

### Expected

- Exported YAML contains `policy: allowlist`.
- `allowed_shells` list is non-empty (contains at least `/bin/bash`).
- `allowed_shell_args` list is non-empty (contains at least `-lc`).

---

## Scenario 7: Explicit Unsafe Mode Remains Functional

### Preconditions

- CLI built from latest source.
- Runtime initialized.

### Goal

Ensure that explicitly setting `policy: unsafe` is accepted, applied, and the task can execute under unsafe mode.

### Steps

1. Apply config with explicit unsafe policy:
   ```bash
   ./scripts/orchestrator.sh db reset --force
   ./scripts/orchestrator.sh init --force
   cat > /tmp/runner-unsafe-explicit.yaml << 'YAML'
   runner:
     policy: unsafe
     executor: shell
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
   ./scripts/orchestrator.sh apply -f /tmp/runner-unsafe-explicit.yaml
   ```
2. Verify policy applied:
   ```bash
   ./scripts/orchestrator.sh manifest export -f /tmp/unsafe-export.yaml
   cat /tmp/unsafe-export.yaml | grep 'policy:'
   ```
3. Create and run task:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create --project default --name "unsafe-mode-test" --goal "unsafe mode" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/orchestrator.sh task start "${TASK_ID}" || true
   ./scripts/orchestrator.sh task info "${TASK_ID}" -o json
   ```

### Expected

- Exported policy is `unsafe`.
- Task executes successfully under unsafe mode (no shell/arg allowlist enforcement).

---

## Scenario 8: Legacy Alias Backward Compatibility

### Preconditions

- CLI built from latest source.
- Runtime initialized.

### Goal

Ensure that `policy: legacy` in YAML manifests is accepted as a backward-compatible alias for `unsafe` and correctly applied.

### Steps

1. Apply config using the legacy alias:
   ```bash
   ./scripts/orchestrator.sh db reset --force
   ./scripts/orchestrator.sh init --force
   cat > /tmp/runner-legacy-alias.yaml << 'YAML'
   runner:
     policy: legacy
     executor: shell
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
   ./scripts/orchestrator.sh apply -f /tmp/runner-legacy-alias.yaml
   ```
2. Verify the policy is normalized to `unsafe` on re-export:
   ```bash
   ./scripts/orchestrator.sh manifest export -f /tmp/legacy-alias-export.yaml
   cat /tmp/legacy-alias-export.yaml | grep 'policy:'
   ```

### Expected

- `apply` succeeds without errors.
- Re-exported manifest shows `policy: unsafe` (normalized from `legacy` input).

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Allowlist Policy Schema Validation | ✅ | 2026-03-02 | cursor | validate returns runner allowlist empty errors |
| 2 | Runtime Policy Blocks Disallowed Shell | ✅ | 2026-03-02 | cursor | `runner.shell '/bin/sh' is not in runner.allowed_shells`; task transitions to `failed` |
| 3 | Structured Output and Log Redaction | ✅ | 2026-03-02 | cursor | `policy: unsafe` + redaction: logs/output_json contain `[REDACTED]`, secret token absent |
| 4 | task_execution_metrics Persistence | ✅ | 2026-03-02 | cursor | row persisted with terminal status and command_runs count |
| 5 | QA Doctor Exposes Observability Metrics | ✅ | 2026-03-02 | cursor | json contains total/last_24h/completion_rate fields |
| 6 | Default Initialization Produces Allowlist Policy | ✅ | 2026-03-02 | cursor | init + self-bootstrap.yaml (no RuntimePolicy) → export shows `policy: allowlist` with populated lists |
| 7 | Explicit Unsafe Mode Remains Functional | ✅ | 2026-03-02 | cursor | `policy: unsafe` apply succeeds, export shows `unsafe`, task executes normally |
| 8 | Legacy Alias Backward Compatibility | ✅ | 2026-03-02 | cursor | `policy: legacy` apply succeeds, re-export normalized to `policy: unsafe` |
