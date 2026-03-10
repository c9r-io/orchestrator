# Orchestrator - Step Execution Profiles

**Module**: orchestrator
**Scope**: Validate project-scoped `ExecutionProfile` resources, step-level `execution_profile` binding, host/sandbox selection, and backward-compatible host defaults
**Scenarios**: 5
**Priority**: High

---

## Background

The orchestrator now supports step-level execution isolation:

- `ExecutionProfile` is a project-scoped resource
- agent steps can reference a profile via `execution_profile`
- missing `execution_profile` falls back to implicit `host`
- `implement` / `ticket_fix` can run in sandbox while `qa_testing` remains on host

This document validates the new step-level contract. Global runner policy behavior remains covered in:

- `docs/qa/orchestrator/21-runner-security-observability.md`
- `docs/qa/orchestrator/31-runner-policy-defaults-compatibility.md`
- `docs/qa/orchestrator/45-cli-unsafe-mode.md`

Sandbox denial semantics are validated separately in:

- `docs/qa/orchestrator/55-sandbox-write-boundaries.md`

Entry point: `orchestrator`

---

## Scenario 1: ExecutionProfile Resource Apply and Export Round-Trip

### Preconditions

- CLI built from latest source.
- Runtime initialized.

### Goal

Ensure `ExecutionProfile` manifests can be applied and exported as project-scoped resources.

### Steps

1. Apply the reusable sandbox execution fixture bundle:
   ```bash
   QA_PROJECT="qa-exec-profile-roundtrip"
   orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   orchestrator apply --project "${QA_PROJECT}" -f fixtures/manifests/bundles/sandbox-execution-profiles.yaml
   ```
2. Export manifests and inspect the profile:
   ```bash
   orchestrator manifest export > /tmp/execution-profile-export.yaml
   rg -n "kind: ExecutionProfile|name: sandbox_write|mode: sandbox|fs_mode: workspace_rw_scoped|name: sandbox_network_deny" /tmp/execution-profile-export.yaml
   ```

### Expected

- `apply` succeeds.
- Export contains the fixture-bundle `ExecutionProfile` resources under the target project.
- Export preserves the declared `mode`, `fs_mode`, and profile names such as `sandbox_write` and `sandbox_network_deny`.

---

## Scenario 2: Non-Agent Step Rejects execution_profile

### Preconditions

- CLI built from latest source.

### Goal

Ensure `execution_profile` can only be used on agent steps.

### Steps

1. Create an invalid workflow with a builtin step using `execution_profile`:
   ```bash
   cat > /tmp/execution-profile-invalid-builtin.yaml << 'YAML'
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: invalid-builtin-profile
   spec:
     steps:
       - id: self_test
         type: self_test
         builtin: self_test
         execution_profile: sandbox_write
         enabled: true
     loop:
       mode: once
   YAML
   orchestrator manifest validate -f /tmp/execution-profile-invalid-builtin.yaml
   ```

### Expected

- Command exits non-zero.
- Validation error states that `execution_profile` is only supported on agent steps.

---

## Scenario 3: Unknown execution_profile Is Rejected

### Preconditions

- CLI built from latest source.
- Runtime initialized.

### Goal

Ensure a workflow cannot reference a non-existent project-scoped profile.

### Steps

1. Prepare isolated project and apply minimal workspace/agent resources:
   ```bash
   QA_PROJECT="qa-exec-profile-missing"
   orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   cat > /tmp/execution-profile-missing-bundle.yaml << 'YAML'
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: default
   spec:
     root_path: "."
     qa_targets: [docs/qa]
     ticket_dir: docs/ticket
   ---
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: mock-impl
   spec:
     capabilities: [implement]
     command: "echo '{\"confidence\":0.9,\"quality_score\":0.9,\"artifacts\":[]}'"
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: invalid-missing-profile
   spec:
     steps:
       - id: implement
         type: implement
         required_capability: implement
         execution_profile: missing_profile
         enabled: true
     loop:
       mode: once
   YAML
   orchestrator apply --project "${QA_PROJECT}" -f /tmp/execution-profile-missing-bundle.yaml
   ```

### Expected

- `apply` exits non-zero.
- Error states that the workflow step references an unknown execution profile.

---

## Scenario 4: Mixed Workflow Applies Host and Sandbox Profiles Per Step

### Preconditions

- macOS environment with `/usr/bin/sandbox-exec` available.
- Runtime initialized.

### Goal

Ensure a single workflow can run `implement` in sandbox and `qa_testing` on host, and that events record the resolved execution profile.

### Steps

1. Apply the reusable sandbox execution fixture bundle:
   ```bash
   QA_PROJECT="qa-exec-profile-mixed"
   orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   orchestrator apply --project "${QA_PROJECT}" -f fixtures/manifests/bundles/sandbox-execution-profiles.yaml
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workflow exec-profile-mixed --name "exec-profile-mixed" --goal "mixed profiles" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}" || true
   ```
2. Query execution profile events:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT event_type, payload_json FROM events WHERE task_id='${TASK_ID}' AND event_type='execution_profile_applied' ORDER BY created_at;"
   ```

### Expected

- Task reaches completion or at minimum starts both steps without profile-resolution errors.
- Events include one row for `implement` with `execution_profile=sandbox_write`.
- Events include one row for `qa_testing` with `execution_profile=host`.

### Expected Data State

```sql
SELECT event_type, payload_json
FROM events
WHERE task_id = '{task_id}'
  AND event_type = 'execution_profile_applied'
ORDER BY created_at;
-- Expected: at least one payload containing "sandbox_write" and one containing "host"
```

---

## Scenario 5: Missing execution_profile Defaults to Host

### Preconditions

- CLI built from latest source.
- Runtime initialized.

### Goal

Ensure old workflows with no `execution_profile` continue to run and resolve to implicit `host`.

### Steps

1. Apply a legacy-style workflow with no `execution_profile`:
   ```bash
   QA_PROJECT="qa-exec-profile-compat"
   orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project "${QA_PROJECT}"
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workflow qa_only --name "exec-profile-compat" --goal "compat host default" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}" || true
   sqlite3 data/agent_orchestrator.db \
     "SELECT event_type, payload_json FROM events WHERE task_id='${TASK_ID}' AND event_type='execution_profile_applied' ORDER BY created_at DESC LIMIT 5;"
   ```

### Expected

- Task behavior matches pre-feature compatibility expectations.
- No validation error requires a profile.
- If `execution_profile_applied` events are emitted, payload resolves to `host`.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | ExecutionProfile Resource Apply and Export Round-Trip | ☐ | | | |
| 2 | Non-Agent Step Rejects execution_profile | ☐ | | | |
| 3 | Unknown execution_profile Is Rejected | ☐ | | | |
| 4 | Mixed Workflow Applies Host and Sandbox Profiles Per Step | ☐ | | | |
| 5 | Missing execution_profile Defaults to Host | ☐ | | | |
