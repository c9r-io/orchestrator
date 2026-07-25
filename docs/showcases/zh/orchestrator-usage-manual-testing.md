# Orchestrator 使用指南（手动测试）

> **Harness Engineering 执行计划**：本文档是一个 agent 可执行场景，用来展示 orchestrator 这个 control plane 如何组织环境、工作流、约束与反馈闭环，而不是一次性的 prompt 调用。
>
> **Agent 协作**：本文档是一个 Agent 可执行的计划。在 AI 编码 Agent（Claude Code、OpenCode、Codex 等）中打开本项目，Agent 读取本计划后，通过 orchestrator CLI 调度其他 Agent 协作完成任务 — 从资源部署、任务执行到结果验证，全程自主完成。

本文档是一份面向实操的指南，用于从 CLI 手动测试 orchestrator，可直接复制粘贴使用。

入口：`orchestrator`（CLI 客户端）+ `orchestratord`（daemon）。

---

## 1. 前置条件

在仓库根目录执行：

```bash
cd "$ORCHESTRATOR_ROOT"   # your orchestrator project directory
cargo build --release -p orchestratord -p orchestrator-cli
```

启动 daemon：

```bash
orchestratord --foreground --workers 2
```

验证 CLI 接口：

```bash
orchestrator --help
orchestrator task --help
```

---

## 2. 清理运行时状态

```bash
orchestrator delete project/<project> --force
orchestrator init
```

运行时数据位置：
- SQLite 数据库：`~/.orchestratord/agent_orchestrator.db`
- 日志：`data/logs/`

---

## 3. 应用 Self-Bootstrap Workflow

```bash
orchestrator manifest validate -f docs/workflow/self-bootstrap.yaml
# ⚠️  必须使用 --project，否则真实 AI agent 会注册到全局空间
orchestrator apply -f docs/workflow/execution-profiles.yaml --project self-bootstrap
orchestrator apply -f docs/workflow/self-bootstrap.yaml --project self-bootstrap
orchestrator get workflow
orchestrator get agent
orchestrator get workspace
```

预期结果：
- workspace `self`
- workflow `self-bootstrap`
- agents `architect`、`coder`、`tester`、`reviewer`

---

## 4.（可选）低成本冒烟 Workflow

用于快速/低成本验证，使用一个 3 步流程（`plan -> qa_doc_gen -> implement`）：

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

## 5. 创建并运行 Task

重要说明：
- item 作用域的 workflow 默认扫描 workspace `qa_targets` 下的 QA/Security markdown 文件
- 仅 task 作用域的 workflow 可以在没有 QA markdown 的情况下创建；orchestrator 会使用合成的 `__UNASSIGNED__` 锚定 item
- 显式指定 `--target-file` 值会覆盖默认来源

创建 task（不自动启动）：

```bash
orchestrator task create --project self-bootstrap \
  -n self-bootstrap-manual \
  -w self \
  -W self-bootstrap-smoke \
  --no-start \
  -g "SMOKE RUN: create docs/qa/self-bootstrap/smoke-self-bootstrap.md with marker SB_SMOKE_20260226; keep changes minimal; do not modify core/src/**" \
  -t docs/qa/orchestrator/26-self-bootstrap-workflow.md
```

启动 task：

```bash
orchestrator task start <task_id>
```

查看摘要：

```bash
orchestrator task list -o json
orchestrator task info <task_id> -o json
```

查看日志：

```bash
orchestrator task logs <task_id> --tail 50
```

---

## 6. 验证步骤执行（Events + Runs）

检查步骤顺序：

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

检查 run 详情和日志文件路径：

```bash
sqlite3 ~/.orchestratord/agent_orchestrator.db "
SELECT id, phase, agent_id, exit_code, validation_status, started_at, ended_at, stdout_path, stderr_path
FROM command_runs
WHERE task_item_id=(SELECT id FROM task_items WHERE task_id='<task_id>' ORDER BY order_no LIMIT 1)
ORDER BY started_at;"
```

---

## 7. 验证 `plan_output` 传播

`plan` 的输出应被注入到下游 `qa_doc_gen`/`implement` 的命令中。

检查命令文本：

```bash
sqlite3 ~/.orchestratord/agent_orchestrator.db "
SELECT phase, command
FROM command_runs
WHERE task_item_id=(SELECT id FROM task_items WHERE task_id='<task_id>' ORDER BY order_no LIMIT 1)
  AND phase IN ('qa_doc_gen','implement')
ORDER BY started_at;"
```

预期结果：
- 命令中包含具体的计划文本
- 命令中不包含字面量 `{plan_output}`

---

## 8. 验证生成的产出物

```bash
ls -la docs/qa/self-bootstrap/smoke-self-bootstrap.md
rg -n 'SB_SMOKE_20260226' docs/qa/self-bootstrap/smoke-self-bootstrap.md
sed -n '1,120p' docs/qa/self-bootstrap/smoke-self-bootstrap.md
```

---

## 9. `plan` 的存储位置

- 执行计划结构（workflow 图）：`tasks.execution_plan_json`
- 步骤输出 payload（包括 stdout）：`command_runs.output_json`
- 原始输出文件：`command_runs.stdout_path` 和 `command_runs.stderr_path`

快速查询：

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

## 10. 清理

删除 task：

```bash
orchestrator task delete <task_id> -f
```

重置 project：

```bash
orchestrator delete project/<project> --force
```
