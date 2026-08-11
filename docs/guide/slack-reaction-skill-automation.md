# 用 Slack Reaction 创建 Skill 任务

这份指南面向第一次配置 Slack 自动化的管理员和日常值守人员。完成后，用户只需在一条 Slack 消息上添加 `:agent-implement:` 或 `:agent-docs:`，Orchestrator 就会选择对应的 Skill、workflow 和 workspace，使用该消息的稳定链接创建任务。

本指南覆盖 setup、preview、enable、inspect、diagnose、suspend、upgrade 和 rollback，不要求先阅读设计文档。

## 先理解六个对象

| 对象 | 作用 | 谁负责配置 |
|---|---|---|
| Slack installation | 一个已安装的 Slack app/workspace 身份，由 Trigger 的 `installationId` 表示 | 管理员 |
| Badge | Slack reaction emoji，例如 `agent-implement`；manifest 中不写两侧冒号 | 管理员约定，Slack 用户点击 |
| SourceTaskTemplate | 版本化任务配方：Skill、goal 模板、workflow、workspace、是否立即启动 | 管理员 |
| SourceTaskBinding | 将 installation 下的 badge、channel 和 actor role 精确绑定到一个 template | 管理员 |
| Route | 从已认证 source event 到 permalink、模板渲染和 canonical task 的持久化执行记录 | daemon |
| Task | 真正进入 Orchestrator scheduler 的工作单元 | daemon 创建，用户/agent 执行 |

一个简单心智模型是：

```text
Slack installation + badge + channel + actor
  → exactly one SourceTaskBinding
  → one immutable SourceTaskTemplate generation
  → one permalink
  → one canonical task
```

如果匹配不到、匹配到多个、actor/channel 未授权、凭据失效或模板无效，系统会 fail closed；不会猜测应该使用哪个 Skill。

## 隐私默认值

- 默认只使用并保存 Slack message permalink，不抓取消息正文、附件或 thread transcript。
- Slack 消息正文不能选择 Skill、workflow、workspace、execution profile 或模板变量。
- signing secret 和 outbound API token 只从 SecretStore 在 daemon 内解析。
- 安全的 Source/Route 列表不会返回 token、raw payload、`normalized_json`、message body 或受保护 permalink。
- Operator/Admin 显式打开 Slack 链接时，GUI 才调用受保护的 route API；ReadOnly 看不到该链接。
- task goal 会包含管理员配置的 Skill invocation 和 permalink，因此有 task 读取权限的人能够看到该链接。

## 1. 准备 Slack app

Orchestrator 不会自动创建 Slack app 或完成 OAuth。先在 Slack 管理后台创建或选择一个 app：

1. 在 **Event Subscriptions** 中启用 Events API。
2. 将 Request URL 指向可从 Slack 访问的 HTTPS 地址：

   ```text
   https://{your-public-host}/source/slack/{project}/{trigger-name}
   ```

   daemon 本地监听由 `orchestratord --webhook-bind {host}:{port}` 提供；公网 TLS、DNS 和反向代理由部署环境负责。

3. 订阅 bot 或 user event `reaction_added`。
4. 为接收 reaction event 授予 `reactions:read`。Slack Events API 说明 reaction 事件受这个 scope 控制：[Slack Events API](https://api.slack.com/events-api)。
5. 安装或重新安装 app，使新增 event/scope 生效。
6. 从 **Basic Information** 取得 Signing Secret；从 **OAuth & Permissions** 取得安装 token。

Orchestrator 使用 `chat.getPermalink` 将 channel ID 和 message timestamp 转成稳定链接。Slack 当前文档显示 bot/user token 调用该方法不要求额外 scope，但 token 必须有效，app 也必须能够访问目标 conversation；遇到 429 时必须遵守 `Retry-After`：[Slack `chat.getPermalink`](https://api.slack.com/methods/chat.getPermalink)。

Slack 请求必须用原始 body、timestamp 和 Signing Secret 验证，不能使用已弃用 verification token：[Slack request signing](https://api.slack.com/authentication/verifying-requests-from-slack)。

## 2. 安全写入 SecretStore

不要把真实 secret/token 提交到仓库，也不要把它们作为普通命令参数留在 shell history。推荐在仓库外创建 owner-only 临时文件，应用后立即删除：

```yaml
apiVersion: orchestrator.dev/v2
kind: SecretStore
metadata:
  name: slack-main-secret
spec:
  data:
    signing: "{slack_signing_secret}"
    bot-token: "{slack_bot_token}"
```

```bash
umask 077
$EDITOR /tmp/slack-main-secret.yaml
orchestrator apply --project main -f /tmp/slack-main-secret.yaml
rm -f /tmp/slack-main-secret.yaml
```

确认文件权限没有被放宽，并清除编辑器备份。`orchestrator get/describe`、日志和 GUI 不应打印 SecretStore 值。

这里有两种不同用途的凭据：

- `secret.fromRef`：验证 Slack 发给 Orchestrator 的请求签名；
- `outboundCredential.fromRef/key`：daemon 调用 `chat.getPermalink` 使用的 token。

它们可以放在同一个 SecretStore，也可以按组织策略拆分。轮换 outbound token 不会改变 signing secret。

## 3. 创建 Slack Trigger

下面的 Trigger 将 installation `T01234567` 映射到项目 `main`，只允许已配置的 actor role 参与 badge routing：

```yaml
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: slack-main
spec:
  event:
    source: webhook
    webhook:
      provider: slack
      installationId: T01234567
      timestampToleranceSecs: 300
      actorRoles:
        U_OPERATOR_1: operator
        U_OPERATOR_2: operator
        U_READER_1: read_only
      reactionRouting: disabled
      secret:
        fromRef: slack-main-secret
      outboundCredential:
        fromRef: slack-main-secret
        key: bot-token
  action:
    workflow: slack-engineering
    workspace: main
    start: true
  concurrencyPolicy: Allow
```

先保持 `reactionRouting: disabled`。这样可以在不创建任务的前提下应用资源、preview 和 simulation。

`actorRoles` 来自管理员配置。Slack payload 里的用户 ID 只能查表，不能自行声明为 Operator。

## 4. 创建两个任务模板

SourceTaskTemplate 与 workflow 的 StepTemplate 不同：前者决定“如何从 source 创建一个 task”，后者决定 workflow 内某一步给 agent 的 prompt。

实现类 badge：

```yaml
apiVersion: orchestrator.dev/v2
kind: SourceTaskTemplate
metadata:
  name: implement-from-slack
spec:
  skill:
    name: ticket-fix
    invocation: "$ticket-fix"
    args: []
  action:
    workflow: slack-engineering
    workspace: main
    start: true
  goalTemplate: |
    {skill_invocation}
    Work from this Slack message: {source_message_url}
  allowedVariables:
    - skill_invocation
    - source_message_url
```

文档类 badge：

```yaml
apiVersion: orchestrator.dev/v2
kind: SourceTaskTemplate
metadata:
  name: docs-from-slack
spec:
  skill:
    name: qa-doc-gen
    invocation: "$qa-doc-gen"
    args: []
  action:
    workflow: slack-docs
    workspace: main
    start: true
  goalTemplate: |
    {skill_invocation}
    Document the work requested by: {source_message_url}
  allowedVariables:
    - skill_invocation
    - source_message_url
```

模板只能使用 `allowedVariables` 中的变量。未知变量、空 URL、未知 workflow/workspace 或不受允许的资源引用会被拒绝。

## 5. 将 badge 绑定到模板

```yaml
apiVersion: orchestrator.dev/v2
kind: SourceTaskBinding
metadata:
  name: slack-implement-badge
spec:
  triggerRef: slack-main
  match:
    eventKind: reaction_added
    reaction: agent-implement
    targetKind: message
    channels: [C_ENGINEERING]
  templateRef: implement-from-slack
  allowedActorRoles: [operator, admin]
  suspend: false
---
apiVersion: orchestrator.dev/v2
kind: SourceTaskBinding
metadata:
  name: slack-docs-badge
spec:
  triggerRef: slack-main
  match:
    eventKind: reaction_added
    reaction: agent-docs
    targetKind: message
    channels: [C_ENGINEERING, C_DOCS]
  templateRef: docs-from-slack
  allowedActorRoles: [operator, admin]
  suspend: false
```

应用模板、binding 和尚未启用的 Trigger：

```bash
orchestrator apply --project main -f slack-automations.yaml
```

可以从零费用的完整 fixture 开始理解结构：

```bash
orchestrator apply --project demo \
  -f fixtures/manifests/bundles/slack-skill-automation-release-fixture.yaml
```

fixture 仅用于本地 QA；不要把其中的测试 secret、installation、channel 或 workspace host 用到生产环境。

## 6. 启用前 Preview 和 Simulation

先预览两个模板。Preview 只渲染，不创建 source、route 或 task，也不调用 Slack：

```bash
orchestrator source template preview implement-from-slack \
  --project main \
  --provider slack \
  --installation T01234567 \
  --message-url https://your-workspace.slack.com/archives/C_ENGINEERING/p1234567890000100 \
  --reaction agent-implement \
  --target-id C_ENGINEERING:1234567890.000100 \
  -o json
```

再模拟 binding。Simulation 使用 live routing matcher，但没有 provider network 或 mutation：

```bash
orchestrator source binding simulate \
  --project main \
  --installation T01234567 \
  --reaction agent-implement \
  --channel C_ENGINEERING \
  --actor U_OPERATOR_1 \
  -o json
```

期望看到：

- `status: matched`；
- `binding_id: slack-implement-badge`；
- `template_ref: implement-from-slack`；
- `resolved_role: operator`。

也要故意测试错误 actor、channel 和 badge，确认得到稳定的 no-match/unauthorized reason，而不是自动选择其他模板。

GUI 路径：打开 **Sources → Automations**：

- **Templates**：选择 installation 和 sample permalink，点击 **Render preview**；
- **Badge bindings**：输入 reaction/channel/actor，点击 **Simulate badge**；
- 保存、suspend/resume、replay/ignore 都会要求 review reason，并使用 revision/version 防止覆盖新配置。

## 7. Enable 并做第一条 Smoke

确认 preview 和 simulation 正确后，将 Trigger 改为：

```yaml
reactionRouting: bindings
```

重新 apply，然后在允许的 Slack channel 中对一条非敏感测试消息添加 `:agent-implement:`。

```bash
orchestrator source automation status --project main -o json
orchestrator source automation list --project main -o json
orchestrator task list --project main -o json
```

正常路径应当依次表现为：

```text
source event persisted
→ route matched/resolving/rendered/creating
→ canonical task pending/running/completed
→ route routed
```

再用 `:agent-docs:` 测试另一条消息，确认 task 使用不同的 template、Skill 和 workflow。

## 8. Inspect：从 Source 一路追到 Task

列出 route 并查看 attempt：

```bash
orchestrator source automation list --project main -o json
orchestrator source automation get {route-id} --attempt-limit 20 -o json
```

查看 source event 和受保护 Slack link：

```bash
orchestrator source get {source-event-id} -o json
orchestrator source route {source-event-id} -o json
```

第二条命令需要 Operator+，因为它可能返回 permalink。不要把输出贴到公开 ticket 或日志。

查看 task 和单任务时间线：

```bash
orchestrator task info {task-id} -o json
orchestrator task timeline {task-id} --limit 50 -o json
```

GUI 中可从 **Sources → Automations → Recent routes** 打开 route，继续跳到：

- 原始 Source event；
- 当前 binding/template；
- 关联 Attention；
- Process Workspace 和 timeline。

## 9. 去重、reaction 删除和手动重跑

- Slack retry、相同 reaction 的重复 delivery、daemon 重启和并发 routing 都使用 durable identity 收敛。
- 默认 identity 包含 project、installation、channel/message timestamp、reaction 和 binding identity。
- 同一个 message/badge/binding 不会因为不同 Slack event ID 创建第二个 task。
- `reaction_removed` 默认不会取消、删除或回滚已创建任务。
- 删除 reaction 再加回来不是重跑机制，也不能绕过审计。
- 需要重跑失败 route 时使用显式 `source automation replay`；已成功创建的任务应通过 task lifecycle 的 retry/new task 策略处理。

Reviewed replay 示例：

```bash
orchestrator source automation replay {route-id} \
  --expected-version {positive-route-version} \
  --reason "Slack credential rotated and preview verified" \
  --idempotency-key replay-20260718-001 \
  --adopt-current-config \
  -o json
```

默认 replay 使用 route 已冻结的 generation。只有在明确复核新 binding/template 后，才添加 `--adopt-current-config`。

## 10. Diagnose：常见失败

| 现象/状态 | 常见原因 | 处理 |
|---|---|---|
| `no_match` | badge、channel、target kind 不符合任何 binding | 用相同 installation/actor/channel 运行 simulation；检查 emoji 名不含冒号 |
| `unauthorized_actor` | Slack user 不在 Trigger `actorRoles`，或 role 不在 binding allowlist | 更新受治理的 actor mapping；不要相信 payload 自报角色 |
| `ambiguous` | 两个启用 binding 对同一输入重叠 | suspend 一个规则并修正 channel/reaction；系统不会任选一个 |
| `invalid_auth` / `credential_missing` | outbound token 过期、撤销、key 名错误 | suspend、轮换 SecretStore、preview/simulate、reviewed replay |
| `slack_rate_limited` | Slack 返回 HTTP 429 | 等待 `Retry-After`；daemon 会持久化 retry，不需要重新加 badge |
| `slack_request_timeout` / provider unavailable | 临时网络/provider 故障 | 查看 attempt/next retry；不要并行手工制造多个 replay |
| `stale config` / `Aborted` | 你保存或 replay 时 revision/version 已变化 | 重新加载，复核新内容，再提交 |
| duplicate delivery but no duplicate task | 正常幂等收敛 | 检查多个 source event 是否指向同一 route/task |
| `needs_attention` | 需要人修复凭据、配置或依赖 | 打开 Attention/route，按稳定 error code 修复后 reviewed replay |

健康检查：

```bash
orchestrator source automation status --project main -o json
orchestrator attention list --project main --state open -o json
```

重点观察 backlog、oldest age、active leases、retrying、needs-attention 和 failure categories。

## 11. Suspend 和止损

暂停单个 badge binding：

```bash
orchestrator source binding suspend slack-implement-badge --project main
orchestrator source binding resume slack-implement-badge --project main
```

暂停整个 Slack Trigger：

```bash
orchestrator trigger suspend slack-main --project main
orchestrator trigger resume slack-main --project main
```

最明确的 writer stop-loss 是将 Trigger 设置为：

```yaml
reactionRouting: disabled
```

然后重新 apply。它会阻止新的 badge route/provider/task 工作，但保留已有 source、route、Attention、task 和 audit 数据。活跃 lease 会完成受限状态转换，不能通过删除数据库行来“快速停止”。

## 12. Credential Rotation

推荐顺序：

1. suspend Trigger 或将 `reactionRouting` 设为 `disabled`；
2. 在 Slack 管理后台轮换 token/signing secret；
3. 用 owner-only 临时 SecretStore manifest apply 新值；
4. 对两个模板运行 preview，对两个 binding 运行 simulation；
5. resume/enable；
6. 对失败 route 使用带 reason/version/idempotency 的 replay；
7. 确认 Attention resolved，且没有第二个 task。

不要在聊天、公开日志或 shell 参数中传递新 token。Signing Secret 轮换期间，如 Slack/provider 策略允许，可在 SecretStore 中保留受控的重叠验证值，完成切换后移除旧值。

## 13. Backup、Upgrade 和 Smoke

升级前先阻止新任务并排空活动工作：

```bash
orchestrator daemon maintenance --enable
orchestrator task list -o json
orchestrator source automation status --project main -o json
```

找到数据库并做 SQLite 在线备份：

```bash
DB_PATH="$(orchestrator db status -o json | jq -r '.db_path')"
BACKUP_PATH="${DB_PATH}.pre-slack-automation.$(date +%Y%m%d%H%M%S).backup"
sqlite3 "$DB_PATH" 'PRAGMA quick_check;'
sqlite3 "$DB_PATH" ".backup '$BACKUP_PATH'"
chmod 600 "$BACKUP_PATH"
sqlite3 "$BACKUP_PATH" 'PRAGMA quick_check;'
```

两次检查都必须返回 `ok`。不要用 `cp` 复制正在写入的 SQLite 文件。

部署同一版本的 daemon、CLI 和 GUI 后，验证 migrations 33-34：

```bash
orchestrator db status -o json \
  | jq -e '.is_current == true and .current_version >= 34'
orchestrator db migrations list -o json \
  | jq -e 'all(.migrations[] | select(.version == 33 or .version == 34); .applied == true)'
```

再做 smoke：

```bash
orchestrator source automation status --project main -o json
orchestrator source automation list --project main -o json
orchestrator attention list --project main -o json
orchestrator task list --project main -o json
```

确认 GUI 的 **Sources → Automations**、Recent routes、Attention 和 Process Workspace 都能打开后，再执行：

```bash
orchestrator daemon maintenance --disable
```

发布候选必须通过：

```bash
./scripts/qa/test-slack-skill-automation-release.sh
```

## 14. Forward-only Rollback

普通二进制 rollback 不是数据库 rollback。本节是运维步骤，规则本身定义在 `crates/orchestrator-persistence/src/migration.rs`：

1. 开启 maintenance。
2. suspend Slack Trigger，并设置 `reactionRouting: disabled`。
3. 等待当前 lease 和幂等任务到达安全边界。
4. 停止 daemon。
5. 只部署已经验证能打开 schema 34 的兼容旧 daemon/CLI/GUI。
6. 保留 migrations 33-34、所有 additive tables、routes、tasks、Attention 和 audit。
7. 只做 read/smoke，确认兼容后再逐步开放 writer。

禁止：

- `DROP` source automation tables；
- 删除 `schema_migrations` 33/34；
- 伪造更低 schema version；
- 因 GUI/feature 回归而恢复旧数据库；
- 删除已经由 badge 创建的 task。

如果旧 binary 不能安全打开 additive schema，立即停止它并使用当前 binary forward-fix。只有启动 migration 失败或 `PRAGMA quick_check` 证明数据库损坏时，才使用升级前 backup 做 disaster restore。restore 会丢失 backup 之后的合法任务和 source evidence，因此不是普通 rollback 手段。

## 15. 上线检查表

- [ ] Slack Event Subscriptions 已启用 `reaction_added`，并授予 `reactions:read`。
- [ ] Request URL 使用 HTTPS 且命中正确 project/Trigger。
- [ ] Signing Secret 和 bot token 仅存在于 SecretStore/外部 secret 管理中。
- [ ] Trigger installation、actorRoles 和 channel allowlist 已复核。
- [ ] 两个模板 preview 与两个 binding simulation 都正确。
- [ ] `reactionRouting` 只在验证后从 `disabled` 改为 `bindings`。
- [ ] 在同一条合成消息上添加两个不同 badge，各自创建不同 Skill/workflow task；同一个 badge 的重复 delivery 不创建第二个 task。
- [ ] ReadOnly 看不到 mutation 或受保护 Slack link。
- [ ] Attention、route attempt、task timeline 和 provenance 可追踪。
- [ ] SQLite backup/integrity、migrations 33-34 和 stop-loss 已演练。
- [ ] clean-tree release gate 已通过。
