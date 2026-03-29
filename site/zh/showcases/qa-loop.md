# QA Loop 模板

> **Harness Engineering 模板**：这个 showcase 展示 orchestrator 作为 agent-first 软件交付控制面的一个能力切片，把 agent、workflow、policy 和反馈闭环固化为可复用的工程资产。
>
> **模板用途**：QA 测试→修复→回归验证循环 — 展示多步骤 workflow 和 capability 匹配。

## 适用场景

- 对项目文档或代码进行自动化 QA 测试
- 发现问题后自动创建 ticket、修复并回归验证
- 需要 QA → ticket_scan → fix → retest 标准链路的场景

## 前置条件

- `orchestratord` 运行中
- 已执行 `orchestrator init`
- 项目目录下有 `docs/qa/` 和 `docs/ticket/`（可为空）

## 使用步骤

### 1. 部署资源

```bash
orchestrator apply -f docs/workflow/qa-loop.yaml --project qa-loop
```

### 2. 创建并运行任务

```bash
orchestrator task create \
  --name "qa-run" \
  --goal "Run QA cycle" \
  --workflow qa_loop \
  --project qa-loop
```

### 3. 查看结果

```bash
orchestrator task list --project qa-loop
orchestrator task logs <task_id>
```

## 工作流步骤

```
qa (qa-agent) → ticket_scan (builtin) → fix (fix-agent) → retest (fix-agent)
```

1. **qa** — 扫描 `qa_targets` 下的文档，执行测试场景
2. **ticket_scan** — 内置步骤，扫描 `ticket_dir` 下的 ticket 文件
3. **fix** — 修复发现的问题
4. **retest** — 回归验证修复是否生效

### Capability 匹配

- `qa-agent` 拥有 `qa` capability → 被分配到 qa 步骤
- `fix-agent` 拥有 `fix` + `retest` capability → 被分配到 fix 和 retest 步骤

## 自定义指南

### 启用循环模式

将 `loop.mode` 从 `once` 改为 `fixed`，设置 `max_cycles` 实现多轮迭代：

```yaml
loop:
  mode: fixed
  max_cycles: 3
```

### 添加 Loop Guard

在步骤列表末尾添加 loop guard 步骤，让 agent 判断是否需要继续循环：

```yaml
- id: loop_guard
  type: loop_guard
  required_capability: review
  enabled: true
  repeatable: true
```

### 替换为真实 Agent

参见 [Hello World 自定义指南](/zh/showcases/hello-world#替换为真实-agent)。

## 进阶参考

- [全量 QA 执行](/zh/showcases/full-qa-execution) — 生产级全量 QA workflow（含 CEL prehook 安全过滤）
- [工作流配置](/zh/guide/workflow-configuration) — 步骤执行模型与循环策略
