# FR-082: 集成 Manifest 包 — Slack / GitHub / Line 预制配置

## 优先级: P2

## 状态: Proposed

## 背景

FR-080 提供了 webhook trigger 基础设施，FR-081 将提供 per-trigger 认证和 CEL 过滤。下一步是为常见平台提供开箱即用的集成配置包，降低用户接入门槛。

## 需求

### 1. 独立仓库 `c9r-io/orchestrator-integrations`

```
orchestrator-integrations/
├── slack/
│   ├── trigger-message.yaml          # Trigger: webhook + Slack 签名验证
│   ├── trigger-slash-command.yaml    # Trigger: Slack slash command
│   ├── secrets-template.yaml         # SecretStore 模板（Signing Secret）
│   ├── step-template-reply.yaml      # StepTemplate: 解析 Slack payload
│   └── README.md                     # 配置指南 + Slack App 设置步骤
├── github/
│   ├── trigger-push.yaml            # Trigger: push event
│   ├── trigger-pr-opened.yaml       # Trigger: PR opened
│   ├── trigger-issue-comment.yaml   # Trigger: issue comment
│   ├── secrets-template.yaml
│   └── README.md
├── line/
│   ├── trigger-message.yaml
│   ├── secrets-template.yaml
│   └── README.md
└── README.md                         # 总览 + 安装说明
```

### 2. 安装方式

```bash
# 克隆集成仓库
git clone https://github.com/c9r-io/orchestrator-integrations.git

# 配置密钥
cp integrations/slack/secrets-template.yaml my-slack-secrets.yaml
# 编辑填入 Slack Signing Secret

# 一键部署
orchestrator apply -f my-slack-secrets.yaml --project myproject
orchestrator apply -f integrations/slack/ --project myproject
```

### 3. 密钥轮替 Showcase

提供 `docs/showcases/secret-rotation-workflow.md`：
- Cron trigger 定期执行密钥同步 workflow
- Agent 调用平台 API 获取最新密钥
- 通过 `orchestrator apply` 更新 SecretStore

### 4. 每个集成包的标准结构

- `README.md` — 平台侧配置步骤（创建 App、获取 Secret、设置 Webhook URL）
- `secrets-template.yaml` — SecretStore 模板（占位符值）
- `trigger-*.yaml` — 预配置的 Trigger（含 CEL filter 和签名验证）
- `step-template-*.yaml` — 可选的 StepTemplate（平台特定 payload 处理）

## 验收标准

- [ ] `c9r-io/orchestrator-integrations` 仓库已创建
- [ ] Slack 集成包可 `orchestrator apply -f` 一键部署
- [ ] GitHub 集成包可 `orchestrator apply -f` 一键部署
- [ ] 每个集成包有完整的 README 配置指南
- [ ] 密钥轮替 Showcase 可执行

## 依赖

- FR-081 (per-trigger 认证 + CEL filter) 必须先完成
