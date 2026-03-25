# FR-083: CRD 插件系统 — Webhook 拦截器与自动化生命周期

## 优先级: P3

## 状态: Proposed

## 背景

FR-081 和 FR-082 通过 per-trigger 认证和 manifest 包解决了短期集成需求。但长期来看，每个平台的签名验证算法、payload 格式、认证流程各有特殊性，硬编码在 core 中不可扩展。

CRD 当前是纯数据/配置层，hooks 仅支持同步 shell 命令。需要扩展 CRD 能力模型，使其能定义平台特定的运行时行为。

## 需求

### 1. CRD Webhook 拦截器

允许 CRD 在 webhook 请求路径上注入自定义逻辑：

```yaml
kind: CustomResourceDefinition
metadata:
  name: SlackWebhook
spec:
  schema:
    properties:
      channel_id: { type: string }
      event_types: { type: array, items: { type: string } }

  webhookInterceptor:
    signatureVerifier: "scripts/verify-slack-sig.sh"
    # 接收环境变量: WEBHOOK_HEADER_*, WEBHOOK_BODY
    # 返回 exit 0 = 验证通过, 非零 = 拒绝

    payloadTransformer: "scripts/normalize-slack-payload.sh"
    # 接收 stdin: 原始 payload
    # 输出 stdout: 标准化后的 JSON
```

- 拦截器在 webhook HTTP handler 中调用，在 trigger 匹配之前执行
- 签名验证器：自定义算法（Slack v0 签名、Line Channel Signature 等）
- Payload 转换器：将平台特定格式标准化为统一 JSON 结构

### 2. CRD 定时任务

允许 CRD 定义周期性后台任务：

```yaml
kind: CustomResourceDefinition
metadata:
  name: SlackWebhook
spec:
  lifecycle:
    cron:
      schedule: "0 0 * * *"
      command: "scripts/rotate-slack-secret.sh"
      # 自动执行密钥轮替、token 刷新等
```

- 由 daemon 的 sweep 机制调度执行
- 适用于：密钥轮替、OAuth token 刷新、健康检查

### 3. CRD 内置工具库

提供一组 CRD 可调用的内置工具：

- `orchestrator-tool secret-rotate <store> <key> <new-value>` — 原子更新 SecretStore
- `orchestrator-tool webhook-verify-hmac <algo> <secret> <body>` — 通用 HMAC 验证
- `orchestrator-tool payload-extract <json-path>` — JSONPath 提取

### 4. CRD 注册机制

CRD apply 时，daemon 自动：
- 注册 webhook 拦截器（如果定义了 `webhookInterceptor`）
- 注册 cron 任务（如果定义了 `lifecycle.cron`）
- CRD delete 时自动注销

## 验收标准

- [ ] CRD 可定义 `webhookInterceptor` 并在 webhook 请求路径上执行
- [ ] 签名验证器脚本可自定义验证算法
- [ ] Payload 转换器可标准化不同平台的 JSON 格式
- [ ] CRD 定时任务由 daemon cron 调度执行
- [ ] 内置工具库可在 CRD hook/script 中调用
- [ ] CRD 删除时自动注销拦截器和定时任务

## 风险

- 拦截器脚本在请求热路径上执行，可能影响延迟 — 需要超时机制
- 长驻进程管理增加 daemon 复杂度 — 先从 cron 开始，不引入长驻进程
- 安全性：CRD 脚本以 daemon 权限运行 — 需要沙箱或权限降级

## 依赖

- FR-081 (per-trigger 认证 + CEL filter)
- FR-082 (集成 manifest 包 — 验证实际需求)
