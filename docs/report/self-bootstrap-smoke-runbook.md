# Self-Bootstrap Smoke Runbook

Date baseline: 2026-02-26  
Repository: `/Volumes/Yotta/ai_native_sdlc`  
Entry CLI: `orchestrator`

This document records a reproducible smoke process for orchestrator self-bootstrap and can be reused in future sessions.

---

## 1. Goal

Validate that self-bootstrap works end-to-end with low model cost, with hard evidence from:
- task status
- events table
- command_runs table
- step logs
- generated artifact file

Primary smoke chain:
- `plan -> qa_doc_gen -> implement`

---

## 2. Preconditions

Run from repo root:

```bash
cd /Volumes/Yotta/ai_native_sdlc
```

Ensure runtime is clean:

```bash
orchestrator delete project/self-bootstrap --force
orchestrator init -f
```

Apply self-bootstrap resources:

```bash
orchestrator manifest validate -f docs/workflow/self-bootstrap.yaml
# ⚠️  必须使用 --project，否则真实 AI agent 会注册到全局空间
orchestrator apply -f docs/workflow/self-bootstrap.yaml --project self-bootstrap
orchestrator get workflow
orchestrator get agent
orchestrator get workspace
```

Expected key resources:
- workspace: `self`
- workflow: `self-bootstrap`
- agents: `architect`, `coder`, `tester`, `reviewer`

---

## 3. Low-Cost Smoke Workflow (recommended)

Use this temporary workflow to limit cost/time:

```bash
cat > /tmp/self-bootstrap-smoke.yaml <<'YAML'
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: self-bootstrap-smoke
spec:
  steps:
    - id: plan
      type: plan
      required_capability: plan
      enabled: true
      repeatable: false
      tty: false
    - id: qa_doc_gen
      type: qa_doc_gen
      required_capability: qa_doc_gen
      enabled: true
      repeatable: false
      tty: false
    - id: implement
      type: implement
      required_capability: implement
      enabled: true
      repeatable: false
      tty: false
    - id: loop_guard
      type: loop_guard
      enabled: true
      repeatable: true
      is_guard: true
      builtin: loop_guard
  loop:
    mode: once
    enabled: true
    stop_when_no_unresolved: true
YAML

orchestrator manifest validate -f /tmp/self-bootstrap-smoke.yaml
orchestrator apply -f /tmp/self-bootstrap-smoke.yaml --project self-bootstrap
```

---

## 4. Create and Run Smoke Task

Create task (do not auto-start):

```bash
orchestrator task create --project self-bootstrap \
  -n self-bootstrap-smoke-final \
  -w self \
  -W self-bootstrap-smoke \
  --no-start \
  -g "SMOKE RUN: create docs/qa/self-bootstrap/smoke-self-bootstrap.md containing marker SB_SMOKE_20260226 and minimal checklist; keep changes minimal; do not modify core/src/**" \
  -t docs/qa/orchestrator/26-self-bootstrap-workflow.md
```

Start task:

```bash
orchestrator task start <task_id>
```

Observe:

```bash
orchestrator task info <task_id> -o json
orchestrator task logs <task_id> --tail 50
```

---

## 5. Evidence Queries (DB)

Step events:

```bash
sqlite3 data/agent_orchestrator.db "
SELECT id,event_type,
       json_extract(payload_json,'$.step') AS step,
       json_extract(payload_json,'$.step_id') AS step_id,
       json_extract(payload_json,'$.success') AS success,
       json_extract(payload_json,'$.exit_code') AS exit_code,
       json_extract(payload_json,'$.validation_status') AS validation_status,
       created_at
FROM events
WHERE task_id='<task_id>'
ORDER BY id;"
```

Run rows:

```bash
sqlite3 data/agent_orchestrator.db "
SELECT id,phase,agent_id,exit_code,validation_status,started_at,ended_at,stdout_path,stderr_path
FROM command_runs
WHERE task_item_id=(SELECT id FROM task_items WHERE task_id='<task_id>' ORDER BY order_no LIMIT 1)
ORDER BY started_at;"
```

Check `plan_output` was resolved:

```bash
sqlite3 data/agent_orchestrator.db "
SELECT phase, command
FROM command_runs
WHERE task_item_id=(SELECT id FROM task_items WHERE task_id='<task_id>' ORDER BY order_no LIMIT 1)
  AND phase IN ('qa_doc_gen','implement')
ORDER BY started_at;"
```

Expected:
- command contains concrete plan text
- command does not contain literal `{plan_output}`

---

## 6. Artifact Validation

```bash
ls -la docs/qa/self-bootstrap/smoke-self-bootstrap.md
rg -n 'SB_SMOKE_20260226' docs/qa/self-bootstrap/smoke-self-bootstrap.md
sed -n '1,120p' docs/qa/self-bootstrap/smoke-self-bootstrap.md
```

Expected:
- file exists
- marker exists

---

## 7. What Was Found in This Session

Issue observed before fix:
- task reached `qa_doc_gen` with unresolved literal `{plan_output}` in command arguments
- result: downstream step could hang or run with poor context

Fix applied:
- propagate `plan` step stdout into pipeline vars via unified execution loop
- file: `core/src/scheduler/item_executor.rs` (unified `process_item_filtered()` loop with `StepExecutionAccumulator`)
- includes:
  - `pipeline_vars.prev_stdout = output.stdout.clone()`
  - `pipeline_vars.prev_stderr = output.stderr.clone()`
  - `pipeline_vars.vars.insert("plan_output", ...)` with large-output spill-to-file support

> **Note**: The original fix was in `core/src/scheduler.rs` `process_item()`. After the Unified Step Execution Model refactoring (design doc 13), all step execution logic moved to `item_executor.rs` with `StepExecutionAccumulator`. The `WorkflowStepType` enum was deleted; steps are now identified by string `id`.

Regression test added:
- `plan_output_is_propagated_to_qa_doc_gen_template`
- file: `core/src/scheduler.rs` test module
- assertion: `qa_doc_gen` command includes propagated plan content and no `{plan_output}` literal

Self-bootstrap model/runtime config:
- file: `docs/workflow/self-bootstrap.yaml`
- switched from `claude -p` to `opencode run`
- model unified to `minimax-coding-plan/MiniMax-M2.7-highspeed`

---

## 8. Acceptance Checklist

- [ ] task status is `completed`
- [ ] events show `plan -> qa_doc_gen -> implement` started and finished
- [ ] each corresponding `command_runs` row has `exit_code=0` and `validation_status=passed`
- [ ] `qa_doc_gen` command contains resolved plan content (no literal `{plan_output}`)
- [ ] artifact file exists with marker
- [ ] step logs exist under `data/logs/<task_id>/`

---

## 9. Cleanup

Delete smoke task:

```bash
orchestrator task delete <task_id> -f
```

Optional hard reset:

```bash
orchestrator delete project/self-bootstrap --force
```

