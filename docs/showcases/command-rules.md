# Command Rules 模板

> **模板用途**：Agent Session 复用与隔离 — 通过 command_rules 和 step_vars 实现跨步骤 session 共享，同时让 QA 步骤独立分析。

## 适用场景

- AI Agent（如 Claude Code）支持 session 模式：首步创建 session，后续步骤 `--resume` 续接上下文
- 计划和实现步骤需要共享 session 上下文（plan 的输出是 implement 的输入前提）
- QA 步骤需要独立 session，避免先入为主的偏差

## 前置条件

- `orchestratord` 运行中
- 已执行 `orchestrator init`

## 使用步骤

### 1. 部署资源

```bash
orchestrator apply -f docs/workflow/command-rules.yaml --project cmd-rules
```

### 2. 创建并运行任务

```bash
orchestrator task create \
  --name "session-demo" \
  --goal "Demonstrate session reuse" \
  --workflow command_rules \
  --project cmd-rules
```

### 3. 查看结果

```bash
orchestrator task list --project cmd-rules
orchestrator task logs <task_id>
```

## 工作流步骤

```
create_session (new) → plan (resume) → implement (resume) → qa_testing (new, isolated)
```

### 步骤详解

| 步骤 | loop_session_id | command_rules 匹配 | 使用的 command | 效果 |
|------|----------------|-------------------|--------------|------|
| create_session | 不存在 | 无匹配 → 默认 | 新建 session | 输出含 `session_id`，capture 到 pipeline vars |
| plan | `"ses-abc-123"` | rule[0] ✓ | resume session | 复用 session 上下文 |
| implement | `"ses-abc-123"` | rule[0] ✓ | resume session | 在 plan 基础上继续 |
| qa_testing | `""` (step_vars 清空) | 无匹配 → 默认 | 新建 session | 独立分析，无先入为主偏差 |

QA 步骤结束后，`loop_session_id` 恢复为 `"ses-abc-123"`（step_vars 只是临时覆盖）。

### 核心机制 1：behavior.captures

```yaml
- id: create_session
  behavior:
    captures:
      - var: loop_session_id     # 写入 pipeline 变量名
        source: stdout           # 从 stdout 提取
        json_path: "$.session_id"  # JSON path 定位
```

Agent 输出 `{"session_id":"ses-abc-123",...}`，capture 自动提取 `session_id` 字段并写入 `loop_session_id` pipeline 变量。后续所有步骤都能访问这个变量。

### 核心机制 2：command_rules

```yaml
kind: Agent
spec:
  command: echo 'new session'          # 默认：创建新 session
  command_rules:
    - when: "loop_session_id != \"\""  # CEL 条件：session 存在
      command: echo 'resumed session'  # 匹配时：复用 session
```

- Pipeline variables 以**顶级名称**注入 CEL（直接写 `loop_session_id`，不需要 `vars.` 前缀）
- 按序评估，首个 true 生效；全不匹配则用默认 `command`
- 匹配的 rule index 记录在 `command_runs.command_rule_index` 用于审计

### 核心机制 3：step_vars

```yaml
- id: qa_testing
  step_vars:
    loop_session_id: ""    # 临时清空 → 强制新 session
```

- 步骤执行前将 `step_vars` 合并到 pipeline vars 的浅拷贝
- 步骤执行后恢复原始值（`loop_session_id` 回到 `"ses-abc-123"`）
- 只影响当前步骤的输入视图，不影响全局 pipeline 状态

## 自定义指南

### 替换为真实 Agent（Claude Code Session）

```yaml
# 默认：创建新 session
command: claude -p "{prompt}" --session-id new --output-format stream-json

command_rules:
  # 有 session → 复用
  - when: "loop_session_id != \"\""
    command: claude -p "{prompt}" --resume {loop_session_id} --output-format stream-json
```

### 更多 step_vars 隔离场景

```yaml
# 安全审计步骤：独立 session + 额外安全指令
- id: security_audit
  step_vars:
    loop_session_id: ""           # 独立 session
    audit_mode: "strict"          # 注入审计配置
```

## 进阶参考

- [Plan & Execute 模板](plan-execute.md) — StepTemplate 和变量传递基础
- [Self-Bootstrap Execution](self-bootstrap-execution-template.md) — 生产级多步骤 workflow
- [CEL Prehooks](../guide/04-cel-prehooks.md) — CEL 表达式语法参考
