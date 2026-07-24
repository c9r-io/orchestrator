# 为每个 Slack Workspace 创建独立的 Orchestrator App

这份指南适合需要更强隔离的项目管理员：每个 Slack workspace 拥有一个独立 App、独立 Signing Secret、独立 OAuth client 和独立 Events URL，但 badge、Skill、workflow 和任务管理仍使用同一套 Orchestrator 体验。

如果你只想最快开始，仍应选择 **Instant — Official Orchestrator App**。Dedicated 模式多两次明确的人为动作：生成短期 Configuration Token，以及批准 Slack OAuth。它更隔离，但不可能在 Slack 现有规则下做到真正的零交互一键安装。

## 先选择正确的连接方式

在 **Sources → Connections** 中会同时看到：

| 模式 | 使用场景 | 代价 |
|---|---|---|
| Instant — Official Orchestrator App | 快速安装、多 workspace 共用官方 App identity | App identity 的影响面更大 |
| Dedicated — Private workspace app | workspace 自己拥有 App、希望独立撤销与更小 credential blast radius | 需要 Configuration Token、OAuth 和更多生命周期管理 |
| Existing app — Manual credentials | 已有 App，或不部署 Gateway | 需要手工维护 SecretStore/Trigger |

Dedicated 不会静默降级成 Instant。Gateway 或 daemon 不支持时，界面会显示不可用原因。

## 安装前准备

平台管理员需要先完成 FR-114 Gateway 配置。确认：

```bash
orchestrator source connection catalog -o json
```

结果中 `managed_dedicated` 应为可用。生产 Gateway 必须使用 HTTPS；daemon 只向 Gateway 和 Slack 发起出站连接。

准备一个非生产 workspace 做首次验证。不要在消息、workspace 名或 channel 中放客户数据。安装者需要有权创建 Slack App、生成 App Configuration Token 并批准 OAuth。

## GUI 安装流程

### 1. 生成一次性 Configuration Token

从 Slack 官方 App Manifest/Configuration Token 页面生成短期 token。这个 token 代表“用户对 workspace 中 App 配置的管理权限”，范围大于某个 App 自己的 token，因此：

- 不要放到命令参数、环境变量、URL、聊天或工单；
- 不要让浏览器插件或剪贴板自动化长期保存；
- 不要把 refresh token 交给 Orchestrator；
- 完成或放弃后立即在 Slack 侧撤销/丢弃。

### 2. 验证固定 Manifest

打开 **Sources → Connections**，在 Dedicated 卡片填写本地显示名称，把 token 粘贴到 **One-time Configuration Token**，然后点击 **Validate manifest**。

提交后输入框会立即清空。页面只会保存 project 和 opaque provisioning ID；不会保存 token、Slack URL 或 App credential。

系统会显示语义 diff：

- `reactions:read` scope；
- `reaction_added`、`app_uninstalled`、`tokens_revoked` events；
- OAuth callback 和 Events API 的 origin；
- App identity 与 token rotation 设置。

回调只显示 origin，不显示完整的专属 private path。任何 scope/event/callback 扩大都会明确标记 **permission expansion**。

### 3. 二次批准并创建 App

确认 diff 后点击 **Approve and create app**，填写审计原因，再点击 **Create app**。

后台顺序固定：

1. Gateway 创建 connection-scoped、短期、一次性的 credential import slot；
2. daemon 调用 Slack `apps.manifest.create`；
3. daemon 把新 App 的 `app_id/client_id/client_secret/signing_secret` 直接交给 Gateway；
4. Gateway 按 connection context 加密并返回签名 receipt；
5. daemon 验证 receipt，清除 Configuration Token 与临时 App credentials；
6. 只有这时才打开 Slack OAuth。

你不需要、也不应该手工复制 Client Secret 或 Signing Secret。

### 4. 完成 OAuth

在 Slack 页面确认预期 workspace 和刚创建的 private App，然后批准。返回 Connections 后应看到：

- `provisioning_mode: managed_dedicated`；
- `app_ownership: workspace`；
- `provision_state: completed`；
- `state: active`；
- App ID 仅显示不可逆 digest；
- 自动 Trigger 存在，且 `reactionRouting: disabled`。

最后进入 **Sources → Automations** 创建/检查 template 和 badge binding，先运行 preview/simulation，再显式启用 routing。安装 App 本身不会创建任务。

## CLI 安装流程

Configuration Token 只从 stdin 读取：

```bash
printf '%s' "$ONE_TIME_SLACK_CONFIGURATION_TOKEN" | \
  orchestrator source connection provision-dedicated \
    --project default \
    --label "Private Engineering Slack" \
    --config-token-stdin \
    --reason "isolate engineering Slack credentials" \
    --idempotency-key "dedicated-preview-20260719"
```

命令先输出 safe manifest diff，不会自动批准。检查后：

```bash
orchestrator source connection dedicated-resume {provisioning_id} \
  --project default \
  --reason "approved fixed manifest permissions" \
  --idempotency-key "dedicated-approve-{provisioning_id}"
```

也可以在首次命令中增加 `--approve` 连续执行，但它仍需要明确 reason，并且适合已经通过外部流程完成 diff 审批的自动化环境。

查询安全状态：

```bash
orchestrator source connection dedicated-status {provisioning_id} \
  --project default -o json
```

放弃：

```bash
orchestrator source connection dedicated-abandon {provisioning_id} \
  --project default \
  --reason "operator reviewed orphan recovery" \
  --idempotency-key "dedicated-abandon-{provisioning_id}"
```

不要使用 shell tracing（`set -x`），也不要把 token 管道命令复制进会长期保存的 CI 日志。

## 中断与恢复

| 状态/错误 | 含义 | 正确动作 |
|---|---|---|
| `awaiting_approval` | Slack validate 已完成，还没有创建 App | 检查 diff 后批准，或 Abandon |
| `handoff_pending` | App 已创建，Gateway receipt 尚未完成，但本 daemon 仍持有临时凭据 | 使用 **Resume secure import**；不会再创建 App |
| `oauth_pending` | Gateway 已安全持有凭据，等待 Slack consent | 重新打开 Slack consent 或等待 intent 终态 |
| `provisioning_session_expired` | 短期 token/session 已过期 | 查看 Attention，Abandon；如需继续，重新生成 token 发起新流程 |
| `provisioning_session_lost` | daemon 重启后临时凭据不可恢复 | 查看 Slack 管理页是否存在 orphan App；不要直接重跑 create |
| `slack_manifest_create_uncertain` | Slack 请求结果不确定 | 停止自动化，人工检查 orphan App，再 Abandon/清理 |

Attention Inbox 使用 provisioning ID 去重。重复刷新不会制造许多异常项。完成或 Abandon 后对应 Attention 会被解决。

## 重新授权、迁移与 Credential 替换

Dedicated 连接的 **Reauthorize** 会使用该连接自己的 client identity 和 callback，而不是 official shared App。成功后仍是同一个逻辑 SourceConnection/Trigger，generation/version 前进，旧 pairing 失效。

Shared → Dedicated 的安全路径是：创建新 App（routing disabled）→ OAuth/health → Gateway 对同一 verified team 原子更新唯一 installation → 原连接/Trigger 保持 → smoke → 恢复 routing。Dedicated → Shared 反向执行。任一时刻只允许一个 active pairing；历史 source/route/task/audit 不删除。

在 GUI 创建 Dedicated App 时，可用 **Migration source (optional)** 明确选择要替换的 active shared connection。Dedicated → Shared 则在目标连接上点击 **Migrate to Official App**，审阅影响并填写原因。CLI 对应命令为：

```bash
orchestrator source connection migrate-to-shared {connection_id} \
  --project default \
  --expected-version {version} \
  --reason "return this workspace to the official App" \
  --idempotency-key "dedicated-to-shared-{connection_id}"
```

OAuth intent 会绑定 installation ID、当前 version 和原 provisioning mode。回调过期、重复、目标已变化或未经过迁移审阅时都会失败关闭，原 active owner 保持不变。

### 升级固定 Manifest

现有 Dedicated 连接可点击 **Review manifest upgrade**。每次升级都需要一个新的 Configuration Token；系统先 export 精确 App ID，再 validate 固定目标 manifest并显示 current → target 语义 diff。CLI 使用：

```bash
printf '%s' "$FRESH_SLACK_CONFIGURATION_TOKEN" | \
  orchestrator source connection dedicated-upgrade {connection_id} \
    --project default \
    --expected-version {version} \
    --config-token-stdin \
    --reason "review dedicated App manifest v1" \
    --idempotency-key "dedicated-upgrade-{connection_id}"
```

不加 `--approve` 只输出 diff。审阅后用新的 token 重跑并增加 `--approve`。如果 Slack 或语义 diff 表明权限扩大，connection 会进入 `suspended / reauthorization_required`，Attention Inbox 出现一个去重项；完成随后打开的 OAuth 前，旧 scope 不会继续 delivery。

Slack 官方 Manifest API支持 App `export/update/delete`，但没有一个等价 API可以原地自动生成新的 Signing Secret/Client Secret。因此 Orchestrator 不会伪造 `rotate-app-credentials`：

1. 用新 Configuration Token 走新的 Dedicated provisioning；
2. OAuth 后让同一 verified team 切换到新 App；
3. 验证旧 App endpoint 已不能投递；
4. 断开/保留证据；
5. 用 Slack 官方管理界面或受控 `apps.manifest.delete` 流程退休旧 App。

这是“受审 App replacement”，不是原地 secret rotate。

## 断开与删除的区别

**Disconnect** 撤销 installation credential、停止 delivery/proxy，并保留 source、route、task、Attention 和 audit。默认不删除 workspace-owned App。

删除 App 是另一个不可逆动作：需要 fresh Configuration Token、精确 App ID 确认、独立 Admin 审计，并且应先断开连接；绝不能把 Disconnect 解释成已经删除 App。

当前版本已提供独立的 **Delete workspace App** 控件；它只在 Dedicated connection 已 `disconnected` 时显示。CLI 等价操作：

```bash
printf '%s' "$FRESH_SLACK_CONFIGURATION_TOKEN" | \
  orchestrator source connection dedicated-delete {connection_id} \
    --project default \
    --expected-version {version} \
    --config-token-stdin \
    --app-id-confirmation {exact_slack_app_id} \
    --reason "retire reviewed sandbox App" \
    --idempotency-key "dedicated-delete-{connection_id}"
```

删除成功后 Gateway 清空该 App 的 encrypted credential envelope，connection 保留为 `disconnected / app_deleted` 证据。Configuration Token 和输入的 App ID 不进入 safe projection、浏览器存储或审计正文。

## 受控 Slack Sandbox 实测 Addendum

先完成 `slack-managed-sandbox-certification-runbook.md` 的公共 Gateway、stop-loss、备份、隐私和证据前置步骤，再追加：

```bash
./scripts/qa/certify-slack-managed-live.sh run \
  --mode dedicated \
  --run-id "slack-dedicated-$(date -u +%Y%m%dT%H%M%SZ)" \
  --env-file ~/.config/orchestrator/qa/slack-live.env
```

也可以用 `--mode both` 在同一 private run inventory 中先做 shared、再做 dedicated。每个 OAuth、manifest receipt、cursor recovery、reauthorize 和 disconnect/delete 阶段都会以退出码 `20` 暂停；完成 provider 操作后用 run ID 记录 checkpoint 并 resume。`dedicated_disconnect_delete` 的 PASS 和最终 external cleanup 都要求再次提供同一 run ID 作为破坏性确认。

1. 使用一个全新的非生产 workspace 和一个全新的 Configuration Token；
2. 记录提交 SHA、`deploy/slack/dedicated-app-manifest.json` SHA-256 和测试日期；
3. 在 GUI 完成 validate → diff → approve → OAuth；刷新一次 `oauth_pending` 页面；
4. 只记录 App ID 的 digest，不记录完整 App ID、workspace/channel/user/message；
5. 配置两个 echo-only badge binding，分别创建两个确定性任务；
6. 验证错误 endpoint/signature/App/team canary 均无 delivery；
7. 重新授权，停止 daemon 后恢复 cursor，再执行 Disconnect；
8. 撤销/丢弃 Configuration Token，删除 sandbox App；
9. 扫描所有保留证据，确认没有 `xox*`、Configuration Token、client/signing secret、OAuth code/state、raw body 或 private URL；
10. 只保留 pass/fail、匿名 digest、安全状态转换和 request ID。

shared 与 dedicated 共用真实 Slack OAuth/Events/Trigger/badge 边界。组合模式保持先 shared、后 dedicated 的顺序；recorded fixture 只用于日常回归，不能替代本 addendum 的 live provider 证据。
