# Plan-Execute 模板

> **Harness Engineering 模板**：这个 showcase 展示 orchestrator 作为 agent-first 软件交付控制面的一个能力切片，把 agent、workflow、policy 和反馈闭环固化为可复用的工程资产。
>
> **模板用途**：计划→实现→验证三阶段迭代 — 展示 StepTemplate、多 Agent 协作和变量传递。

## 适用场景

- 任何需要"先规划、再实现、后验证"的开发任务
- 功能开发、Bug 修复、重构等软件工程场景
- 需要将计划与执行分离，由不同 Agent 各司其职的场景

## 前置条件

- `orchestratord` 运行中
- 已执行 `orchestrator init`

## 使用步骤

### 1. 部署资源

```bash
orchestrator apply -f docs/workflow/plan-execute.yaml --project plan-exec
```

### 2. 创建并运行任务

```bash
orchestrator task create \
  --name "my-feature" \
  --goal "Implement user authentication with JWT tokens" \
  --workflow plan_execute \
  --project plan-exec
```

### 3. 查看结果

```bash
orchestrator task list --project plan-exec
orchestrator task logs <task_id>
```

## 工作流步骤

```
plan (planner) → implement (coder) → verify (coder)
```

1. **plan** — 由 planner agent 生成实现计划，输出被自动捕获
2. **implement** — 由 coder agent 按计划实施，通过 `{plan_output_path}` 读取上一步计划
3. **verify** — 由 coder agent 验证实现是否符合计划

### 核心特性：StepTemplate

每个步骤使用独立的 StepTemplate 定义 prompt，与 Agent 解耦：

```yaml
kind: StepTemplate
metadata:
  name: plan
spec:
  prompt: >-
    You are working on a project at {source_tree}.
    Your task: create a detailed implementation plan for: {goal}.
    ...
```

**Pipeline 变量**（自动注入）：
- `{goal}` — task 创建时指定的目标
- `{source_tree}` — Workspace 的 root_path
- `{diff}` — 当前 cycle 的 git diff
- `{plan_output_path}` — plan 步骤输出的文件路径

### 核心特性：多 Agent 协作

- **planner** agent（capability: `plan`）— 专注规划
- **coder** agent（capability: `implement`, `verify`）— 专注编码和验证

Orchestrator 根据步骤的 `required_capability` 自动匹配 Agent。

## 自定义指南

### 添加 QA 步骤

在 verify 之后添加 QA 测试步骤：

```yaml
- id: qa_testing
  type: qa_testing
  scope: item
  required_capability: qa
  template: qa_testing
  enabled: true
```

`scope: item` 表示按 QA 文件扇出并行执行。

### 启用 2-Cycle 模式

第 1 轮实现，第 2 轮回归验证：

```yaml
loop:
  mode: fixed
  max_cycles: 2
```

### 添加 Prehook 条件控制

使用 CEL 表达式按条件启用步骤：

```yaml
- id: verify
  ...
  prehook:
    engine: cel
    when: "cycle == 2"
    reason: "Only verify in the second cycle"
```

## 进阶参考

- [Self-Bootstrap Execution](self-bootstrap-execution-template.md) — 生产级计划-执行-验证 workflow（8 个 StepTemplate + 4 个 Agent + CEL prehook）
- [CEL Prehooks](../guide/04-cel-prehooks.md) — 动态控制流详解
- [Workflow Configuration](../guide/03-workflow-configuration.md) — scope、loop、safety 配置
