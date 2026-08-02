# FR-157: Source 域分解与测试补强

## 优先级: P3

## 状态: Proposed

## 背景

2026-08-01 审计（at `9bcfaa96`,数字为子代理扫描,单一方法）发现 Source 域是架构失衡的最大单点。
**下列数字已于 2026-08-02 在 `65000dff` 重新推导,推导方法逐条注明;与原审计不符处已就地更正,更正明细见文末「事实核验记录」。**

1. **gRPC 面占比**：单一 `service OrchestratorService`（`crates/proto/orchestrator.proto` 唯一 service）共 **121** 个 `rpc`,其中 `Source*` 前缀 **37** 个（30.6%）;全文件 **250** 个顶层 `message`、**0** 个 `enum`（`grep -cE '^\s*(rpc|enum) '` / `grep -cE '^message '`,两种缩进模式计数一致）。所有枚举语义为裸字符串,如 `action_audit_mode`、`fallback_reason_code`。
2. **daemon 实现集中且欠测试**（生产行 = 首个 `^#[cfg(test)]` 之前的行数,at `65000dff`）：
   - `crates/daemon/src/server/source_connection.rs` **2572 生产行 / 75 测试行**（2647 总行）——经全工作区生产行排序确认为**最大生产文件**（次位 `crates/cli/src/commands/guide.rs` 2103）;
   - `crates/daemon/src/source_router.rs`（注意不在 `server/` 下）**1574 生产行 / 701 测试行**（2275 总行）——原审计写的"约 1574 行"是生产行而非总行,**该文件并不欠测试**,测试占全文件 31%,且已含 axum 环回桩驱动的端到端路由用例;
   - `crates/daemon/src/server/source.rs` **1433 生产行 / 0 测试行**;
   - 三者合计 5579 行,占 `crates/daemon/src` 生产行合计 **17476** 的 **31.9%**（原审计 28%）。
   - 覆盖率基线（`coverage/boundary-baseline.json`,approvedAt 2026-07-27）逐字核对无误：`daemon/source_connection` 行覆盖 **12.21%**（303/2482）、`daemon/session` 15.52%（160/1031）,对照 `core/domain` 84.29%、`daemon adapter` 28.77%（4813/16730）。
3. **全仓库尺度**：`*source*.rs` **18415 行,横跨 6 个单元**（`core`、`daemon`、`cli`、`gui`、`orchestrator-config`、`orchestrator-persistence`）。注意该 glob 字面上同时匹配 `resource*.rs`——上述数字已剔除 resource 系列;不剔除时为 14626 行（仅 `crates/`）。
4. **`"legacy_client"` 审计回退共 20 处,散布于 7 个 handler 文件**（`grep -rn` 与 `grep -rno` 两法同得 20,即无同行多次）:`server/source.rs` 5、`server/session.rs` 4、`server/attention.rs` 4、`server/handoff.rs` 3、`server/resource.rs` 2、`server/trigger.rs` 1、`server/action_audit.rs` 1。原审计的 17 处漏掉了 **`server/action_audit.rs`**,后者的 `mode == "enforced" && reason_code == "legacy_client"` 是**比较**而非赋值——19 处赋值 + 1 处比较,全部位于生产段（各文件 `#[cfg(test)]` 之后无命中）。
   `action_audit_mode` 的 `"compatibility"` 默认字面量为 **10 行 / 11 次出现**（`crd/builtin_defs.rs:150` 同行两次: schema `enum` 与 `default`）,分布于 **6 个文件跨 core 与 crates 两侧**:`crates/orchestrator-config/src/cli_types.rs`、`crates/daemon/src/server/action_audit.rs`(×3)、`core/src/crd/store.rs`、`core/src/crd/projection.rs`、`core/src/crd/builtin_defs.rs`(×2)、`core/src/resource/runtime_policy.rs`、`core/src/resource/project.rs`、`core/src/resource/test_fixtures.rs`。原审计的 7 处低估,且未预见**常量化要跨 core/daemon crate 边界**（常量必须落在 `core`,由 daemon 引用）。`"enforced"` 另有 10 次出现。

这不是"代码烂"而是"单域按需堆叠从未回头分解"——但 12% 覆盖率的 2500 行 gRPC handler 是回归风险最集中的地方,而 Slack/source 恰是对外集成面。

## 需求

### 1. 测试先行（本 FR 的主体,P3 中的 P1）
- 为 `source_connection.rs` 的核心状态机（OAuth intent 生命周期、shared↔dedicated 迁移、generation/version fence）建立单元/集成测试,目标把 `daemon/source_connection` 行覆盖提升至与 `daemon adapter` 均值（28.77%）之上,并在 boundary-baseline 中锁住不回退;
- **`source_router.rs` 移出本需求**——核验发现它有 701 测试行且已具备 axum 环回桩用例,不是欠测试点;
- 现有 `BoundaryFixture`（`server/boundary_contract_tests.rs`）向 `slack_gateway` 传 `None`,故绝大多数 handler 在"网关未配置"分支即返回,这正是 12% 的成因。测试需构造指向环回 axum 桩的 `SlackGatewayClient`（`new()` 对 `http://127.0.0.1` 有 loopback 例外,enrollment key ≥32 字节）与 `SlackClient`,复用 `source_router.rs` 已验证的桩模式;
- 覆盖率数字按技能 Phase 6 要求双路推导（cargo-llvm-cov + 基线 JSON 比对）。

### 2. 文件级分解
- `source_connection.rs` 按 OAuth/lifecycle/transfer/delivery 拆为子模块,单文件生产行数上限对齐 `server/` 其余模块（实测中位 335、非 source 最大为 `session.rs` 1064,故 <1000 是合理上限）;纯移动重构与行为变更分 commit。
- **拆分必须同步修改 `scripts/coverage/coverage-governance.mjs:121`**：`KEY_MODULES["daemon/source_connection"]` 硬编码为精确路径 `crates/daemon/src/server/source_connection.rs`,而 `matchingBucket` 用 `startsWith` 匹配——前缀以 `.rs` 结尾,拆出的 `source_connection/*.rs` **不会**被计入该 keyModule,覆盖率会因分母缩小而"提升",测量对象被静默掏空。前缀须改为不带 `.rs` 的 `crates/daemon/src/server/source_connection`（同时匹配旧单文件与新目录）。这是 §4.4 shape 2 落在测量装置自身上。

### 3. proto 面治理（设计决策,可只出 DD 不动线上）
- 评估:单 service 拆分 vs 按域前缀分组注释 + 命名约定固化;新增 RPC 的域归属规则写入 DD;
- 高频裸字符串枚举（`action_audit_mode` 等）引入 proto enum 或常量表的迁移评估——注意 wire 兼容性,结论进 DD。

### 4. 审计回退常量化
- `"legacy_client"` 与 `"compatibility"`/`"enforced"` 收敛为单一常量定义处,20 + 11 个散布点引用之（集合由 grep 派生验证归零）。常量须落在 `core`（`"compatibility"` 的多数出现点在 core 侧,daemon 依赖 core 而非反向）。

## 验收标准

- [ ] `daemon/source_connection` 行覆盖 ≥ 28.77%（双路推导:`cargo llvm-cov` 导出与基线 JSON 比对;keyModule 前缀已按需求 2 修正,分母不得低于当前 2482 的 95%——防止"拆分掏空分母"式虚假达标）
- [ ] `source_connection` 目录下**每个**生产文件行数 < 1000（由目录派生断言,非手抄清单）,拆分 commit 与行为 commit 分离
- [ ] 负向验证:在拆分后模块中引入一个状态机 bug（**注释掉**某个 fence 判断而非删除整行）,新增测试能捕获,并记录该测试的诊断输出而非仅退出码
- [ ] `"legacy_client"` / `"compatibility"` / `"enforced"` 三者的字面量在 `crates/` 与 `core/` 的生产段中各仅存 1 处定义（由 grep 派生集合断言,`grep -rno` 计出现数而非行数）;**并配一条行为断言**:一个不带 audit context 的 gRPC 调用仍产生 `fallback_reason_code == legacy_client` 的审计记录（字面量归一不得改变线上取值）
- [ ] proto 治理 DD 落地并被 `docs/design_doc/README.md` 索引

## 依赖与关联

- 依赖已满足:FR-151（0.4.0 发版）已 Closed,大规模重构不再与首个发布争抢窗口。
- 关联 DD-125/126（managed Slack）、DD-142（core boundary freeze,其 Phase B/C 仍 open——但其对象是 `core` 的 `rusqlite` 引用面,本 FR 拆分的是 `crates/daemon`,不动 core 账本;需求 4 的常量若落在 `core` 需确认不增加 core 的 `pub mod` 计数）。

## 事实核验记录（2026-08-02, at `65000dff`）

| 原始断言 | 核验结果 | 影响 |
|---|---|---|
| 121 RPC / 37 `Source*` / 250 message / 0 enum | ✅ 逐个吻合 | 无 |
| `daemon/source_connection` 12.21%、`session` 15.52%、`core/domain` 84.29%、`daemon adapter` 28.77% | ✅ 逐字吻合 | 无 |
| `source_connection.rs` 约 2574 生产行,75 测试行 | ⚠️ 2572 / 75 | 可忽略 |
| `source_router.rs` 约 1574 行 | ❌ 那是生产行;总 2275,含 **701 测试行** | **需求 1 缩小**:该文件不欠测试 |
| 三文件占 daemon 生产代码 28% | ❌ 5579/17476 = 31.9% | 佐证更强,不改动作 |
| `*source*.rs` 约 2 万行 / 6 crate | ⚠️ 18415 行 / 6 单元（glob 需剔除 `resource*`） | 无 |
| `"legacy_client"` 17 处 / 6 文件 | ❌ 20 处 / 7 文件,漏 `server/action_audit.rs` | 需求 4 集合扩大 |
| `"compatibility"` 7 处 | ❌ 10 行 / 11 次 / 6 文件,**跨 core 与 crates** | 需求 4 常量归属需定 |
| FR-151 未发版 | ❌ 已 Closed,0.4.0 已发布 | 排期约束解除 |
| （原文未提） | 🆕 `coverage-governance.mjs:121` 的 keyModule 前缀硬编码为精确 `.rs` 路径,拆目录会静默掏空测量对象 | 列入需求 2 |
