# Webhook 集成模板

> **Harness Engineering 模板**：这个 showcase 展示 orchestrator 作为 agent-first 软件交付控制面的一个能力切片，把 agent、workflow、policy 和反馈闭环固化为可复用的工程资产。
>
> **模板用途**：Webhook 驱动的外部平台集成 — 展示 Trigger webhook 源、per-trigger 签名认证、CEL payload 过滤、CRD 插件系统和集成 manifest 包。

## 适用场景

- 接收 GitHub push/PR 事件，自动触发代码审查或安全扫描
- 接收 Slack 消息/命令，驱动 agent 响应
- 接收 LINE 消息，驱动客服自动化
- 任何需要 webhook 回调触发 agent 工作流的场景

## 前置条件

- `orchestratord` 运行中，指定 `--webhook-bind <addr>`（默认 `127.0.0.1:19090`）
- 已执行 `orchestrator init`
- 可选：`orchestrator-integrations` 仓库已克隆

## 使用步骤

### 1. 部署集成包（以 GitHub 为例）

```bash
# 克隆集成仓库
git clone https://github.com/c9r-io/orchestrator-integrations.git

# 准备密钥
cp orchestrator-integrations/github/secrets-template.yaml secrets.yaml
# 编辑 secrets.yaml，填入 GitHub Webhook Secret
vim secrets.yaml

# 部署资源
orchestrator apply -f secrets.yaml
orchestrator apply -f orchestrator-integrations/github/trigger-push.yaml
```

### 2. 配置 GitHub Webhook

在 GitHub 仓库 Settings > Webhooks 中添加：
- **Payload URL**: `http://<your-host>:19090/webhook/github-push`
- **Content type**: `application/json`
- **Secret**: 与 SecretStore 中 `webhook_secret` 值一致
- **Events**: `push`

### 3. 手动测试

```bash
# 模拟一个 webhook 请求（带 HMAC 签名）
SECRET="your-webhook-secret"
BODY='{"ref":"refs/heads/main","commits":[{"message":"test"}]}'
SIG=$(echo -n "$BODY" | openssl dgst -sha256 -hmac "$SECRET" -hex | awk '{print "sha256="$NF}')

curl -X POST http://127.0.0.1:19090/webhook/github-push \
  -H "Content-Type: application/json" \
  -H "X-Hub-Signature-256: $SIG" \
  -d "$BODY"
```

### 4. 查看结果

```bash
orchestrator get task
orchestrator task logs <task_id>
```

## 核心特性

### Per-Trigger 签名认证

每个 trigger 可独立配置 SecretStore 引用和签名 header：

```yaml
kind: SecretStore
metadata:
  name: github-webhook
spec:
  data:
    webhook_secret: "your-github-secret"
---
kind: Trigger
metadata:
  name: github-push
spec:
  event:
    source: webhook
    webhook:
      secret:
        fromRef: github-webhook          # 引用 SecretStore
      signatureHeader: X-Hub-Signature-256  # GitHub 签名 header
    filter:
      condition: "payload_ref != ''"     # CEL 过滤
  action:
    workflow: handle-push
    workspace: default
```

签名验证支持多密钥轮替 — SecretStore 中的所有 value 都会依次尝试，任意一个匹配即通过。

### CEL Payload 过滤

用 CEL 表达式精确匹配感兴趣的事件：

```yaml
# 仅匹配 main 分支 push
filter:
  condition: "payload_ref == 'refs/heads/main'"

# Slack: 仅匹配 event_callback 类型
filter:
  condition: "payload_type == 'event_callback'"

# GitHub: 仅匹配 PR opened
filter:
  condition: "payload_action == 'opened'"
```

Webhook JSON body 的顶层字段自动注入为 `payload_<field>` CEL 变量。

### CRD 插件系统认证

对于 HMAC-SHA256 无法覆盖的认证场景（如 Slack v0 签名算法、自定义 token 验证），可通过 CRD 插件定义自定义认证逻辑：

```yaml
kind: CustomResourceDefinition
metadata:
  name: slackintegrations.integrations.orchestrator.dev
spec:
  kind: SlackIntegration
  plural: slackintegrations
  group: integrations.orchestrator.dev
  versions:
    - name: v1
      schema:
        type: object
  plugins:
    # 自定义签名验证 — 替代内置 HMAC
    - name: verify-slack-v0
      type: interceptor
      phase: webhook.authenticate
      command: "scripts/verify-slack-v0-sig.sh"
      timeout: 5

    # Payload 标准化 — 将 Slack 特有格式转为统一 JSON
    - name: normalize-payload
      type: transformer
      phase: webhook.transform
      command: "scripts/normalize-slack-payload.sh"
      timeout: 5

    # 定期令牌刷新
    - name: refresh-token
      type: cron
      schedule: "0 */6 * * *"
      command: "scripts/refresh-slack-token.sh"
```

Trigger 通过 `crdRef` 关联 CRD 插件：

```yaml
kind: Trigger
metadata:
  name: slack-events
spec:
  event:
    source: webhook
    webhook:
      crdRef: SlackIntegration    # 启用 CRD 插件
  action:
    workflow: handle-slack
    workspace: default
```

**插件执行流程**：

```
Webhook 请求到达
  -> CRD interceptor (webhook.authenticate) — 自定义认证
  -> 解析 JSON body
  -> CRD transformer (webhook.transform) — payload 标准化
  -> CEL filter — 事件过滤
  -> 触发 Workflow
```

### 内置工具库

CRD 插件脚本可调用 `orchestrator tool` 内置工具：

```bash
# HMAC 签名验证
orchestrator tool webhook-verify-hmac \
  --secret "$SECRET" --body "$BODY" --signature "$SIG"

# JSON 路径提取（从 stdin 读取）
echo '{"event":{"type":"message"}}' | \
  orchestrator tool payload-extract --path event.type

# SecretStore 密钥原子更新
orchestrator tool secret-rotate my-store my-key --value "new-secret"
```

## 可用集成包

| 平台 | 签名 Header | 仓库路径 |
|------|------------|---------|
| GitHub | `X-Hub-Signature-256` | `orchestrator-integrations/github/` |
| Slack | `X-Slack-Signature` | `orchestrator-integrations/slack/` |
| LINE | `X-Line-Signature` | `orchestrator-integrations/line/` |

每个集成包包含：
- `secrets-template.yaml` — SecretStore 模板
- `trigger-*.yaml` — 预配置的 webhook trigger
- `step-template-*.yaml` — 可选的 payload 解析 StepTemplate
- `README.md` — 平台特定的设置指南

## 自定义指南

### 添加新平台集成

1. 创建 SecretStore（平台签名密钥）
2. 创建 Trigger（webhook source + SecretStore 引用）
3. 如需自定义认证，创建 CRD + 插件脚本

### 多 Webhook 共存

同一个 daemon 可同时接收多个平台的 webhook，每个 trigger 独立配置认证：

```
POST /webhook/github-push     -> github-webhook SecretStore
POST /webhook/slack-events     -> slack-signing SecretStore + CRD 插件
POST /webhook/line-message     -> line-channel SecretStore
```

## 进阶参考

- [Scheduled Scan 模板](scheduled-scan) — Cron Trigger 示例
- [FR Watch 模板](fr-watch) — Filesystem Trigger + CEL 过滤示例
- [密钥轮替 Workflow](secret-rotation-workflow) — 密钥轮替示例
