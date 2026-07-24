# FR-123: 受控 Slack Sandbox 持续认证与证据保鲜

## 优先级: P1

## 状态: Proposed

## 背景

FR-114 与 FR-115 已分别闭环 shared official App OAuth 和 per-workspace dedicated App provisioning，并提供 live certification 脚本与 runbook。但真实 Slack/OAuth 行为无法由单元测试替代，认证信息、Slack API 演进、App manifest/scopes 和 Cloudflare callback 配置会随时间漂移。

当前缺口不是重新实现 OAuth，而是把已有一次性/人工闭环提升为有到期语义、可重复执行、可安全清理的持续认证体系。

## 目标

- 对 shared 与 dedicated 两种模式建立统一的 opt-in live certification 入口。
- 安全复用本地 `.env`/SecretStore 中的认证材料，不进入 git、日志或测试 artifact。
- 每次执行产生脱敏、可过期、可比较的认证证据。
- 测试创建的 workspace、App、连接、Trigger、任务和消息具备确定性 inventory 与清理流程。

## 非目标

- 不把真实 Slack 凭据放入普通 PR CI。
- 不要求测试自动绕过验证码、人工 OAuth 同意或平台风控。
- 不以 recorded fixture 替代最终 live certification。

## 需求

### 1. 统一认证入口

- 提供 shared、dedicated、两者组合的显式模式。
- preflight 检查 Slack/Cloudflare 回调、必要工具、环境变量、scope 与测试 inventory。
- 支持“暂停等待用户完成认证后继续”，避免重跑已完成阶段。
- 每个阶段具有稳定 checkpoint 和幂等恢复语义。

### 2. Secret 与环境治理

- 提供已提交的 `.env.example`，只列变量名、用途和获取方式。
- 实际 `.env`、token、Configuration Token、client secret 和 signing secret 永不进入 git。
- 子进程只获得阶段所需的最小环境变量。
- 日志与 artifact 经过 allowlist 投影和 secret 扫描。

### 3. Live 场景矩阵

- shared 模式：OAuth 安装、双 workspace/daemon、reaction delivery、断线恢复、撤销。
- dedicated 模式：App 创建、manifest apply、OAuth、credential import receipt、权限升级/重授权、App 删除。
- 至少验证同一消息的两个 badge 映射到两个不同 Skill/task template。
- 验证 cursor 恢复、duplicate delivery 幂等和断开后停止路由。

### 4. 证据与保鲜

- 生成不含凭据的 run metadata：时间、模式、App/Workspace 哈希身份、版本、场景结果、cleanup result。
- 定义证据有效期；过期只表示“需要重认证”，不自动宣称功能回退。
- README/发布检查能展示最近一次 shared/dedicated 认证状态。
- recorded fixtures 用于日常回归，live 结果单独标识。

### 5. 清理与失败恢复

- 执行前记录所有将创建/复用的外部对象。
- 正常完成和中途失败均输出 cleanup inventory。
- 删除 Slack App、workspace 或外部域名等高影响动作必须由用户明确确认。
- 未清理对象进入可重跑 cleanup 模式，不静默遗留。

## 验收标准

- [ ] 单一入口支持 shared、dedicated 和组合认证，并能从认证 checkpoint 继续
- [ ] `.env.example` 完整，真实 secret 不出现在 git diff、stdout/stderr 或 artifact
- [ ] shared 与 dedicated 的核心 live 矩阵均可重复执行
- [ ] 同消息双 badge、duplicate delivery、cursor 恢复和 disconnect fail-closed 均有证据
- [ ] 每次运行生成脱敏结果、有效期和 cleanup inventory
- [ ] 中途失败后可单独重跑 cleanup，破坏性清理需要明确确认
- [ ] 普通 CI 使用 recorded fixtures，live suite 保持 opt-in 且不会因缺少 secret 失败

## QA 计划

- Shell 单元测试：preflight、checkpoint、redaction、缺失 secret、cleanup inventory。
- Recorded fixture：OAuth callback、Events API delivery、manifest diff 和 gateway receipt。
- 受控 live：复用 shared/dedicated sandbox runbook，在干净 daemon/data dir 中执行。
- 泄漏检查：对日志、临时目录、JUnit/JSON artifact 和 git diff 扫描已知 secret。

## 风险与缓解

- **Slack 风控/验证码导致自动化中断**：显式人工 checkpoint，可恢复而非绕过。
- **外部对象误删**：inventory + 所有权标签 + 破坏性确认。
- **凭据泄漏**：最小环境、结构化脱敏输出和运行后扫描。
- **API 漂移造成假失败**：区分 provider drift、环境错误和产品回归。

## 依赖与参考

- `docs/design_doc/orchestrator/125-managed-slack-connection-shared-oauth.md`
- `docs/design_doc/orchestrator/126-dedicated-slack-app-auto-provisioning.md`
- `docs/guide/slack-managed-sandbox-certification-runbook.md`
- `scripts/qa/certify-slack-managed-live.sh`
- `scripts/qa/test-slack-dedicated-app-provisioning.sh`

