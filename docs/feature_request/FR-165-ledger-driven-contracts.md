# FR-165: 账本与契约的驱动化 —— 从"有记录"到"有排程"

## 优先级: P2

## 状态: Proposed

## 背景

计数 at `6678144d`，方法注明。产品分析的核心诊断之一：本仓库的债务全部有
记录（36 个 DD 携带 Known limits、~150 条停放项、多本 ledger JSON），但除
CI 预算外，没有任何账本**驱动**行为。四个实测的失驱动面：

1. **manual 门禁层无节律**（`config/governance/manual-gate-freshness.json`，
   staleAfterDays: 90）：35 个 manual 门禁中 **12 个 `lastRun: null` 从未运行**
   （jq 派生清单在案：certify-codex-session-resume、fr001-sandbox-matrix、
   health-policy-check、process-console-metrics/release/ui、
   self-bootstrap-cycle2、slack-dedicated-app-provisioning、
   slack-managed-live-smoke、slack-managed-shared-oauth、
   slack-skill-automation-release、watchdog）；其余 23 个的记录全部来自
   同一天同一修订（2026-08-10 @ 2ae4e854——FR-160 的清扫，非习惯），其中
   **7 个 `worktreeDirty: true`**。40% 门禁面依赖一个从未形成的习惯；
   FR-160 系列 6 张 ticket 中 5 张是此层的沉积腐化。
2. **回滚契约 0 行代码**：`rg 'forward-only' migration.rs` → 0；契约以散文
   复述于 **8 份文档**（DD-116:73 最强形式"binary rollback keeps migrations
   27-32"，另 DD-111/121/122/124、guide ×2、威胁模型 T12）。vertical 门禁的
   钉过期事故（ticket 已闭环）即无人看守此窗口的直接后果。
3. **覆盖率棘轮单边**：`coverage/boundary-baseline.json` 自己的 reapproval
   注释承认 CLI 实际 52.86% 而基线 35.49% 继续通过（"keep passing while
   under-ratcheted"）；Rust 侧无分支信号；基线仅 macos-aarch64。
4. **audit.toml 无 unmatched 棘轮**：19 条忽略的退役条件写成给人看的
   `cargo tree -i` 单行（`.cargo/audit.toml`），无门禁；RUSTSEC-2024-0429
   "Nothing reminds anyone to re-check"（deny.toml 自注）。

## 需求

### 1. manual 门禁的分诊与节律

12 个 never-run 逐一三分类：可无头运行的降级为 ci-required 或 scheduled
（进 CI 预算核算，DD-172 规则适用）；确需人工的给 owner + 运行节律；
已无价值的按退役程序处理。staleness 从 advisory 升级为至少一个强制点
（建议：release 工作流的前置检查——过期即阻断发布，而非阻断日常推送，
避免重蹈"新鲜度门禁天天红"的反噬）。`worktreeDirty: true` 的记录裁定
是否作废重跑。

### 2. 回滚契约代码化

契约("迁移前向-only；上一 release 二进制必须能服务当前 schema；restore
仅限灾难")写入 `crates/orchestrator-persistence/src/migration.rs` 一处
文档注释 + 一道守护（候选：迁移注册表的结构断言 / 现有 populated-upgrade
测试的契约化命名与强制），8 处散文改为指向单源。若判定不加 CI 门禁，
理由按 DD-172 成文（vertical 门禁的钉推进规则已覆盖行为半边，QA 161）。

### 3. 覆盖率棘轮双边化

改进即再基线（自动或按 reapproval 流程强制），消除 under-ratcheted 区间；
Tauri 9.42% 层给出提升计划或具名豁免（它是 5.45% 的 tauri/commands 所在，
产品分析中最低的可测面）。

### 4. audit.toml 的 unmatched 棘轮

对齐 deny.toml 的 `--deny unmatched-skip` 纪律：忽略项的 crate 离树即失效
（实现方式 step 0 调研——cargo-audit 原生不支持则由治理脚本比对 ignore
列表与 Cargo.lock）。

## 验收标准

- [ ] 12 个 never-run 各有分诊记录；降级项进 CI 且预算台账更新；staleness
      强制点存在且有负夹具（人为过期 → 强制点红）
- [ ] 回滚契约单源 + 守护在位；8 处散文指向单源（rg 派生清单差集为空）
- [ ] 基线更新至实测值且双边规则成文；下一次改进被自动收顶的行为断言
- [ ] audit.toml 棘轮存在负夹具（虚构一条已离树 crate 的 ignore → 红）
- [ ] 治理预算仍 ≤2700s，变动量记录

## 依赖与关联

- 承接 FR-158（新鲜度台账的建立者——本 FR 是它预留的"攒出历史再选阈值"
  的下一步）、DD-172（预算与 shape 规则约束需求 1 的一切降级）、
  QA 161（钉推进规则）。

## 未核验项（明确标注）

- 12 个 never-run 中多少可无头化未逐一验证（Slack 族大概率需真实凭据或
  录制夹具——分诊时逐个实跑判定）。
- cargo-audit 对 unmatched-ignore 的原生支持程度未调研。
- release 工作流当前是否有可挂前置检查的结构未核验。
