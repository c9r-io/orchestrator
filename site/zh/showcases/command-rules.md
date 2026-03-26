# Command Rules 模板

> **模板用途**：Agent 条件命令选择 + 步骤级变量覆盖 — 展示 command_rules 和 step_vars。

## 适用场景

- 同一 Agent 在不同步骤使用不同命令（如默认/详细/快速三种模式）
- 某些步骤需要隔离的变量环境（如 QA 步骤清空 session 上下文）
- Agent 根据运行时状态（session 是否存在、当前 cycle 等）选择不同工具

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
  --name "mode-demo" \
  --goal "Demonstrate command rules" \
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
default_analysis (default cmd) → verbose_analysis (rule[0]) → quick_review (rule[1])
```

1. **default_analysis** — 无 step_vars → `run_mode` 不存在 → 所有 rules 不匹配 → 默认 command
2. **verbose_analysis** — `step_vars: { run_mode: "verbose" }` → rule[0] 匹配 → verbose command
3. **quick_review** — `step_vars: { run_mode: "quick" }` → rule[1] 匹配 → quick command

每个步骤的 echo 输出不同（`default-mode` / `verbose-mode` / `quick-mode`），可通过 `task logs` 验证命令切换生效。

### 核心特性：command_rules

```yaml
kind: Agent
spec:
  command: echo 'default mode'        # 兜底命令
  command_rules:
    - when: "run_mode == \"verbose\""  # CEL 条件（变量名直接引用）
      command: echo 'verbose mode'     # 匹配时使用此命令
    - when: "run_mode == \"quick\""
      command: echo 'quick mode'
```

**CEL 变量引用**：pipeline variables 以顶级名称注入 CEL — 直接写 `run_mode == "verbose"`，**不需要** `vars.` 前缀。

**匹配语义**：按序评估，首个 `when` 为 true 的规则生效；全部不匹配则回退默认 `command`。

**审计**：匹配的 rule index 记录在 `command_runs.command_rule_index`（NULL = 默认命令，0 = 第一条规则）。

### 核心特性：step_vars

```yaml
- id: verbose_analysis
  step_vars:
    run_mode: "verbose"    # 仅对此步骤有效
```

**语义**：步骤执行前将 `step_vars` 合并到 pipeline variables 的浅拷贝中，步骤执行后恢复原始值。其他步骤看到的变量不受影响。

**典型用途**：
- 控制 command_rules 匹配（如本模板）
- 清空 session ID 强制新建 session（`step_vars: { session_id: "" }`）
- 注入步骤特定配置（如超时时间、日志级别）

## 自定义指南

### 真实 Agent Session 复用

```yaml
# 默认：创建新 session
command: claude -p "{prompt}" --session-id new --output-format stream-json
command_rules:
  # 有 session → 复用
  - when: "loop_session_id != \"\""
    command: claude -p "{prompt}" --resume {loop_session_id} --output-format stream-json
```

QA 步骤通过 step_vars 清空 session 强制独立分析：
```yaml
- id: qa_testing
  step_vars:
    loop_session_id: ""    # 屏蔽 session 复用
```

### 按步骤类型切换工具

```yaml
command_rules:
  - when: "step_type == \"test\""
    command: cargo test --workspace 2>&1
  - when: "step_type == \"lint\""
    command: cargo clippy --workspace -- -D warnings 2>&1
```

## 进阶参考

- [Plan & Execute 模板](/zh/showcases/plan-execute) — StepTemplate 和多 Agent 协作基础
- [自举引导执行](/zh/showcases/self-bootstrap-execution-template) — 生产级 session 复用 workflow
- [CEL 前置钩子](/zh/guide/cel-prehooks) — CEL 表达式语法参考
