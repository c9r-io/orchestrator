# FR-157: Source 域分解与测试补强

## 优先级: P3

## 状态: Proposed

## 背景

2026-08-01 审计（at `9bcfaa96`,数字为子代理扫描,单一方法,实施时重新推导）发现 Source 域是架构失衡的最大单点：

1. **gRPC 面占比**：单一 `OrchestratorService` 共 121 个 RPC,其中 `Source*` 前缀 37 个（31%）;整个 proto 250 个 message、**0 个 enum**（所有枚举语义为裸字符串,如 `action_audit_mode`、`fallback_reason_code`）。
2. **daemon 实现集中且欠测试**：`crates/daemon/src/server/source_connection.rs` 约 2574 生产行（工作区最大生产文件,仅 75 测试行）、`source_router.rs` 约 1574 行、`server/source.rs` 约 1433 行（零测试）——三者合计约占 daemon 生产代码 28%。覆盖率基线（`coverage/boundary-baseline.json`,approvedAt 2026-07-27）:`daemon/source_connection` 行覆盖 **12.21%**、`daemon/session` 15.52%,对照 `core/domain` 84.29%。
3. **全仓库尺度**：`*source*.rs` 文件合计约 2 万行,横跨 6 个 crate。
4. **17 处 `"legacy_client"` 审计回退**散布于 session/source/attention/handoff/resource/trigger 各 handler;`action_audit_mode` 的 `"compatibility"` 默认字符串字面量重复出现 7 处,无单一常量。

这不是"代码烂"而是"单域按需堆叠从未回头分解"——但 12% 覆盖率的 2500 行 gRPC handler 是回归风险最集中的地方,而 Slack/source 恰是对外集成面。

## 需求

### 1. 测试先行（本 FR 的主体,P3 中的 P1）
- 为 `source_connection.rs` 的核心状态机（OAuth intent 生命周期、shared↔dedicated 迁移、generation/version fence）与 `source_router.rs` 的路由决策建立单元/集成测试,目标把 `daemon/source_connection` 行覆盖提升至与 `daemon adapter` 均值（28.77%）之上,并在 boundary-baseline 中锁住不回退;
- 覆盖率数字按技能 Phase 6 要求双路推导（cargo-llvm-cov + 基线 JSON 比对）。

### 2. 文件级分解
- `source_connection.rs` 按 OAuth/lifecycle/transfer/delivery 拆为子模块,单文件生产行数上限对齐 `server/` 其余模块（现状中位数量级 <1000）;纯移动重构与行为变更分 commit。

### 3. proto 面治理（设计决策,可只出 DD 不动线上）
- 评估:单 service 拆分 vs 按域前缀分组注释 + 命名约定固化;新增 RPC 的域归属规则写入 DD;
- 高频裸字符串枚举（`action_audit_mode` 等）引入 proto enum 或常量表的迁移评估——注意 wire 兼容性,结论进 DD。

### 4. 审计回退常量化
- `"legacy_client"` 与 `"compatibility"`/`"enforced"` 收敛为单一常量定义处,17+7 个散布点引用之（集合由 grep 派生验证归零）。

## 验收标准

- [ ] `daemon/source_connection` 行覆盖 ≥ 28.77%（双路推导,基线 JSON 更新走审批流程）
- [ ] `source_connection.rs` 单文件生产行数 < 1000,拆分 commit 与行为 commit 分离
- [ ] 负向验证:在拆分后模块中引入一个状态机 bug,新增测试能捕获
- [ ] `grep -rn '"legacy_client"' crates/daemon/src | wc -l` 仅常量定义处 1 次
- [ ] proto 治理 DD 落地并被 `docs/design_doc/README.md` 索引

## 依赖与关联

- 建议排在 FR-151 发版后:大规模纯重构不应挤进积压 4 个月的首个发布。
- 关联 DD-125/126（managed Slack）、DD-142（core boundary freeze,其 Phase B/C 仍 open,拆分时避免与其冲突,先读该 DD 的剩余项）。
