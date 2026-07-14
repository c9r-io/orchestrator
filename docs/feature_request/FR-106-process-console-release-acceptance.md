# FR-106: Agent Process Console Release Acceptance And Rollback Runbook

## 优先级: P1

## 状态: Proposed

## 依赖: FR-105

## 计划闭环产物

- `docs/design_doc/orchestrator/116-process-console-release-acceptance.md`
- `docs/qa/orchestrator/153-process-console-release-acceptance.md`
- `docs/guide/agent-process-console-v1-operations.md`
- `scripts/qa/test-process-console-release.sh`
- `CHANGELOG.md`（更新）

## Background

FR-095 至 FR-104 已实现 Agent Process Console 的 timeline、Attention、handoff/resume、session、source/Slack、信息架构、真实 Tauri/gRPC 垂直流与本地运营指标。然而路线图 P6-05 还要求 migration、release notes 和 rollback runbook，并以“升级保留既有 task/session 数据”为验收门。

最近的 repository audit 暴露了两个发布治理问题：

1. `scripts/qa/test-control-plane-action-audit.sh` 将最新 schema 版本硬编码为 31；FR-104 增加 migration 32 后，即使 migration 31 和 audit 表都正确存在，脚本仍失败。
2. `CHANGELOG.md` 只记录 FR-100 的 UI 迁移；各 slice 的升级、feature flag、兼容与 rollback 指令分散在 DD-105 至 DD-114，没有一个 Console v1 operator runbook 或聚合 release gate。

FR-105 另行处理 session policy authority 的真实代码缺陷。本 FR 不重复修复该控制面逻辑，而是在 FR-105 关闭后建立 Console v1 可重复、可维护的发布验收边界。

## Goals

- 修复 QA 对“拥有 migration 31”和“最新 schema 恰好是 31”的错误等价。
- 建立从干净构建产物运行全部 Console slice、真实垂直流、迁移与性能门禁的一键 release gate。
- 补齐覆盖 FR-095 至 FR-105 的 Console v1 release notes。
- 提供单一 operator runbook，说明备份、升级、验证、feature flag rollout、故障止损和 rollback 顺序。
- 用 populated upgrade fixture 证明现有 task、session、Attention、handoff、source binding、audit 与 metrics 数据在前向迁移后保持有效。

## Non-goals

- 实现新的 Process Console 产品功能或重新设计 UI。
- 发布 GitHub Release、推送 tag 或构建 FR-076 的桌面安装包。
- 引入 down migration 或自动删除 additive Console tables。
- 重复 FR-105 的 session policy 修复。
- 将所有历史 FR 的 QA 脚本合并成一个不可维护的大脚本；release gate 负责协调，不复制 slice assertions。

## Scope

### In scope

- 修正 control-plane action audit QA：验证 migration 31 已应用、schema 至少为 31，并允许后续 additive migrations。
- 增加反向测试，确保 migration 31 缺失时不能因最新版本更高而误通过。
- 新增 release orchestrator script，先构建当前 HEAD 的 daemon/CLI/GUI 所需产物，再调用 Console slice scripts。
- 聚合验证 timeline、Attention、handoff/resume、session control、source/Slack、action audit、Console UI、真实 Tauri/gRPC vertical flow 和 process metrics。
- 更新 changelog，记录完整 Console v1 用户能力、控制面/安全行为、迁移范围、兼容性和已知 non-goals。
- 新增集中式 operations guide，提供升级与 rollback 的精确命令、顺序、预期状态和数据保留规则。
- 更新 roadmap，使 Console v1 的 complete 状态只能由 FR-105 和 FR-106 的绿色证据重新建立。

### Out of scope

- 自动执行生产数据库备份或 rollback。
- 改变 migrations 27 至 32 的 forward-only 策略。
- 增加 hosted/SaaS、多租户或浏览器部署说明。
- 修改 Slack provider 语义、metric definitions 或 session fencing protocol。

## Interfaces And Data Changes

该 FR 不增加 runtime API 或 schema。它治理以下发布接口：

- `scripts/qa/test-control-plane-action-audit.sh`：按 migration identity/capability 验证，不把 latest-version equality 当作 feature presence。
- `scripts/qa/test-process-console-release.sh`：Console v1 的统一、非交互、失败即停 release gate。
- `docs/guide/agent-process-console-v1-operations.md`：operator-facing rollout/rollback contract。
- `CHANGELOG.md`：Console v1 的正式 release note source。

Release gate 必须调用既有 slice scripts，保留每个 FR 的独立诊断粒度，并汇总清晰的 pass/fail 结果。

## Key Design

1. **Capability-based migration assertion**：检查 migration 31 的 catalog/applied identity 与 `control_action_audit` schema；允许 migration 32 及未来版本共存。
2. **Fresh-binary gate**：release script 在 daemon-based QA 前构建当前 HEAD，避免旧 `target/debug` binary 产生假阳性或假阴性。
3. **Layered acceptance**：先运行静态/单元与 isolated slice fixtures，再运行真实 Tauri→gRPC vertical flow，最后运行 release-mode performance fixtures。
4. **Forward-only rollback**：停用 writers/projectors/feature flags并部署前一 binary；保留 additive tables，不执行 destructive down migration。
5. **Data-preserving upgrade**：使用 populated historical fixture，按正常 migration kernel 升至 latest，并验证关键实体数量、身份、关联和 public reads。

## Required Operations Guide

Runbook 至少包含：

- 支持的起始版本、工具依赖和磁盘空间检查。
- SQLite 一致性检查与备份命令，以及备份文件的权限要求。
- daemon drain/stop、binary upgrade、startup migration 和 `db status`/`db migrations list` 验证。
- Attention、handoff/resume、session read/control、source ingest、action audit enforcement 和 process metrics 的 feature flag rollout 顺序。
- Console UI、CLI 与 daemon 的兼容矩阵和 smoke commands。
- projector lag/failure、session control、source ingest 或 GUI 回归时的分域止损步骤。
- rollback 到前一 binary 的顺序、哪些 tables 必须保留、哪些 writers 必须先关闭，以及恢复后的检查。
- 数据库恢复仅作为迁移失败/数据损坏的最后手段，不与普通 binary rollback 混淆。

## Tradeoffs

- 一键 release gate 会比单 slice QA 更慢，但提供跨 slice 兼容和真实边界证据；开发者仍可单独运行快速脚本。
- 允许未来 migration 版本意味着不能只检查 `MAX(version)`；按 migration identity 与 schema capability 检查更稳定但断言更具体。
- 集中 runbook 会重复部分 DD 内容，但 operator 不应在十份设计文档之间拼接事故响应步骤。

## Risks And Mitigations

| Risk | Mitigation |
|---|---|
| Release script 复制 slice 逻辑并发生漂移 | 只负责构建、调用和汇总既有脚本；业务断言保留在 owning script |
| 未来 migration 再次破坏版本断言 | 按 migration identity/applied state 与表能力验证，并增加 latest > 31 fixture |
| Runbook 指令误导 operator 执行 destructive rollback | 明确 forward-only 默认策略；禁止 drop additive tables；数据库 restore 标记为最后手段 |
| 全量 gate 因端口或本机 daemon 冲突而 flaky | 所有 daemon fixture 使用独立 data dir、non-standard ports、cleanup trap 和预检查 |
| Fresh build 显著拉长本地反馈 | release gate 与 fast slice scripts分离；输出每层耗时和失败 owner |
| Release note 只描述 UI 而遗漏安全/兼容变化 | 以 FR-095 至 FR-105 checklist 驱动 changelog 和 review |

## Observability And Operations

- Release gate 输出每个 slice 的名称、耗时、状态和失败命令，不输出 prompt、transcript、source body、terminal input 或 secrets。
- 失败时保留或明确打印 isolated artifact 目录的 opt-in 开关，默认 cleanup。
- Runbook 使用现有 `db status`、`db migrations list`、audit query、Operations dashboard 和 isolated QA 作为验证面。
- Rollback 先关闭 mutation/writers/projectors，再降级 binary；additive tables 继续保留供 forward-fix 和重新升级。
- 版本发布前必须从干净 worktree、fresh binaries 运行 release gate。

## Testing And Acceptance

实现后生成 QA-153，并通过统一 release script 记录 Console v1 的最终证据。

Acceptance criteria:

- [ ] Action-audit QA 在 migrations 31、32 以及一个模拟后续 additive migration 的场景中都能正确确认 migration 31；缺失 migration 31 时失败。
- [ ] Release gate 显式构建当前 HEAD 所需 binary，不因已有 `target/debug` 文件而跳过构建。
- [ ] Release gate 调用并通过 timeline、Attention、handoff/resume、session、source/Slack、action audit、UI、vertical-flow 和 metrics 九个验收脚本。
- [ ] 真实 Tauri→gRPC flow 证明 failure→Attention→evidence→handoff→stale rejection→reviewed resume→Attention resolution。
- [ ] populated database 从 Console 前置 schema 升级到 latest 后，既有 task、session、Attention、handoff、source binding 和 audit 关联保持不变，migration 32 metrics 可重建。
- [ ] `CHANGELOG.md` 覆盖 FR-095 至 FR-105 的用户能力、breaking-change 声明、migration、feature flags、兼容性和 rollback 摘要。
- [ ] `docs/guide/agent-process-console-v1-operations.md` 覆盖备份、升级、rollout、smoke、分域止损、binary rollback 和灾难恢复边界。
- [ ] Runbook 不要求或建议普通 rollback 删除 migrations 27 至 32 的 additive tables。
- [ ] 路线图 P6-05 明确链接 release note、operations guide 和 QA-153，不再仅依赖分散 DD。
- [ ] `cargo test --workspace`、严格 Clippy、GUI unit/build/Playwright、release performance fixtures 和文档 lint 全部通过。

