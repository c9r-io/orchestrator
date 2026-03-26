# Command Rules 模板

> **模板用途**：Agent 条件命令选择 + 步骤级变量覆盖 — 展示 command_rules 和 step_vars。

## 适用场景

- Agent 需要根据运行时状态选择不同命令（如首步创建 session、后续步骤复用 session）
- 某些步骤需要隔离的变量环境（如 QA 步骤需要独立 session，不受先入为主影响）
- 需要同一 Agent 在不同步骤使用不同工具或参数的场景

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
  --goal "Demonstrate command rules and step_vars" \
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
init_session (default cmd) → continue_session (rule[0]: resume) → independent_review (rule[1]: fresh, via step_vars)
```

1. **init_session** — 无 session_id 变量 → 所有 rules 不匹配 → 使用默认 command
2. **continue_session** — session_id 已设置 → `command_rules[0]` 匹配 → 使用 resume command
3. **independent_review** — `step_vars: { fresh_session: "true" }` → `command_rules[1]` 匹配 → 使用 fresh session command

### 核心特性：command_rules

```yaml
kind: Agent
spec:
  command: echo 'default command'   # 兜底命令
  command_rules:
    - when: "vars.session_id != ''"      # CEL 条件
      command: echo 'resume session'      # 匹配时使用此命令
    - when: "vars.fresh_session == 'true'"
      command: echo 'fresh session'
```

**匹配语义**：按序评估，首个 `when` 为 true 的规则生效；全部不匹配则回退默认 `command`。

**CEL 上下文**：`vars` map 包含当前 pipeline variables，可访问所有已捕获的步骤输出和 step_vars 覆盖值。

**审计**：匹配的 rule index 记录在 `command_runs.command_rule_index`（NULL = 默认命令）。

### 核心特性：step_vars

```yaml
- id: independent_review
  step_vars:
    fresh_session: "true"    # 仅对此步骤有效
```

**语义**：步骤执行前将 `step_vars` 合并到 pipeline variables 的浅拷贝中，步骤执行后恢复原始值。不影响其他步骤的变量视图。

## 自定义指南

### 替换为真实 Agent（Session 复用场景）

```yaml
command: claude -p "{prompt}" --session-id new --verbose --output-format stream-json
command_rules:
  - when: "vars.session_id != ''"
    command: claude -p "{prompt}" --resume {session_id} --verbose --output-format stream-json
  - when: "vars.fresh_session == 'true'"
    command: claude -p "{prompt}" --session-id new --verbose --output-format stream-json
```

### 按步骤类型切换工具

```yaml
command_rules:
  - when: "vars.step_type == 'test'"
    command: cargo test --workspace 2>&1
  - when: "vars.step_type == 'lint'"
    command: cargo clippy --workspace -- -D warnings 2>&1
```

## 进阶参考

- [Plan & Execute 模板](/zh/showcases/plan-execute) — StepTemplate 和多 Agent 协作基础
- [自举引导执行](/zh/showcases/self-bootstrap-execution-template) — 生产级 session 复用 workflow
- [CEL 前置钩子](/zh/guide/cel-prehooks) — CEL 表达式语法参考
