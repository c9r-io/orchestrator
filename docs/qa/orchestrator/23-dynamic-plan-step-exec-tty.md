# Orchestrator - Dynamic Plan Step Injection and Exec TTY

**Module**: orchestrator  
**Scope**: Validate `task edit` insertion of `plan` step and `orchestrator exec [-it]` target behavior (`task/<task_id>/step/<step_id>` and `session/<session_id>`)  
**Scenarios**: 5  
**Priority**: High

---

## Background

This document validates the new interactive planning workflow:

- `task edit` can insert a `plan` step before an existing step (for example `qa`)
- workflow step supports `tty` flag
- `exec` supports target selector `task/<task_id>/step/<step_id>` and `session/<session_id>`
- `exec -it` requires step `tty=true`

Entry point: `orchestrator`

---

## Scenario 1: CLI Surface Exposes `task edit` and `exec`

### Preconditions

- CLI binary is available.

### Steps

1. Verify root command includes `exec`:
   ```bash
   orchestrator --help | rg "exec"
   ```
2. Verify task subcommand includes `edit`:
   ```bash
   orchestrator task --help | rg "edit"
   ```
3. Verify usage and flags:
   ```bash
   orchestrator exec --help
   orchestrator task edit --help
   ```

### Expected

- Root help shows `exec`.
- `task --help` shows `edit`.
- `exec --help` includes `-i/-t` and target formats `task/<task_id>/step/<step_id>` and `session/<session_id>`.

### Expected Data State
```sql
SELECT COUNT(*) FROM tasks;
-- Expected: unchanged by this scenario
```

---

## Scenario 2: Insert `plan` Step Before `qa` with `tty=true`

### Preconditions

- Runtime is initialized and a valid orchestrator config is applied.
- Config includes at least one agent with both `plan` and `qa` capabilities (required for Scenario 3 execution).
- At least one task exists and has step `qa` in `execution_plan_json`.

### Steps

1. Apply manifest with plan-capable agent and create isolated task:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/output-formats.yaml
   orchestrator apply -f fixtures/manifests/bundles/self-bootstrap-test.yaml
   QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
   orchestrator project reset "${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   orchestrator apply -f fixtures/manifests/bundles/self-bootstrap-test.yaml --project "${QA_PROJECT}"
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --name "plan-insert" --goal "insert plan before qa" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ```

   > **Troubleshooting**: If `task start` in Scenario 3 fails with `No healthy agent found with capability: plan`, verify the applied config includes an agent with `plan` capability: `orchestrator get agents | grep plan`.
2. Insert plan step:
   ```bash
   orchestrator task edit "${TASK_ID}" --insert-before qa --step plan --tty
   ```
3. Verify execution plan structure:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT json_extract(execution_plan_json,'$.steps') FROM tasks WHERE id='${TASK_ID}';"
   ```

### Expected

- Command returns success message for inserted step.
- `execution_plan_json.steps` contains a `plan-*` step before `qa`.
- Inserted step has `id` starting with `plan-` and `tty=true`.

### Expected Data State
```sql
SELECT
  EXISTS(
    SELECT 1
    FROM tasks
    WHERE id = '{task_id}'
      AND execution_plan_json LIKE '%"id":"plan-%'
      AND execution_plan_json LIKE '%"tty":true%'
  );
-- Expected: 1
```

---

## Scenario 3: Resume Task Executes `plan` Step Before `qa`

### Preconditions

- Scenario 2 completed and task contains inserted `plan` step.
- Agent templates include `plan` and `qa` capabilities.

### Steps

1. Start task:
   ```bash
   orchestrator task start "{task_id}" || true
   ```
2. Query recent events:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT event_type, json_extract(payload_json,'$.step') AS step_name FROM events WHERE task_id='{task_id}' AND event_type IN ('step_started','step_finished') ORDER BY id DESC LIMIT 40;"
   ```
3. Query phase runs:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT phase, started_at FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='{task_id}') ORDER BY started_at ASC;"
   ```

### Expected

- `step_started/step_finished` contains `plan`.
- `command_runs.phase` includes `plan`.
- `plan` run appears before `qa` for the same task execution timeline.

### Expected Data State
```sql
SELECT COUNT(*)
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}')
  AND phase = 'plan';
-- Expected: >= 1
```

---

## Scenario 4: `exec -it` Rejects Non-TTY Step

### Preconditions

- A task exists with step `qa` where `tty=false` (default).

### Steps

1. Run interactive exec against non-tty step:
   ```bash
   orchestrator exec -it task/{task_id}/step/qa -- echo "should-fail"
   ```

### Expected

- Command exits non-zero.
- Error message states that step `tty` is disabled and suggests enabling it via `task edit ... --tty`.

### Expected Data State
```sql
SELECT COUNT(*)
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}')
  AND phase = 'qa'
  AND command LIKE '%should-fail%';
-- Expected: 0
```

---

## Scenario 5: `exec` Non-Interactive Command in Step Context

### Preconditions

- A task exists with an addressable step id (for example `plan-1` or `qa`).
- Workspace root and task item are resolvable.

### Steps

1. Run non-interactive exec:
   ```bash
   orchestrator exec task/{task_id}/step/{step_id} -- echo "exec-smoke"
   ```
2. Validate command output contains marker:
   ```bash
   orchestrator exec task/{task_id}/step/{step_id} -- echo "exec-smoke-2"
   ```

### Expected

- Command succeeds (exit code 0).
- stdout contains `exec-smoke` / `exec-smoke-2`.
- No task status corruption occurs after command execution.

### Expected Data State
```sql
SELECT status
FROM tasks
WHERE id = '{task_id}';
-- Expected: still a valid lifecycle state (pending/running/paused/completed/failed), no malformed transition
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | CLI Surface Exposes `task edit` and `exec` | ☐ | | | |
| 2 | Insert `plan` Step Before `qa` with `tty=true` | ☐ | | | |
| 3 | Resume Task Executes `plan` Step Before `qa` | ☐ | | | |
| 4 | `exec -it` Rejects Non-TTY Step | ☐ | | | |
| 5 | `exec` Non-Interactive Command in Step Context | ☐ | | | |
