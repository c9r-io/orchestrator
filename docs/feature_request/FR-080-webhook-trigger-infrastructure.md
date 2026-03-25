# FR-080: Webhook Trigger 基础设施 — HTTP 事件入口与通用事件源扩展

## 优先级: P0

## 状态: Proposed

## 背景

当前 Trigger 系统仅支持两种事件源：cron 定时和 task 生命周期事件（task_completed / task_failed）。Daemon 只暴露 gRPC 接口，无法接收外部 HTTP 回调。

这限制了与外部系统（Slack、GitHub Webhooks、CI/CD 管道等）的集成能力。需要一个通用的 webhook 入口，让外部系统可以通过 HTTP POST 触发任务创建。

## 需求

### 1. Daemon HTTP Webhook Endpoint

- 在 daemon 中新增一个轻量 HTTP 服务（与 gRPC 并行运行）
- 暴露 `POST /webhook/{trigger_name}` 端点
- 可配置绑定地址：`--webhook-bind <ADDR>`（默认禁用，显式启用才开放）
- 接收任意 JSON payload，广播到 trigger 事件通道

### 2. Trigger 事件源扩展

- `TriggerEventConfig.source` 新增 `webhook` 类型
- Webhook trigger 配置示例：

```yaml
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: on-slack-message
spec:
  event:
    source: webhook
    filter:
      expression: "payload.event.type == 'message' && payload.event.channel == 'C12345'"
  taskTemplate:
    workflow: respond-to-slack
    goal: "Handle Slack message: {payload.event.text}"
  historyLimit:
    successful: 10
    failed: 5
```

### 3. Webhook 安全

- 可选的共享密钥验证：`--webhook-secret <SECRET>` 或 per-trigger `spec.event.webhookSecret`
- 支持 HMAC-SHA256 签名验证（`X-Webhook-Signature` header）
- 无签名时返回 401（如果配置了 secret）

### 4. Payload 传递

- Webhook 的 JSON payload 通过 trigger 事件通道传递到 TriggerEngine
- CEL filter 表达式可访问 `payload.*` 字段
- `taskTemplate.goal` 支持 `{payload.xxx}` 变量替换

### 5. CLI 支持

- `orchestrator trigger fire <name> --payload '{"key":"value"}'` — 模拟 webhook 触发（用于测试）
- `orchestrator daemon status` 显示 webhook 端口状态

## 验收标准

- [ ] `orchestratord --webhook-bind 0.0.0.0:9090` 启动 HTTP webhook 服务
- [ ] `curl -X POST http://localhost:9090/webhook/on-slack-message -d '{"event":{"type":"message"}}'` 成功触发任务创建
- [ ] CEL filter 正确过滤不匹配的 payload
- [ ] `--webhook-secret` 启用后，无签名请求返回 401
- [ ] `orchestrator trigger fire <name> --payload '{...}'` 可模拟触发
- [ ] 不配置 `--webhook-bind` 时，HTTP 服务不启动（零开销）
- [ ] 现有 cron 和 task_completed/task_failed 触发器不受影响

## 风险

- HTTP 端口暴露增加攻击面 — 默认禁用 + 签名验证缓解
- Webhook payload 可能包含大量数据 — 需要 body size limit（默认 1MB）
- 高频 webhook 可能导致大量任务创建 — 需要 rate limiting 或 debounce 机制（可后续 FR）

## 不包含

- 具体的 Slack/GitHub/Jira 适配逻辑 — 这些放在独立的 `c9r-io/orchestrator-integrations` 仓库
- Webhook 重试/回调确认 — 调用方负责重试
- WebSocket 长连接 — 仅 HTTP POST
