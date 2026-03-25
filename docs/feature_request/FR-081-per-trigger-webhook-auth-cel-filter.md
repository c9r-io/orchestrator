# FR-081: Per-Trigger Webhook 认证与 CEL Payload 过滤

## 优先级: P1

## 状态: Proposed

## 背景

FR-080 引入了全局 webhook endpoint，但认证（`--webhook-secret`）是全局共享的。实际场景中，Slack、GitHub、Line 等平台各有独立的签名密钥和验证机制，需要 per-trigger 级别的认证配置。同时，CEL filter 在 trigger engine 中已预留但未实现，需要补全以支持 payload 条件过滤。

## 需求

### 1. Per-Trigger Webhook 认证

在 `TriggerEventConfig` 中新增 `webhook` 配置块：

```yaml
kind: Trigger
metadata:
  name: slack-message
spec:
  event:
    source: webhook
    webhook:
      secret:
        fromRef: slack-signing-secret   # 从 SecretStore 读取，遍历所有 key 尝试验证
      signatureHeader: X-Slack-Signature  # 自定义签名 header 名
  action:
    workflow: handle-slack
    workspace: default
```

- `webhook.secret.fromRef` 引用 SecretStore，验证时遍历 store 中所有 value 依次尝试
- `webhook.signatureHeader` 自定义签名 header（默认 `X-Webhook-Signature`）
- 支持密钥轮替：在 SecretStore 中同时保留新旧密钥，任一命中即通过
- 全局 `--webhook-secret` 作为 fallback（per-trigger 优先）

### 2. CEL Payload 过滤

实现 `filter.condition` 中的 CEL 表达式求值：

```yaml
spec:
  event:
    source: webhook
    filter:
      condition: "payload.event.type == 'message' && payload.event.channel == 'C12345'"
```

- CEL 上下文变量：`payload`（webhook JSON body）、`headers`（HTTP headers）
- 表达式返回 true 才触发任务创建
- 复用现有 `cel-interpreter` 依赖

### 3. 全局 secret 降级为 fallback

- 如果 trigger 配置了 `webhook.secret`，使用 per-trigger 验证
- 如果没有配置，使用全局 `--webhook-secret`（如果设置了）
- 两者都没有时，不做签名验证

## 验收标准

- [ ] Per-trigger secret 从 SecretStore 读取并验证签名
- [ ] 多密钥轮替：SecretStore 中多个 key，任一匹配即通过
- [ ] 自定义 `signatureHeader` 生效
- [ ] CEL `filter.condition` 正确过滤 webhook payload
- [ ] 全局 secret 作为 fallback 正常工作
- [ ] 现有无认证 webhook trigger 不受影响
