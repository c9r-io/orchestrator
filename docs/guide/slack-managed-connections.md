# 使用官方 Orchestrator Slack App 一键连接 Workspace

这份指南面向两类人：负责部署 Gateway/官方 Slack App 的平台管理员，以及只需要在界面里连接 workspace、配置 badge 自动化的项目管理员。

如果你的目标只是“在 Slack 消息上加 reaction，然后创建一个带 Skill 的任务”，推荐使用这里的 **Instant — Official Orchestrator App** 路径。你不需要复制 Signing Secret 或 Bot Token。

## 先理解三个选项

打开 **Sources → Connections**，会看到三张卡片：

| 方式 | 适合谁 | 当前状态 |
|---|---|---|
| Instant — Official Orchestrator App | 希望一次 OAuth 授权就完成连接 | 已实现，需要部署 Gateway |
| Dedicated — Private app for this workspace | 希望每个 workspace 有独立 Slack App | FR-115，当前会明确显示不可用 |
| Existing app — Manual credentials | 已经有自己的 Slack App，或完全本地部署 | 保持兼容，参考手工 Slack 指南 |

Instant 模式使用同一个官方 App 服务多个 workspace，但每次安装拥有独立 token、pairing、owner daemon 和 project。Gateway 只处理 OAuth、Slack 签名验证、可靠投递与最小 permalink 代理；badge 匹配、Skill、模板和任务仍在本地 daemon 中执行。

## 普通项目管理员：连接一个 Workspace

### 1. 确认服务可用

在 **Sources → Connections** 中，Instant 卡片应显示可用。也可以用 CLI 检查：

```bash
orchestrator source connection catalog -o json
```

你应该看到 `managed_shared` 可用、`gateway_configured: true`。如果不可用，先让平台管理员完成后文的 Gateway 配置。

### 2. 发起 OAuth

在界面中选择 project，填写一个仅供本地识别的名称，然后点击 **Connect workspace**。系统浏览器会打开 Slack 授权页。确认显示的是预期 workspace 和官方 Orchestrator App，再批准权限。

也可以使用 CLI：

```bash
orchestrator source connection connect \
  --project default \
  --label "Team Slack" \
  --reason "connect Slack automation" \
  --idempotency-key "slack-connect-20260718"
```

如果不希望 CLI 自动打开浏览器，增加 `--no-open`，再手工打开输出中的 authorize URL。

### 3. 页面刷新或授权中断

页面只会在本地保存 project 和 intent ID，不会保存 OAuth state、token 或 Slack URL。刷新后会自动继续查询状态。

CLI 可以显式恢复：

```bash
orchestrator source connection status {intent_id} \
  --project default -o json
```

不再继续时取消 pending intent：

```bash
orchestrator source connection cancel {intent_id} \
  --project default \
  --reason "authorization abandoned" \
  --idempotency-key "cancel-{intent_id}"
```

### 4. 检查连接结果

成功后，Connections 列表会显示：

- `state: active`；
- `provisioning_mode: managed_shared`；
- generation、version、scope、delivery cursor/lag；
- 自动创建的 Trigger 名称。

```bash
orchestrator source connection list --project default --provider slack -o json
orchestrator source connection get {connection_id} --project default -o yaml
```

自动 Trigger 默认是 `reactionRouting: disabled`。这是有意的安全默认值：仅仅安装 App 不会创建任何任务。

### 5. 配置 Badge → Skill 任务

继续打开 **Sources → Automations**：

1. 创建 SourceTaskTemplate，选择 Skill、workflow、workspace 和 goal template；
2. 创建 SourceTaskBinding，选择刚创建的 Trigger、reaction badge、channel 和 template；
3. 先运行 preview 和 simulation；
4. 检查结果后显式启用 binding/Trigger 的 reaction routing。

完整的模板、binding、preview、恢复和回滚操作见 [用 Slack Reaction 创建 Skill 任务](slack-reaction-skill-automation.md)。

## 日常操作

### 重新授权

当 scope、token 或 Slack 安装需要刷新时，使用连接详情里的 **Reauthorize**。CLI 需要当前 version：

```bash
orchestrator source connection reauthorize {connection_id} \
  --project default \
  --expected-version {version} \
  --reason "rotate Slack authorization" \
  --idempotency-key "reauth-{connection_id}-{version}"
```

成功后仍是同一个逻辑 connection/Trigger，generation 和 version 前进，旧 pairing/generation 立即失效。

### 转移到另一台 Daemon

先确认目标 daemon 已配置同一 Gateway、拥有相同 project 资源（至少一个 Workspace 和 Workflow），并记下它稳定的 daemon ID。

```bash
orchestrator source connection transfer {connection_id} \
  --project default \
  --expected-version {version} \
  --target-daemon-id {target_daemon_id} \
  --reason "move Slack automation owner" \
  --idempotency-key "transfer-{connection_id}-{version}"
```

转移采用两阶段接力：旧 daemon 立刻失去 pairing 并显示 `suspended / owner_transfer_pending_acceptance`；Gateway 将新 pairing 加密保留给目标；目标 daemon 自动领取、创建或复用默认 Trigger、保存 cursor 后确认。短暂 suspended 是预期行为，它避免两台 daemon 同时消费。

若长时间未完成，检查目标 daemon 是否运行、project 名是否存在、是否至少有一个 Workflow/Workspace，以及 Gateway enrollment 配置是否一致。不要重复 OAuth 或手工复制数据库凭据来“修复”转移。

### 断开连接

Disconnect 会销毁 Gateway 和本地访问凭据，并停止新的 delivery/proxy，但不会删除已经创建的 source event、route、task、Attention 或 audit。

```bash
orchestrator source connection disconnect {connection_id} \
  --project default \
  --expected-version {version} \
  --reason "retire Slack integration" \
  --idempotency-key "disconnect-{connection_id}-{version}"
```

这是破坏性操作。需要恢复时重新走 OAuth，而不是恢复旧 token。

## 平台管理员：部署 Gateway 和官方 App

### 安全前提

- Gateway 是新的公网、多 workspace 信任边界，应独立部署和备份；
- 生产 `SLACK_GATEWAY_PUBLIC_URL` 必须是 HTTPS origin；
- Gateway SQLite 与 daemon SQLite 不能共用；
- `SLACK_GATEWAY_MASTER_KEY` 是 base64 编码的 32 字节密钥，放在部署 secret backend；
- `SLACK_GATEWAY_ENROLLMENT_KEY` 至少 32 字节，是平台级 bootstrap secret，只分发给受信 Orchestrator daemon；
- dev/staging/prod 使用不同 Slack App、数据库、master key、enrollment key 和域名；
- TLS 反向代理必须保留 Slack 签名验证所需的原始 request body。

生成示例密钥时，不要把输出写进 shell history、日志或仓库：

```bash
openssl rand -base64 32
openssl rand -hex 32
```

### 配置环境

```bash
export SLACK_GATEWAY_BIND="127.0.0.1:19440"
export SLACK_GATEWAY_PUBLIC_URL="https://slack-gateway.example.com"
export SLACK_GATEWAY_DATABASE="/var/lib/orchestrator-slack-gateway/gateway.db"
export SLACK_GATEWAY_MASTER_KEY="{base64_32_byte_key}"
export SLACK_GATEWAY_ENROLLMENT_KEY="{operator_bootstrap_secret_at_least_32_bytes}"
```

daemon 使用相同的公网 Gateway origin 和 enrollment secret：

```bash
export ORCHESTRATOR_SLACK_GATEWAY_URL="https://slack-gateway.example.com"
export ORCHESTRATOR_SLACK_GATEWAY_ENROLLMENT_KEY="{same_operator_bootstrap_secret}"
orchestratord --foreground --workers 2
```

没有这两个 daemon 变量时，managed mode 保持 opt-in disabled，不会增加后台联网要求。

### Provision 或 Validate 官方 App

仓库内的 [`deploy/slack/official-app-manifest.json`](../../deploy/slack/official-app-manifest.json) 不含 secret。Gateway 工具只允许替换部署环境的 callback/Events URL；scope 和 event 发生漂移会直接失败。

把 Slack 短期 Configuration Token 通过 stdin 传入，禁止放入 argv：

```bash
printf '%s' "$SLACK_CONFIGURATION_TOKEN" | \
  orchestrator-slack-gateway manifest validate \
    --manifest deploy/slack/official-app-manifest.json \
    --config-token-stdin
```

首次创建 App：

```bash
printf '%s' "$SLACK_CONFIGURATION_TOKEN" | \
  orchestrator-slack-gateway manifest provision \
    --manifest deploy/slack/official-app-manifest.json \
    --config-token-stdin
```

Provision 只输出 safe status 和 app ID；返回的 Client Secret/Signing Secret 会直接加密写入 Gateway 数据库。完成后立即撤销或丢弃短期 Configuration Token。

启动服务：

```bash
orchestrator-slack-gateway
curl -fsS https://slack-gateway.example.com/healthz
```

## 诊断速查

| 现象 | 优先检查 | 安全处理 |
|---|---|---|
| Instant 卡片不可用 | daemon 的 Gateway URL/key、Gateway health/capability | 修复配置；不要降级冒充 shared 成功 |
| OAuth 一直 pending | popup、intent expiry、callback、Gateway/Slack connectivity | status 查询或 cancel 后创建新 intent |
| `owner conflict` | 同一 workspace 已被其他 daemon/project 拥有 | 由 Admin 执行 transfer，不重复安装抢占 |
| `scope mismatch` | manifest 与 Slack 实际授权 | validate manifest，再 reauthorize |
| delivery lag 上升 | daemon 离线、pairing/generation、Gateway queue | 恢复 daemon；从 cursor 自动补投 |
| `revoked` | app uninstall / token revocation | 检查审计，确认后重新 OAuth |
| transfer pending | 目标 daemon/project/Workflow/Workspace/enrollment | 修复目标前置条件，保留 handoff 等待重试 |
| permalink 429/timeout | Slack rate limit/provider health | 等待受控重试，不绕过 proxy |

查看单调变化流：

```bash
orchestrator source connection watch --project default --after {cursor}
```

日志中只应出现稳定 ID/digest、state、generation/version、cursor 和 error code。若出现 OAuth code/state、`xoxb-` token、Signing Secret、raw Slack body、workspace 名或 message URL，应立即按安全事件处理。

## 备份、升级和止损

分别备份 Gateway 与 daemon SQLite，并分别保管密钥。备份前先执行 SQLite integrity check；恢复演练必须同时证明数据库和对应 encryption key 可用。

升级顺序建议：

1. 备份并验证 Gateway；
2. 升级 Gateway migration；
3. 检查 `/v1/capabilities`；
4. 逐台升级 daemon；
5. 先用 staging workspace smoke，再扩大 rollout。

止损时暂停新的 managed connection 和 delivery 消费，但保留 Gateway queue 与 daemon 的 source/task/audit。正常 rollback 保留 forward-only schema，不删除 migration row；只有 migration 失败或确认损坏才恢复备份。

发布前运行：

```bash
./scripts/qa/test-slack-managed-shared-oauth.sh
```

真实 Slack sandbox 认证是独立的非 CI 门禁，证据只能记录匿名 digest、commit、manifest digest、request ID、状态转换和结果，不能记录 workspace、用户、channel/message URL 或任何 credential。

