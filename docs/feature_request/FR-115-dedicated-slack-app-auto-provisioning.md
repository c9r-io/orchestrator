# FR-115: 每 Workspace 独立 Slack App 自动 Provisioning

## 优先级: P0

## 状态: In Progress（初始 provisioning 与完整 aggregate 已落地；App upgrade/delete/migration lifecycle 和受控 Slack sandbox 待完成）

## 依赖

- FR-114：SourceConnection、Slack Integration Gateway、pairing/delivery/provider proxy 与 Connections UI
- FR-089：SecretStore/key emergency recovery patterns
- FR-107 through FR-113：Slack badge automation runtime

## 计划闭环产物

- `docs/design_doc/orchestrator/126-dedicated-slack-app-auto-provisioning.md`
- `docs/qa/orchestrator/163-dedicated-slack-app-auto-provisioning.md`
- `docs/guide/slack-dedicated-app-provisioning.md`
- `fixtures/manifests/bundles/slack-managed-dedicated-app-fixture.yaml`
- `scripts/qa/test-slack-dedicated-app-provisioning.sh`
- FR-114 user guide、`docs/architecture.md`、security 文档与 `CHANGELOG.md`（更新）

## Background And Product Decision

FR-114 的 `managed_shared` 模式提供最理想的一键 OAuth，但所有 workspace 安装共享一个 official App identity。部分企业需要更强的隔离、tenant-owned App、独立 scope/event policy、独立 revocation 与更小 credential blast radius。

本 FR 实现 FR-114 预留的 `managed_dedicated` capability：

> 每个 Slack workspace 自动创建一个独立 private Slack App，但仍使用同一 SourceConnection、Gateway delivery、SourceTaskBinding、SourceTaskTemplate 和 task runtime。

Slack 标准 OAuth 只能安装已有 App；自动创建 App 必须使用 App Manifest API，而该 API要求 App Configuration Token。Configuration Token 绑定 user + workspace、不是 app-scoped，因此不能作为普通长期 OAuth credential托管。本 FR 采用“短期 token 本地使用、立即丢弃”的安全设计。

`managed_shared` 始终保留为默认快速路径。Dedicated 不是取代 shared，而是 Connections wizard 中面向隔离/扩展需求的高级选项。

## Goals

- 在 Connections wizard 中提供 **Dedicated — Private app for this workspace**。
- 用版本化 manifest和短期Configuration Token自动创建、配置并安装独立App。
- 自动完成App credential导入Gateway、OAuth安装、SourceConnection/Trigger创建和badge setup引导。
- Configuration Token/refresh token不离开local daemon、不持久化、不进入Gateway。
- 每个connection使用独立App ID、Signing Secret、Client Secret、installation token、event URL与encryption context。
- 提供manifest diff、显式scope审批、reauthorize、upgrade、credential regeneration、disconnect和ownership治理。
- 复用FR-114的connection/delivery/proxy协议，确保shared/dedicated对badge runtime无差异。
- 支持managed shared ↔ dedicated的reviewed migration，且不重复创建task或丢失历史evidence。

## Non-goals

- 在没有Slack管理员参与或Configuration Token的情况下绕过Slack consent创建App。
- 长期保存Configuration Token或`tooling.tokens.rotate` refresh token。
- 管理用户在该workspace中的其他Slack App。
- 默认自动删除workspace-owned Slack App；删除必须是单独、显式、可审查的操作。
- 为每个project创建App；MVP仍是一Slack workspace一active App/owner connection。
- Enterprise Grid org-wide App、GovSlack、Marketplace与跨region secret replication。
- 自定义任意Slack scope/event/function；只允许版本化Orchestrator manifest profile。
- 分叉SourceTaskBinding/Template/router/task实现。
- 让Gateway生成App Manifest或决定权限；manifest权威在版本化daemon/repository profile。

## User Experience

1. Admin 选择 `Sources → Connections → Slack → Dedicated private app`。
2. UI解释隔离收益、额外步骤、所需Slack权限，并生成一个pending connection ID。
3. 点击 **Generate one-time configuration token** 打开Slack官方token页面。
4. Admin将短期token输入password field或CLI stdin；禁止command argument、URL、clipboard auto-read和localStorage。
5. Daemon先调用manifest validate并展示scope/event/callback diff；Admin再次确认。
6. Daemon调用`apps.manifest.create`，收到App ID、专属credentials和OAuth authorize URL。
7. Daemon通过FR-114 pairing channel把新App专属credential bundle导入Gateway；Gateway返回durable receipt。
8. Daemon zeroize Configuration Token和临时App credentials，打开Slack OAuth authorize URL。
9. OAuth callback完成后connection变为active，自动创建默认`reactionRouting: disabled` Trigger。
10. UI引导template、badge binding、preview、simulation和enable。

整个过程是一个连续wizard，但必须诚实标注：Slack要求用户生成短期Configuration Token和批准OAuth，因此Dedicated模式不是零交互的一次点击；真正一键仍是`managed_shared`。

## Provisioning Contract

### Bootstrap intent

- pending connection预先生成opaque `connection_id`、project/daemon/actor绑定、expiry与idempotency key。
- 每个dedicated App使用唯一Request URL，例如`/slack/connections/{connection_id}/events`，使Gateway在解析未认证payload前确定Signing Secret。
- OAuth callback使用同一connection intent/state，且exact redirect/callback host来自allowlisted Gateway config。

### Configuration Token handling

- CLI只接受`--config-token-stdin`或受控interactive prompt；禁止argv/env/file默认路径。
- GUI field使用password semantics，submit后立即清空，不写DOM snapshot、localStorage、telemetry或crash report。
- Token只在local daemon的zeroizing memory中存在；不写SecretStore、SQLite、log、audit payload、Gateway或task。
- Daemon可用token调用`apps.manifest.validate/create`；不得调用export/update/delete其他App。
- create success、failure、cancel、timeout或daemon crash都必须best-effort zeroize。
- 不保存Configuration Token refresh token；后续App update要求Admin重新提供短期token。

### Manifest and approval

- Manifest profile固定App display、OAuth scopes、`reaction_added`、Request URL、redirect URL、token rotation policy和metadata；用户不能注入任意manifest字段。
- validate后显示语义diff：新增/移除scope、event、callback host、token rotation和App identity。
- scope/event/callback扩大必须二次Admin确认并写canonical action audit；secret/token不进audit。
- `apps.manifest.create`返回的credentials视为最高敏感数据，以typed secret/zeroizing buffer处理。

### Credential handoff

- Daemon使用connection-scoped加密pairing channel将`app_id/client_id/client_secret/signing_secret`发送Gateway。
- Gateway使用per-connection envelope key加密保存，返回包含app ID digest/generation的signed receipt。
- receipt durable前不得清除本地临时credential或启动OAuth；receipt成功后立即zeroize。
- partial create后handoff失败进入`provisioning_attention`，禁止重复create；Admin可以resume import或显式abandon，不得猜测重建。
- Gateway lookup按unique event path/connection ID定位credential set，再验证Slack raw-body signature。

## OAuth, Delivery And Badge Runtime

- Dedicated App OAuth沿用FR-114 state/callback/scope/owner contract，但使用该connection自己的client credentials。
- Gateway per-connection token custody、event queue、cursor/ack、provider proxy与revocation状态完全复用FR-114协议。
- Daemon看到的normalized SourceConnection/Slack event与`managed_shared`一致；provisioning mode不进入template变量或task identity。
- 现有SourceTaskBinding继续引用connection关联Trigger；badge match、permalink、route generation、Attention和canonical task代码不得按mode分叉。
- 同workspace已存在active shared/manual/dedicated connection时，新建dedicated必须先选择cancel或reviewed migrate；不能并行启用两个owner。

## App Upgrade And Lifecycle

- App manifest version记录在connection与Gateway credential metadata中；不包含manifest secret。
- Upgrade需要新的短期Configuration Token，先export/validate目标App identity，再展示current→target diff。
- `apps.manifest.update`只能作用于connection记录的exact App ID；team/user/app mismatch fail closed。
- Scope扩大后必须OAuth reauthorize；connection在reauthorize前保持suspended/attention，不能以旧scope继续假装健康。
- Signing Secret/client secret regeneration使用new credential generation + atomic Gateway swap；旧generation停止新verification/proxy但保留bounded grace用于in-flight验证。
- Disconnect默认撤销installation credential、停止delivery并保留App和daemon evidence。
- Delete App需要fresh Configuration Token、typed App ID confirmation和独立Admin audit；不是disconnect默认行为。
- 创建App的Slack用户离职/失权进入ownership Attention，并提供添加collaborator/transfer/runbook，不自动接管其他App。

## Interfaces

在FR-114接口之上新增：

- `source connection provision-dedicated --project ... --config-token-stdin`；
- manifest preview/validate/diff、provision resume/abandon、upgrade、rotate-app-credentials、delete-app；
- Gateway credential import receipt、per-connection app health/capability；
- GUI dedicated wizard：token entry、diff approval、OAuth waiting、partial recovery、upgrade和delete confirmation；
- connection projection字段：`app_ownership=workspace`、app ID digest、manifest version、provision state/error、credential generation；
- `managed_dedicated` capability negotiation；旧Gateway或旧daemon不支持时fail closed。

所有provision/upgrade/rotate/delete mutation要求Admin、reason、expected version、idempotency key和request ID。安全read projection不显示token、secret、完整private URL或Slack user identity。

## Security And Reliability Requirements

- FR-114 threat model增加Configuration Token、manifest injection、credential handoff、partial App creation、cross-App signature confusion和App ownership loss abuse paths。
- dedicated event endpoint使用unguessable connection ID且仍必须Slack signature验证；opaque URL不是认证替代品。
- Gateway secret storage使用per-connection envelope context，list/get/export API永不返回secret plaintext。
- 未认证payload中的`api_app_id/team_id`不能独立选择tenant/secret；credential lookup来自verified endpoint mapping并在签名后交叉核对App/team identity。
- provisioning/upgrade执行bounded timeout、Retry-After、idempotent resume与crash checkpoint，禁止blind create retry。
- orphan App detection基于durable pending connection/app receipt，不通过扫描用户其他App实现。
- dedicated App compromise只暂停对应connection；不能影响shared official App或其他dedicated connections。
- Gateway compromise仍是集中风险，因此per-connection key、least-privilege KMS policy、audit和rotation必须可验证。
- retained diagnostics禁止Configuration Token、refresh token、app credentials、OAuth code/state、bot token、raw body和private URL。

## Compatibility And Migration

- FR-114 `managed_shared`与现有`manual`行为不变；Dedicated为显式opt-in。
- shared → dedicated migration：provision new App（routing disabled）→ OAuth/health → freeze shared delivery → switch owner/Trigger connectionRef → smoke → resume dedicated。
- dedicated → shared rollback采用反向流程；不删除source/route/task/audit，不改变历史template/binding hash。
- 任一切换点失败恢复到恰有一个active owner；不允许同时接收同一workspace事件。
- App manifest/schema升级forward-only；普通binary rollback停止new provisioning但保留已连接App delivery。
- `enterprise_is_restricted`、not-in-team或policy denial返回stable unsupported/policy error，不降级为共享App而绕过用户选择。

## Acceptance Criteria

- [x] Connections wizard同时展示Instant shared、Dedicated private和Existing manual，默认shared且清楚说明trade-off。
- [x] Admin提供短期Configuration Token后，系统自动validate/create独立App、导入Gateway并启动OAuth，无需复制App credentials。
- [ ] Configuration Token/refresh token只存在local daemon memory，create完成/失败/cancel/crash后不出现在任何持久化、日志、GUI storage或artifact。
- [ ] 两个workspace分别创建不同App ID/Signing Secret/client credentials/event URL，并严格路由到各自connection。
- [x] manifest preview准确显示scope/event/callback/token rotation diff；扩大权限必须二次Admin审批。
- [ ] create callback retry、daemon/Gateway restart和API timeout不会创建第二个App；partial create可resume或abandon。
- [x] credential handoff必须durable receipt后才启动OAuth；跨connection receipt/secret/import全部拒绝。
- [x] Dedicated OAuth完成后自动创建disabled Trigger，并可用现有preview/simulation安全enable。
- [ ] 两个badge在dedicated mode选择不同Skill/template/workflow，runtime路径与shared mode一致且无mode分叉task语义。
- [x] event endpoint先定位connection再验证对应Signing Secret，并交叉检查verified app/team identity；cross-App signature confusion测试通过。
- [ ] App update需要新Configuration Token、exact App ID匹配和diff审批；scope扩大强制reauthorize。
- [ ] disconnect不删除App/evidence；delete App需要fresh token、typed confirmation与独立audit。
- [ ] shared↔dedicated migration任意失败点均恢复到恰有一个active owner且无duplicate task。
- [ ] 一个dedicated App compromise/revoke只暂停自身connection并产生dedupe Attention。
- [x] ReadOnly/Operator/Admin UI与直接RPC权限一致；secret字段不进入safe projection。
- [x] FR-114 shared OAuth aggregate、manual mode及FR-107 through FR-113 release gate全部无回归。
- [ ] Gateway/daemon populated upgrade、compatible rollback与provision checkpoint recovery通过。
- [x] Workspace、Clippy、Gateway/provisioner、GUI unit/build/Playwright、security、doc lint与dedicated aggregate全绿。
- [ ] 受控Slack sandbox创建一个真实dedicated App并完成badge task certification；完成后按runbook撤销token/清理App，证据无secret/private data。

## QA Plan

- fake Slack Manifest/OAuth/Events API覆盖validate/create/update/delete、credentials response、OAuth和rate-limit contract。
- 两个workspace/config token/app credential canary验证隔离；任何cross-App lookup/import/delivery即失败。
- crash injection覆盖create-before-receipt、receipt-before-zeroize、OAuth-before-Trigger、upgrade-before-reauthorize。
- retained process memory以外扫描DB/config/log/audit/GUI DOM/storage/artifact，禁止全部bootstrap/App/installation secrets。
- 并发create/callback/restart验证一个pending connection对应一个App；blind retry测试必须失败。
- 真实Tauri + daemon + fake Gateway/Slack跑token stdin/secure form → manifest diff → App create → OAuth → badge task。
- Playwright覆盖三模式选择、secret field清理、diff审批、partial recovery、upgrade/delete、RBAC、focus、keyboard、narrow和axe。
- 聚合FR-114与FR-107 through FR-113；live sandbox certification使用echo agent和无敏感内容message。

## Risks And Mitigations

| Risk | Mitigation |
|---|---|
| Configuration Token可管理用户workspace中的多个App | 只在local zeroizing memory使用；不进Gateway/存储；不保存refresh token；allowlist Manifest API操作 |
| create成功但credential handoff失败形成orphan App | durable checkpoint、receipt、resume/abandon，不blind retry；明确orphan recovery runbook |
| N个App导致manifest/version漂移 | versioned profile、health projection、diff-driven upgrade与reauthorize |
| Gateway按未认证app ID选错Signing Secret | unique connection endpoint定位候选secret，签名后交叉验证app/team identity |
| Dedicated和shared同时接收导致duplicate paid work | exclusive owner invariant、drain-and-switch migration、现有task idempotency双保险 |
| 创建App的Slack用户离职导致无法升级 | ownership Attention、collaborator/transfer runbook、禁止长期借用其config refresh token |
| 用户把Dedicated误解为真正零点击 | UI明确额外token+consent步骤；shared保持默认Instant path |
| 每tenant secret增加Gateway运营复杂度 | per-connection envelope key、统一connection protocol、自动health/rotation evidence |

## External Protocol References

- [Slack App Manifest and configuration tokens](https://api.slack.com/reference/manifests)
- [Slack `apps.manifest.create`](https://api.slack.com/methods/apps.manifest.create)
- [Slack token types](https://api.slack.com/concepts/token-types)
- [Slack OAuth V2](https://api.slack.com/authentication/oauth-v2)
- [Slack token rotation](https://api.slack.com/authentication/rotation)
