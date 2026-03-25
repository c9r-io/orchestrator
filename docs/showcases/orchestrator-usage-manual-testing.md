# Orchestrator Usage (Manual Testing)

> **Agent Collaboration**: This document is an agent-executable plan. Open this project in an AI coding agent (Claude Code, OpenCode, Codex, etc.) — the agent reads this plan and orchestrates other agents via the orchestrator CLI to collaboratively complete the task, from resource deployment and execution to result verification, fully autonomously.

This document is a practical, copy-paste oriented guide for manually testing the orchestrator from CLI.

Entry point: `orchestrator` (CLI client) + `orchestratord` (daemon).

---

## 1. Prerequisites

Run from repository root:

```bash
cd "$ORCHESTRATOR_ROOT"   # your orchestrator project directory
cargo build --release -p orchestratord -p orchestrator-cli
```

Start the daemon:

```bash
orchestratord --foreground --workers 2
```

Verify CLI surface:

```bash
orchestrator --help
orchestrator task --help
```

---

## 2. Clean Runtime State

```bash
orchestrator delete project/<project> --force
orchestrator init
```

Runtime data locations:
- SQLite DB: `~/.orchestratord/agent_orchestrator.db`
- Logs: `data/logs/`

---

## 3. Apply Self-Bootstrap Workflow

```bash
orchestrator manifest validate -f docs/workflow/self-bootstrap.yaml
# ⚠️  必须使用 --project，否则真实 AI agent 会注册到全局空间
orchestrator apply -f docs/workflow/execution-profiles.yaml --project self-bootstrap
orchestrator apply -f docs/workflow/self-bootstrap.yaml --project self-bootstrap
orchestrator get workflow
orchestrator get agent
orchestrator get workspace
```

Expected:
- workspace `self`
- workflow `self-bootstrap`
- agents `architect`, `coder`, `tester`, `reviewer`

---

## 4. (Optional) Low-Cost Smoke Workflow

For fast/cheap verification, use a 3-step flow (`plan -> qa_doc_gen -> implement`):

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

## 5. Create and Run a Task

Important:
- item-scoped workflows default to scanning QA/Security markdown under workspace `qa_targets`
- task-scoped-only workflows can be created without QA markdown; the orchestrator uses a synthetic `__UNASSIGNED__` anchor item
- explicit `--target-file` values override the default source

Create task without auto start:

```bash
orchestrator task create --project self-bootstrap \
  -n self-bootstrap-manual \
  -w self \
  -W self-bootstrap-smoke \
  --no-start \
  -g "SMOKE RUN: create docs/qa/self-bootstrap/smoke-self-bootstrap.md with marker SB_SMOKE_20260226; keep changes minimal; do not modify core/src/**" \
  -t docs/qa/orchestrator/26-self-bootstrap-workflow.md
```

Start task:

```bash
orchestrator task start <task_id>
```

Watch summary:

```bash
orchestrator task list -o json
orchestrator task info <task_id> -o json
```

Watch logs:

```bash
orchestrator task logs <task_id> --tail 50
```

---

## 6. Validate Step Execution (Events + Runs)

Check step order:

```bash
sqlite3 ~/.orchestratord/agent_orchestrator.db "
SELECT id,
       event_type,
       json_extract(payload_json,'$.step') AS step,
       json_extract(payload_json,'$.step_id') AS step_id,
       json_extract(payload_json,'$.success') AS success,
       json_extract(payload_json,'$.exit_code') AS exit_code,
       created_at
FROM events
WHERE task_id='<task_id>'
ORDER BY id;"
```

Check run details and log file paths:

```bash
sqlite3 ~/.orchestratord/agent_orchestrator.db "
SELECT id, phase, agent_id, exit_code, validation_status, started_at, ended_at, stdout_path, stderr_path
FROM command_runs
WHERE task_item_id=(SELECT id FROM task_items WHERE task_id='<task_id>' ORDER BY order_no LIMIT 1)
ORDER BY started_at;"
```

---

## 7. Validate `plan_output` Propagation

`plan` output should be injected into downstream `qa_doc_gen`/`implement` commands.

Check command text:

```bash
sqlite3 ~/.orchestratord/agent_orchestrator.db "
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

## 8. Validate Generated Artifact

```bash
ls -la docs/qa/self-bootstrap/smoke-self-bootstrap.md
rg -n 'SB_SMOKE_20260226' docs/qa/self-bootstrap/smoke-self-bootstrap.md
sed -n '1,120p' docs/qa/self-bootstrap/smoke-self-bootstrap.md
```

---

## 9. Where `plan` Is Stored

- Execution plan structure (workflow graph): `tasks.execution_plan_json`
- Step output payload (including stdout): `command_runs.output_json`
- Raw output files: `command_runs.stdout_path` and `command_runs.stderr_path`

Quick query:

```bash
sqlite3 ~/.orchestratord/agent_orchestrator.db "
SELECT t.id,
       substr(t.execution_plan_json,1,120) AS execution_plan_json_head,
       r.phase,
       substr(json_extract(r.output_json,'$.stdout'),1,120) AS stdout_head,
       r.stdout_path
FROM tasks t
JOIN task_items i ON i.task_id=t.id
JOIN command_runs r ON r.task_item_id=i.id
WHERE t.id='<task_id>' AND r.phase='plan'
ORDER BY r.started_at DESC
LIMIT 1;"
```

---

## 10. Cleanup

Delete a task:

```bash
orchestrator task delete <task_id> -f
```

Reset project:

```bash
orchestrator delete project/<project> --force
```
