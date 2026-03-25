# 密钥自动轮替 Workflow 执行计划

> **Agent 协作**：本文档是一个 Agent 可执行的计划。在 AI 编码 Agent（Claude Code、OpenCode、Codex 等）中打开本项目，Agent 读取本计划后，通过 orchestrator CLI 调度其他 Agent 协作完成任务 — 从资源部署、任务执行到结果验证，全程自主完成。

## 1. 目标

配置一个 cron trigger，定期执行密钥同步 workflow。Agent 调用平台 API 获取最新密钥，通过 `orchestrator apply` 更新 SecretStore，实现密钥自动轮替。

## 2. 适用场景

- Slack Signing Secret 定期更换
- GitHub Webhook Secret 轮替
- API token 过期前自动刷新
- 任何需要定期更新 SecretStore 的场景

## 3. 前置条件

- orchestratord 运行中，已配置 `--webhook-bind`
- 已部署对应平台的集成包（`orchestrator-integrations`）
- Agent 有权限调用平台 API（API token 在 SecretStore 中）

## 4. Manifest 示例

### 4.1 密钥轮替 StepTemplate

```yaml
apiVersion: orchestrator.dev/v2
kind: StepTemplate
metadata:
  name: rotate-secrets
spec:
  description: "Rotate platform signing secrets"
  prompt: >-
    Check the current signing secrets for all configured integrations.
    For each platform:
    1. Call the platform API to verify the current secret is still valid
    2. If the platform supports secret regeneration, generate a new secret
    3. Update the SecretStore via: orchestrator apply -f <updated-secrets.yaml>
    4. Verify the new secret works by sending a test webhook
    5. Report which secrets were rotated and which remain unchanged
```

### 4.2 Cron Trigger（每周执行）

```yaml
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: weekly-secret-rotation
spec:
  cron:
    schedule: "0 2 * * 0"    # 每周日凌晨 2 点
    timezone: "Asia/Tokyo"
  action:
    workflow: secret-rotation
    workspace: default
    start: true
```

### 4.3 完整 Workflow

```yaml
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: secret-rotation
spec:
  max_parallel: 1
  steps:
    - id: rotate
      scope: task
      required_capability: plan
      template: rotate-secrets
      enabled: true
      repeatable: false
  loop:
    mode: fixed
    max_cycles: 1
```

## 5. 执行流程

1. Cron trigger 按计划触发 → 创建 task
2. Agent 执行 `rotate-secrets` step：
   - 读取当前 SecretStore 配置
   - 调用平台 API 验证/轮替密钥
   - 生成更新后的 SecretStore YAML
   - `orchestrator apply -f` 更新 SecretStore
3. 新密钥立即生效（webhook handler 每次请求都读取最新 config）
4. Task 完成，记录轮替结果

## 6. 注意事项

- 密钥轮替期间，SecretStore 中同时保留新旧密钥（multi-key rotation）
- 确认新密钥有效后再移除旧密钥
- 建议在低流量时段执行（cron schedule 设置为凌晨）
- Agent 需要平台 API 的管理权限
