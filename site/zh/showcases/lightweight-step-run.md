# 轻量化单步执行模板

> **Harness Engineering 模板**：这个 showcase 展示 orchestrator 作为 agent-first 软件交付控制面的一个能力切片，把 agent、workflow、policy 和反馈闭环固化为可复用的工程资产。
>
> **模板用途**：展示三种轻量化执行模式 — 步骤过滤、同步执行、直接组装 — 让用户无需创建完整 workflow 即可按需点射单步。

## 适用场景

- 在多步 workflow 中只想重跑某一步（如单独执行 `fix` 修复一个 ticket）
- 需要向步骤注入临时变量（如指定 ticket 路径）
- 希望同步等待执行结果，而非异步轮询
- 想直接引用 StepTemplate + Agent 能力执行即时任务，不依赖已有 workflow

## 前置条件

- `orchestratord` 运行中（`orchestratord --foreground --workers 2`）
- 已执行 `orchestrator init`
- 已部署包含多步骤的 workflow（如 `sdlc`）

## 使用步骤

### 1. 部署多步 workflow 资源

以 plan-execute 模板为例：

```bash
orchestrator apply -f docs/workflow/plan-execute.yaml --project demo
```

确认资源加载：

```bash
orchestrator get workflows --project demo
orchestrator get agents --project demo
```

### 2. Phase 1 — 步骤过滤（`--step` + `--set`）

只执行 workflow 中 `id=implement` 的步骤，同时注入自定义变量：

```bash
# 只执行 implement 步骤
orchestrator task create \
  --workflow plan-execute \
  --project demo \
  --step implement

# 注入 pipeline 变量
orchestrator task create \
  --workflow plan-execute \
  --project demo \
  --step fix \
  --set ticket_paths=docs/ticket/T-0042.md

# 多个步骤按 workflow 顺序执行
orchestrator task create \
  --workflow plan-execute \
  --project demo \
  --step plan --step implement
```

查看执行结果：

```bash
orchestrator task list --project demo
orchestrator task logs <task_id>
```

**错误处理**：指定不存在的步骤 ID 时，CLI 返回明确错误：

```bash
orchestrator task create --workflow plan-execute --step nonexistent
# Error: unknown step id 'nonexistent' in --step filter; available steps: plan, implement, verify
```

### 3. Phase 2 — 同步执行（`orchestrator run`）

`run` 命令创建任务后自动 follow 日志，等待完成并返回退出码：

```bash
# 同步执行，日志直接输出到终端
orchestrator run \
  --workflow plan-execute \
  --project demo \
  --step implement \
  --set goal="修复登录模块的并发问题"

# 后台执行（回退到 task create 行为）
orchestrator run \
  --workflow plan-execute \
  --project demo \
  --step implement \
  --detach
```

`run` 命令行为：
1. 创建 task → 自动 follow 日志 → 等待完成
2. 终端输出 agent 实时日志
3. 任务完成后输出最终状态，exit 0（completed）或 exit 1（failed）
4. `--detach` 退化为 `task create`，打印 task ID 后立即返回

### 4. Phase 3 — 直接组装模式（脱离 Workflow）

不依赖已有 workflow，直接引用 StepTemplate + Agent 能力执行：

```bash
# 直接指定 template 和 agent 能力
orchestrator run \
  --template fix-ticket \
  --agent-capability fix \
  --project demo \
  --set ticket_paths=docs/ticket/T-0042.md

# 指定 execution profile
orchestrator run \
  --template fix-ticket \
  --agent-capability fix \
  --profile host-unrestricted \
  --project demo \
  --set ticket_paths=docs/ticket/T-0042.md
```

直接组装模式内部构造一个单步 `TaskExecutionPlan`，复用已 apply 到 workspace 中的 StepTemplate、Agent、ExecutionProfile 资源。

## 执行流程

```
Phase 1: task create --step fix --set key=val
         ↓
         步骤过滤 → 仅执行指定步骤
         变量注入 → 注入为 pipeline variables

Phase 2: orchestrator run --workflow X --step fix
         ↓
         创建 task → follow 日志 → 等待完成 → exit code

Phase 3: orchestrator run --template T --agent-capability C
         ↓
         构造单步 plan → 创建 ephemeral task → 执行
```

### 核心特性：步骤过滤

`--step` 指定的步骤 ID 在 daemon 端验证，不在执行计划中的 ID 被拒绝。
过滤后的步骤按 workflow 中原始顺序执行，scope 分区（task/item）机制不变。

### 核心特性：变量注入

`--set key=value` 注入的变量在 task 启动时合并到 pipeline variables，
可在 StepTemplate prompt（`{key}`）和 prehook CEL 表达式中引用。

### 核心特性：安全性保证

- ExecutionProfile sandbox 保护不被绕过
- 所有 `run` 执行产生 RunResult 记录，可通过 `event list` 查阅
- 审计日志完整，等同于常规 task 执行

## 自定义指南

### 为现有 workflow 添加点射能力

无需修改任何 YAML — `--step` 和 `--set` 直接作用于已有 workflow：

```bash
# 在 sdlc workflow 中只执行 qa 步骤
orchestrator run --workflow sdlc --step qa --project my-project

# 在 sdlc workflow 中执行 qa + fix 步骤
orchestrator run --workflow sdlc --step qa --step fix --project my-project
```

### 创建可直接组装的 StepTemplate

```yaml
apiVersion: orchestrator.dev/v2
kind: StepTemplate
metadata:
  name: fix-ticket
spec:
  prompt: |
    修复以下 ticket 中描述的问题。
    Ticket 路径: {ticket_paths}
    项目目标: {goal}
    源码根目录: {source_tree}
```

然后直接执行：

```bash
orchestrator run \
  --template fix-ticket \
  --agent-capability fix \
  --set ticket_paths=docs/ticket/T-0042.md
```

## 进阶参考

- [Plan-Execute 模板](/zh/showcases/plan-execute) — 多步 workflow 模板，配合 `--step` 实现部分执行
- [Hello World 模板](/zh/showcases/hello-world) — 最小可运行 workflow
- [CLI 参考](/zh/guide/cli-reference) — `run` 命令完整参数说明
- [工作流配置](/zh/guide/workflow-configuration) — Step 定义、scope、loop policy 详解
