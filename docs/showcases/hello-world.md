# Hello World 模板

> **Harness Engineering 模板**：这个 showcase 展示 orchestrator 作为 agent-first 软件交付控制面的一个能力切片，把 agent、workflow、policy 和反馈闭环固化为可复用的工程资产。
>
> **模板用途**：最小可运行 workflow — 一个 Workspace、一个 Agent、一个 Workflow，零 API 成本。

## 适用场景

- 初次接触 orchestrator，验证安装和基本流程
- 快速了解 Workspace → Agent → Workflow 的资源关系
- 作为自定义 workflow 的起点骨架

## 前置条件

- `orchestratord` 运行中（`orchestratord --foreground --workers 2`）
- 已执行 `orchestrator init`

## 使用步骤

### 1. 部署资源

```bash
orchestrator apply -f docs/workflow/hello-world.yaml --project hello-world
```

### 2. 确认资源已加载

```bash
orchestrator get workspaces --project hello-world
orchestrator get agents --project hello-world
orchestrator get workflows --project hello-world
```

### 3. 创建并运行任务

```bash
orchestrator task create \
  --name "hello" \
  --goal "Say hello" \
  --workflow hello \
  --project hello-world
```

### 4. 查看结果

```bash
orchestrator task list --project hello-world
orchestrator task info <task_id>
orchestrator task logs <task_id>
```

## 预期输出

echo agent 返回固定 JSON：

```json
{
  "confidence": 0.95,
  "quality_score": 0.9,
  "artifacts": [{
    "kind": "analysis",
    "findings": [{
      "title": "hello-world",
      "description": "Workflow executed successfully.",
      "severity": "info"
    }]
  }]
}
```

任务应在数秒内完成，状态变为 `Completed`。

## 自定义指南

### 替换为真实 Agent

将 echo agent 的 `command` 替换为真实 AI agent：

```yaml
# Claude Code
command: claude -p "{prompt}" --verbose --output-format stream-json

# OpenCode
command: opencode -p "{prompt}"
```

替换后需配置对应的 API key（通过 SecretStore 或环境变量）。

### 添加更多步骤

在 Workflow 的 `steps` 中添加新步骤，并确保 Agent 拥有对应的 `capability`。

## 进阶参考

- [Quick Start](../guide/01-quickstart.md) — 完整的 5 分钟上手教程
- [Resource Model](../guide/02-resource-model.md) — 深入理解资源模型
- [QA Loop 模板](qa-loop.md) — 下一步：多步骤 workflow
