# FR-114: Managed Slack Connection 与官方 App OAuth 快速路径

## 优先级: P0

## 状态: In Progress

## 依赖

- FR-002：Daemon 控制面认证、鉴权与传输安全
- FR-080 / FR-081：Webhook 入口、签名认证与事件过滤
- FR-089：SecretStore 加密密钥紧急恢复
- FR-107 through FR-113：Slack badge → Skill task automation release

## 计划闭环产物

- `docs/design_doc/orchestrator/125-managed-slack-connection-shared-oauth.md`
- `docs/qa/orchestrator/162-managed-slack-connection-shared-oauth.md`
- `docs/guide/slack-managed-connections.md`
- `fixtures/manifests/bundles/slack-managed-shared-oauth-fixture.yaml`
- `scripts/qa/test-slack-managed-shared-oauth.sh`
- `docs/architecture.md`、相关 security 文档与 `CHANGELOG.md`（更新）

## Background And Product Decision

FR-107 through FR-113 已完成 Slack `reaction_added` → SourceTaskBinding → SourceTaskTemplate → canonical task，但当前采用 Bring Your Own Slack App：管理员需要手工创建 App、配置 Events API、复制 Signing Secret/Bot Token，并创建 SecretStore 与 Trigger。

本 FR 建立统一的 **Managed Slack Connection** 产品与协议基础，并首先交付真正的一键快速路径：

> Orchestrator 运营方维护一个官方 Slack App；不同 Slack workspace 通过标准 OAuth 将同一 App 安装到自己的 workspace。

连接模型从第一天保留三种稳定 provisioning mode，避免后续形成三套 badge runtime：

| Mode | Owner | 本 FR 状态 |
|---|---|---|
| `managed_shared` | Orchestrator 官方 App + Gateway | 本 FR 实现 |
| `managed_dedicated` | 每 workspace 独立 private App | schema/capability 保留；由 FR-115 实现 |
| `manual` | 用户已有 App + SecretStore/Trigger | 兼容现有模式并提供统一安全投影 |

官方 App 的 Events API Request URL 与 OAuth callback 必须是公网 HTTPS 地址，而 `orchestratord` 是 local-first daemon，通常位于 NAT/防火墙之后。因此本 FR 引入可选的 **Slack Integration Gateway**。Gateway 只负责 OAuth、Slack 请求认证、installation token 保管、durable delivery 和最小 provider API proxy；binding、模板渲染、Attention 与 task mutation 仍只在 daemon 执行。

## Goals

- 提供 `Sources → Connections → Slack → Connect workspace` 的一键 OAuth 体验。
- 建立 provider-aware、project-scoped 的 SourceConnection durable model、RPC、CLI 与 UI。
- 定义 `managed_shared`、`managed_dedicated`、`manual` 的稳定 mode/capability contract。
- 用一个官方 Orchestrator Slack App 服务多个 workspace，并严格隔离 installation、daemon 与 project。
- 通过版本化 App Manifest provision/validate 官方 App 的 scopes、events、callback 和 Request URL。
- 建立 Gateway ↔ local daemon 的出站认证、durable delivery、cursor、ack 和 provider proxy。
- OAuth 完成后自动创建默认 `reactionRouting: disabled` 的 Trigger 投影，并复用现有 SourceTaskBinding。
- 覆盖 connect、cancel、reauthorize、revoke、disconnect、owner transfer、离线积压与恢复。
- 保证 App 级与 installation 级凭证不会进入 daemon config、task、Source projections、GUI、metrics 或日志。

## Non-goals

- 自动创建每 workspace 独立 Slack App；由 FR-115 承载。
- Slack Marketplace 上架、计费、采购或法律条款流程。
- 首版支持 Enterprise Grid org-wide installation、GovSlack 或跨 region 数据驻留。
- 同一 Slack workspace 同时 fan-out 到多个 active project/daemon。
- 读取 Slack message body、附件、thread transcript 或搜索 workspace 内容。
- 向 Slack 回写任务进度、评论或 reaction。
- GitHub/Linear OAuth 或 badge provider。
- 替换现有手工 Slack App + SecretStore + Trigger 模式。
- 在 Gateway 中执行 SourceTaskBinding 匹配、模板渲染或 task mutation。

## User Experience

1. Admin 打开 `Sources → Connections → Slack`。
2. 默认选择 **Instant — Official Orchestrator App**，选择目标 project 后点击 **Connect workspace**。
3. Daemon 向 Gateway 创建短期、单次使用、绑定 actor/project/daemon 的 installation intent。
4. GUI 使用系统浏览器打开 Slack OAuth authorize URL。
5. Workspace 管理员批准最小 scopes；Slack callback 到 Gateway。
6. Gateway 校验 `state`、交换 code、加密保存 installation token，并建立 exclusive owner mapping。
7. Daemon 通过出站连接观察完成，创建 SourceConnection 与默认关闭 reaction routing 的 Trigger。
8. Admin 选择/创建 SourceTaskTemplate 与 SourceTaskBinding，preview、simulate 后显式 enable。

安装完成页必须同时展示另两条路径：

- **Dedicated — Private app for this workspace**：标记为 FR-115 capability；不可用时解释原因，不伪装成功。
- **Existing app — Manual credentials**：进入现有 SecretStore/Trigger 引导，并投影为 `manual` connection。

## Required Architecture

```text
Admin browser ── OAuth ──> Slack official App
     │                         │
     │ installation intent     │ callback + signed Events API
     v                         v
local orchestratord <── outbound authenticated channel ──> Slack Integration Gateway
     │                                                       ├── official App credentials
     │                                                       ├── encrypted installation tokens
     │                                                       └── durable normalized delivery queue
     v
source_events → source automation router → canonical task
```

### Authority boundaries

- **Slack**：workspace consent、OAuth code、installation identity 与 provider event 的权威来源。
- **Gateway**：官方 App credentials、installation token、raw-body signature verification、OAuth state、delivery cursor 与 bounded provider proxy 的权威来源。
- **Daemon**：SourceConnection project ownership、Trigger、binding、template、route、Attention、audit 与 task mutation 的唯一权威来源。
- **GUI/CLI**：只发起 reviewed operations 和显示 role-safe projection；不能交换 OAuth code、读取 token 或自行路由事件。

Gateway 与 daemon 使用独立数据库，不能共享 SQLite/文件系统。Daemon 必须通过出站 mTLS 或等价的双向认证应用协议连接 Gateway，local-first 部署无需开放入站端口。

## SourceConnection Contract

SourceConnection 是 durable runtime resource/read model，不把 OAuth status 或 secret 伪装成静态 manifest 内容。最小安全投影包含：

- `id`、`project_id`、`provider=slack`、display label；
- `provisioning_mode`：`managed_shared | managed_dedicated | manual`；
- `installation_id`/team ID digest、nullable enterprise ID；
- `owner_daemon_id`、generation、version、capabilities；
- `state`：`connecting | active | attention | suspended | revoked | disconnected`；
- scope names、last delivery time、lag/error code 与关联 Trigger；
- created/updated/reauthorized timestamps；
- 不包含 app secret、OAuth code/state、bot/refresh/config token、raw body 或 private URL。

Trigger webhook 增加 `connectionRef`；managed mode 下它与 `secretRef`/`outboundCredential` 互斥。现有 manual Trigger 不强制迁移，daemon 可将其合成为 `manual` connection projection。

同一 official App + Slack team identity 同一时刻只能有一个 active owner daemon/project。重复 connect 返回已有 owner 的安全摘要；transfer 必须 drain old owner、清空 lease、CAS 切换 owner、记录 request ID 后再恢复 delivery。

## Official App And OAuth Contract

- 仓库维护不含 secret 的 official App Manifest 模板和 scope/event contract test。
- release operator 使用短期 App Configuration Token provision/validate official App；token 不进入 CI artifact 或常规 daemon SecretStore。
- Client Secret 与 Signing Secret 直接写入 Gateway secret backend，禁止 stdout/log 输出。
- manifest diff 新增 scope、event、callback host 或 Request URL 时需要显式审批。
- dev/staging/prod 使用不同 App identity、endpoint、database、key 和 namespace。
- OAuth `state` 至少 128-bit entropy、短期、单次使用，并绑定 daemon、project、actor、requested scopes 与 exact redirect target。
- callback 拒绝缺失、过期、重放、redirect mismatch、owner conflict 和 scope mismatch。
- authorization code 只在 Gateway 使用 official client credentials 交换；code/token 不返回 browser 或 daemon。
- reinstall/reauthorize 原子推进 installation generation，不创建新的逻辑 connection。

## Gateway Event And Provider Contract

- Gateway 对 Slack timestamp、raw body 和 Signing Secret 完成验证后才解析请求。
- 只持久化 allowlisted normalized envelope：event ID、team/enterprise、actor、reaction、channel、message timestamp、event timestamp 与 authorization metadata；不保留 raw/message body。
- durable enqueue 后才向 Slack success ack；daemon 离线不阻塞 Slack webhook path。
- Gateway → daemon 使用 at-least-once delivery、单调 cursor、bounded batch、ack、lease 和 restart recovery。
- Daemon 继续用 `source_events.external_event_id` 与 message/badge/binding automation identity 收敛重复 delivery。
- 未知 installation、owner conflict、scope 缺失和 revoked token fail closed，不猜测 project/Trigger。
- Slack provider proxy 首版只允许 permalink resolution 与 installation health；沿用 timeout、`Retry-After`、host/redirect validation 和 privacy-safe errors。

## Interfaces

新增独立 Rust service/binary，建议为 `crates/slack-gateway/`，包含 OAuth、Events API ingress、pairing/delivery、provider proxy、installation persistence、encryption、audit、retention 和 health。

Daemon/gRPC/CLI/UI 至少提供：

- `source connection list|get|watch|connect|cancel|reauthorize|disconnect|transfer`；
- safe catalog/capabilities 和 connection health；
- OAuth intent status polling，页面刷新后可恢复；
- canonical Admin mutation envelope：reason、expected version、idempotency key、request ID；
- Sources 下的 Connections UI，不新增顶层导航；
- connect 后的 template/binding/preview/simulation/enable 下一步引导。

ReadOnly 可以查看安全状态；connect、reauthorize、disconnect、transfer 要求 Admin。Operator 继续管理已有 badge binding/route，但不能改变 credential ownership。

## Security, Reliability And Operations

- Gateway 是互联网与多租户新信任边界，闭环前必须提交 repository-grounded threat model。
- 所有公网 endpoint 强制 TLS、strict host/redirect allowlist、size/rate limit 与 abuse protection。
- pairing credential installation-scoped、可撤销、短期轮换；禁止一个 bearer token访问所有 workspace。
- tenant lookup 只能来自已验证 Slack identity，不能信任 request 中的 project ID。
- OAuth/error/Attention/metrics/log/audit 仅包含 stable digest/error code，不含 credentials、state、raw body、message URL 或私有 workspace 名称。
- Gateway outage 不影响已创建 task；恢复后从 cursor 补投保留期内事件。
- daemon 离线超过 retention 产生 gap Attention，不静默假装完整。
- revoke/disconnect 立即停止 proxy 与新 delivery；保留 daemon 中既有 source/route/task/audit evidence。
- Gateway/daemon 独立 migration、backup 和 forward-only rollback；任何一方失败不得删除另一方证据。
- 提供 connection state、OAuth failure、delivery lag、revocation、rate limit 与 proxy latency 的低基数指标。

## Compatibility And Rollout

- 现有 manual SecretStore/Trigger/badge fixtures保持通过且默认不迁移。
- Gateway/managed mode 为 opt-in；local-only 用户不新增联网要求或后台进程。
- `managed_dedicated` 在 FR-115 capability 未交付前返回明确 unsupported，不降级成 shared mode。
- rollout：Gateway dark launch → dev official App → staging workspace → internal production workspace → limited external workspace。
- stop-loss 暂停 official App delivery/connection，不删除 task/source/audit；可回退 manual mode。
- capability negotiation 明确新 daemon/旧 Gateway 与旧 daemon/新 Gateway 的兼容窗口。

## Acceptance Criteria

- [x] SourceConnection 三模式 schema/capability、safe projection 与 lifecycle persistence完成；本 FR 启用 `managed_shared`/`manual`，保留 `managed_dedicated` capability slot。
- [x] Platform operator 可从版本化 manifest provision/validate official App，secret 不出现在 stdout/log/artifact。
- [ ] Admin 可从 GUI/CLI 通过一次 Slack OAuth consent 建立 active connection，无需复制 Signing Secret/Bot Token。
- [ ] 两个 workspace 安装同一 official App 后严格路由到各自 owner daemon/project，无跨租户 list/get/watch/proxy/delivery。
- [ ] OAuth state 的过期、重放、取消、拒绝、redirect/scope mismatch 和 callback retry均 fail closed且可诊断。
- [x] 同 workspace 重复/并发 connect 收敛为一个 connection/Trigger；同一时刻恰有一个 active owner。
- [ ] daemon 离线时 Gateway durable ack event；重连从 cursor 恢复且同一 badge 只创建一个 task。
- [x] managed shared 下两个 badge 继续选择不同 Skill/template/workflow并保留 source → route → task provenance。
- [x] permalink proxy支持 timeout、429、invalid_auth、revocation 和 reviewed recovery。
- [x] reauthorize推进 credential generation；旧 generation 停止新 provider request。
- [ ] `app_uninstalled`/revocation暂停 connection、dedupe Attention并阻止新 task。
- [x] transfer失败回滚后恰有一个 owner；disconnect清除 credential并保留执行证据。
- [x] ReadOnly/Operator/Admin UI和直接 RPC 边界一致，所有 privileged mutation经过 audit/CAS/idempotency。
- [x] manual mode 与 FR-107 through FR-113 release aggregate无回归。
- [ ] Gateway/daemon populated upgrade、compat rollback、backup/restore与capability negotiation通过。
- [x] Workspace、Clippy、Gateway、GUI unit/build/Playwright、security、doc lint与OAuth aggregate全绿。
- [ ] 受控 Slack sandbox完成非 CI live certification，证据不含 workspace 私有数据或 credentials。

## Implementation Evidence

- `4ed7a22f` adds the independent Slack Integration Gateway, reviewed manifest tooling, OAuth/event/provider contracts, encrypted persistence, durable delivery, and Gateway tests.
- `f14e8e14` adds daemon migration 35, SourceConnection repository/RPC/CLI/Tauri/GUI, managed Trigger association, outbound reconciliation, and manual-mode compatibility.
- `4b506e78` makes owner transfer a durable target-side claim/ack protocol; the old daemon clears its pairing and never receives the replacement credential.
- `74086363` and `1ebb4922` add reviewed Connections transfer/reauthorize/disconnect UI, version fencing, focus-safe dialogs, Playwright coverage, and populated v34→v35 migration evidence.
- The focused FR-114 gate passes Gateway 22 tests, SourceConnection/migration 8 tests, strict managed Clippy, 4 Connections component tests, 2 Connections Playwright tests, GUI build, fixture privacy, and documentation lint.
- On 2026-07-18, `./scripts/qa/test-slack-managed-shared-oauth.sh` passed from clean commit `8cc91385`: all 12 FR-114 gates in 471 seconds, including the 16-gate FR-113 release aggregate in 418 seconds. The aggregate covered fresh Rust/Web builds, full workspace tests, strict workspace Clippy, 89 frontend tests with coverage, 21 Playwright tests, documentation lint, FR-107 through FR-113 contract/vertical tests, privacy scanning, and previous-daemon compatibility rollback/forward recovery.

Remaining closure evidence is deliberately not inferred: a controlled live Slack sandbox certification must still pass, together with the live multi-workspace/two-daemon recovery, revocation, and backup/restore observations called out by the unchecked criteria. The live record must contain anonymous digests and state/request evidence only, never credentials or workspace-private data.

## QA Plan

- 两个 fake workspace + fake Slack OAuth/API + 两个 daemon/project验证安装、隔离和badge task闭环。
- 注入 callback/event/proxy timeout、429、5xx、duplicate、out-of-order、restart、revocation与offline retention。
- 并发 connect/callback/delivery/transfer，验证 connection、Trigger、source和task幂等。
- 跨 workspace/project/daemon canary 做 list/get/watch/proxy/delivery负面测试。
- 扫描 Gateway/daemon/GUI/CLI/log artifact，禁止 fixture secret、code/state、token和private URL。
- 真实 Tauri + daemon + fake Gateway跑 OAuth → connection → badge task vertical test。
- Playwright覆盖connect/cancel/error/reauthorize/disconnect/transfer、RBAC、keyboard、focus、narrow、reduced motion和axe。
- 聚合 FR-107 through FR-113 release gate；live certification只用sandbox workspace与echo agent。

## Risks And Mitigations

| Risk | Mitigation |
|---|---|
| Shared official App/Gateway成为高价值credential target | 专用secret backend、envelope encryption、最小proxy、短期pairing、审计与threat model |
| Slack事件错投其他租户 | verified team identity → exclusive owner；跨租户canary/negative tests |
| local daemon离线导致event丢失 | Gateway persist-before-ack、cursor、retention与gap Attention |
| duplicate OAuth产生多个Trigger/token generation | single-use state、canonical identity、CAS与idempotent callback |
| Gateway侵入业务权威 | 只认证/投递/proxy；binding/render/task继续只在daemon |
| managed rollout破坏local-first | opt-in Gateway；manual mode持续支持并纳入发布门禁 |
| FR-115尚未实现却误选dedicated | capability negotiation fail closed，不自动降级shared |

## External Protocol References

- [Slack App Manifest](https://api.slack.com/reference/manifests)
- [Slack `apps.manifest.create`](https://api.slack.com/methods/apps.manifest.create)
- [Slack OAuth V2](https://api.slack.com/authentication/oauth-v2)
- [Slack `oauth.v2.access`](https://api.slack.com/methods/oauth.v2.access)
- [Slack Events API](https://api.slack.com/events-api)
- [Slack request verification](https://api.slack.com/authentication/verifying-requests-from-slack)
