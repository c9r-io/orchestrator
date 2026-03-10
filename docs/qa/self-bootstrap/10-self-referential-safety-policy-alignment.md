# Self-Bootstrap - Self-Referential Safety Policy Alignment

**Module**: self-bootstrap
**Scope**: Verify the unified self-referential safety contract across `orchestrator check`, runtime startup rejection, and policy audit events
**Scenarios**: 5
**Priority**: High

---

## Background

FR-003 aligned self-referential safety behavior so every entry point uses the same policy:

- Required: `checkpoint_strategy != none`
- Required: `auto_rollback == true`
- Required: at least one enabled builtin `self_test`
- Recommended-only: `binary_snapshot == true`
- Probe add-on: `self_referential_probe` requires a self-referential workspace and strict probe-only workflow shape

This document validates the shared evaluator through `orchestrator check`, task startup, and persisted policy audit events.

## Environment Note

Run this document against a freshly started daemon with an isolated app root and socket. Do not reuse a long-lived shared `data/` directory from unrelated QA runs.

---

## Scenario 1: `orchestrator check` Reports `binary_snapshot` As Warning-Only

### Preconditions
- Recreate an isolated QA project and apply the valid deterministic fixture bundle:
  ```bash
  QA_PROJECT="qa-self-ref-policy-${USER}-$(date +%Y%m%d%H%M%S)"
  orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
  rm -rf "workspace/${QA_PROJECT}"
  orchestrator apply -f fixtures/manifests/bundles/self-referential-safety-alignment.yaml --project "${QA_PROJECT}"
  ```

### Goal
Verify that `binary_snapshot: false` is reported as a warning with structured diagnostics and does not count as an error.

### Steps
1. Run the filtered preflight check in JSON format:
   ```bash
   orchestrator check --project "${QA_PROJECT}" --workflow binary-warning -o json > /tmp/self-ref-binary-warning.json
   cat /tmp/self-ref-binary-warning.json
   ```
2. Inspect the warning entry:
   ```bash
   jq '.checks[] | select(.rule=="self_ref.binary_snapshot_recommended")' /tmp/self-ref-binary-warning.json
   ```

### Expected
- The command exits with code `0`
- The JSON report contains `rule == "self_ref.binary_snapshot_recommended"`
- The matching entry has `severity == "warning"` and `blocking == false`
- The entry includes non-empty `actual`, `expected`, and `suggested_fix`
- The summary shows `errors == 0`

---

## Scenario 2: Self-Referential Task Fails When `checkpoint_strategy` Is `none`

### Preconditions
- Scenario 1 preconditions completed

### Goal
Verify config application rejects a self-referential workflow that omits checkpoints.

### Steps
1. Write an invalid workflow manifest:
   ```bash
   cat > /tmp/unsafe-checkpoint.yaml <<'YAML'
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: unsafe-checkpoint
   spec:
     steps:
       - id: implement
         type: implement
         enabled: true
         command: "echo checkpoint"
       - id: self_test
         type: self_test
         enabled: true
     loop:
       mode: once
     safety:
       auto_rollback: true
       checkpoint_strategy: none
       binary_snapshot: true
   YAML
   ```
2. Apply the manifest and capture the rejection:
   ```bash
   orchestrator apply -f /tmp/unsafe-checkpoint.yaml --project "${QA_PROJECT}" 2>/tmp/unsafe-checkpoint.err || true
   cat /tmp/unsafe-checkpoint.err
   ```

### Expected
- `apply` exits non-zero
- stderr includes `[SELF_REF_POLICY_VIOLATION]`
- stderr includes `self_ref.checkpoint_strategy_required`
- The invalid workflow is not added to project config

### Expected Data State
```sql
SELECT COUNT(*)
FROM resources
WHERE kind = 'Workflow'
  AND name = 'unsafe-checkpoint';
-- Expected: 0
```

---

## Scenario 3: Self-Referential Task Fails When `auto_rollback` Is Disabled

### Preconditions
- Scenario 1 preconditions completed

### Goal
Verify `auto_rollback: false` is rejected during config application, not downgraded to a warning.

### Steps
1. Write an invalid workflow manifest:
   ```bash
   cat > /tmp/unsafe-auto-rollback.yaml <<'YAML'
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: unsafe-auto-rollback
   spec:
     steps:
       - id: implement
         type: implement
         enabled: true
         command: "echo auto-rollback"
       - id: self_test
         type: self_test
         enabled: true
     loop:
       mode: once
     safety:
       auto_rollback: false
       checkpoint_strategy: git_tag
       binary_snapshot: true
   YAML
   ```
2. Apply the manifest and inspect stderr:
   ```bash
   orchestrator apply -f /tmp/unsafe-auto-rollback.yaml --project "${QA_PROJECT}" 2>/tmp/unsafe-auto-rollback.err || true
   cat /tmp/unsafe-auto-rollback.err
   ```

### Expected
- `apply` exits non-zero
- stderr contains `self_ref.auto_rollback_required`
- There is no warning-only continuation path for this workflow

### Expected Data State
```sql
SELECT COUNT(*)
FROM resources
WHERE kind = 'Workflow'
  AND name = 'unsafe-auto-rollback';
-- Expected: 0
```

---

## Scenario 4: Self-Referential Task Fails When Builtin `self_test` Is Missing

### Preconditions
- Scenario 1 preconditions completed

### Goal
Verify missing builtin `self_test` is enforced as a blocking rule during config application.

### Steps
1. Write an invalid workflow manifest:
   ```bash
   cat > /tmp/unsafe-no-self-test.yaml <<'YAML'
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: unsafe-no-self-test
   spec:
     steps:
       - id: implement
         type: implement
         enabled: true
         command: "echo no-self-test"
     loop:
       mode: once
     safety:
       auto_rollback: true
       checkpoint_strategy: git_tag
       binary_snapshot: true
   YAML
   ```
2. Apply the manifest and inspect stderr:
   ```bash
   orchestrator apply -f /tmp/unsafe-no-self-test.yaml --project "${QA_PROJECT}" 2>/tmp/unsafe-no-self-test.err || true
   cat /tmp/unsafe-no-self-test.err
   ```

### Expected
- `apply` exits non-zero
- stderr contains `self_ref.self_test_required`
- The invalid workflow is rejected before task creation

### Expected Data State
```sql
SELECT COUNT(*)
FROM resources
WHERE kind = 'Workflow'
  AND name = 'unsafe-no-self-test';
-- Expected: 0
```

---

## Scenario 5: Probe Workflow Rejects Non-Self-Referential Workspace Binding

### Preconditions
- Scenario 1 preconditions completed

### Goal
Verify `self_referential_probe` must be bound to a self-referential workspace even when the workflow itself is otherwise valid.

### Steps
1. Create a task that binds the valid probe workflow to the plain workspace:
   ```bash
   GOAL="qa-probe-plain-${RANDOM}"
   orchestrator task create --project "${QA_PROJECT}" --workspace plain --workflow probe-valid --goal "${GOAL}"
   sleep 2
   ```
2. Inspect the policy event and failed task:
   ```bash
   TASK_ID="$(sqlite3 data/agent_orchestrator.db "SELECT id FROM tasks WHERE project_id='${QA_PROJECT}' AND goal='${GOAL}' ORDER BY created_at DESC LIMIT 1;")"
   sqlite3 data/agent_orchestrator.db "SELECT status FROM tasks WHERE id='${TASK_ID}';"
   sqlite3 data/agent_orchestrator.db "SELECT payload_json FROM events WHERE task_id='${TASK_ID}' AND event_type='self_referential_policy_checked' ORDER BY id DESC LIMIT 1;"
   ```

### Expected
- The task ends in `failed`
- The policy event payload contains `self_ref.probe_requires_self_referential_workspace`
- The diagnostic payload still records the workflow as `profile":"self_referential_probe"`

### Expected Data State
```sql
SELECT status FROM tasks WHERE id = '{TASK_ID}';
-- Expected: one row, status = 'failed'

SELECT payload_json
FROM events
WHERE task_id = '{TASK_ID}' AND event_type = 'self_referential_policy_checked'
ORDER BY id DESC LIMIT 1;
-- Expected: payload_json contains self_ref.probe_requires_self_referential_workspace and profile self_referential_probe
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | `orchestrator check` Reports `binary_snapshot` As Warning-Only | ☐ | | | |
| 2 | Self-Referential Task Fails When `checkpoint_strategy` Is `none` | ☐ | | | |
| 3 | Self-Referential Task Fails When `auto_rollback` Is Disabled | ☐ | | | |
| 4 | Self-Referential Task Fails When Builtin `self_test` Is Missing | ☐ | | | |
| 5 | Probe Workflow Rejects Non-Self-Referential Workspace Binding | ☐ | | | |
