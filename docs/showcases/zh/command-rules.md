# Driver Session 连续性模板

> **Harness Engineering 模板**：这个 showcase 展示 orchestrator 作为 agent-first 软件交付控制面的一个能力切片，把 agent、workflow、policy 和反馈闭环固化为可复用的工程资产。
>
> **模板用途**：Agent Session 复用与隔离 —— 步骤通过 `behavior.driverRequirements.sessionResume` 显式声明续接 provider session；不声明的步骤自动开新 session。

## 适用场景

- AI Agent（如 Claude Code）支持 session 模式：先行步骤建立上下文，后续步骤续接
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
create_session（新建）→ plan（续接）→ implement（续接）→ qa_testing（新建，隔离）
```

### 逐步拆解

| 步骤 | `sessionResume` | 使用的 session | 效果 |
|------|----------------|---------------|------|
| create_session | 未声明 | 新 session | 建立 provider 上下文 |
| plan | `true` | 续接 | 复用 session 上下文 |
| implement | `true` | 续接 | 在 plan 基础上继续 |
| qa_testing | 未声明 | 新 session | 独立分析，无偏差 |

隔离是默认行为。只有明确声明的步骤才与先前步骤连续。

### 核心机制：`behavior.driverRequirements.sessionResume`

```yaml
- id: plan
  type: plan
  required_capability: plan
  template: plan
  behavior:
    driverRequirements:
      sessionResume: true       # 续接 provider 上下文
      workspaceAccess: write

- id: qa_testing
  type: qa_review
  required_capability: qa_review
  template: qa_review
  behavior:
    driverRequirements:
      workspaceAccess: write    # 未声明 sessionResume → 新 session
```

Provider 的 session 标识符始终不离开 daemon：不从 stdout 捕获、不经变量传递、不出现在任何
manifest 中——步骤声明的是**需求**，由 driver 去满足。若所选 agent 的 driver 不支持续接，apply
会以 `[driver_session_resume_required]` 拒绝该 workflow，使不兼容组合在任务运行前暴露，而不是
悄悄开一个新 session。

### 为什么不再用旧写法

本模板此前用 `behavior.captures` 从 agent 的 stdout 中捕获 session id，用 Agent 级的
`command_rules` CEL 块据此切换命令，再用 `step_vars` 为某一步清空它。三者都把一个 provider
内部标识符送进了 manifest 可见的协调状态：

- `behavior.captures` 已随协调机制坍缩移除 —— `[legacy_coordination_removed]`
- `step_vars` 已随 pipeline 变量授权面一并移除 —— `[legacy_pipeline_variables_removed]`
- `command_rules` 在 `Agent` 上仍然存在，但已不是表达 session 连续性的方式

typed 需求取代了全部三者，这也是为什么隔离现在是默认行为，而不再需要靠清空一个变量来安排。

## 自定义指南

### 隔离更多步骤

不声明 `sessionResume` 即可，没有任何东西需要清空：

```yaml
- id: security_audit
  type: qa_review
  required_capability: qa_review
  behavior:
    driverRequirements:
      workspaceAccess: write
```

### 向步骤传递配置

写进该步骤自己的 `StepTemplate` prompt，或在步骤命令中用
`orchestrator store get <store> <key> --project {project_id}` 从 store 读取。

## 进阶参考

- [Plan & Execute 模板](plan-execute.md) — StepTemplate 和变量传递基础
- [Self-Bootstrap Execution](self-bootstrap-execution-template.md) — 生产级多步骤 workflow
- [Coordination Tools](../../guide/zh/coordination-tools.md) — 旧协调机制的 typed 替代
- [错误码](../../guide/zh/error-codes.md) — `legacy_coordination_removed`、`legacy_pipeline_variables_removed`
