# FR-165: 账本与契约的驱动化 —— 从"有记录"到"有排程"

## 优先级: P2

## 状态: Proposed

## 背景

> **Step 0 核验记录（2026-08-11 @ `3fbda023`）**：本节原始计数 pin 在
> `6678144d`。13 项事实中 7 项成立、6 项被修正，另有 3 处 FR 未察觉的缺口
> 已补入需求。修正逐条标注 `[原:…]`，方法与二次派生随号写明。核心结论未变
> ——四个面确实失驱动——但需求 1 的工作量比 FR 设想的小（强制机制已存在，
> 只是没接线），需求 2 的散文面比 FR 设想的大一倍，且 `forward-only` 在本
> 树是**三义重载**词，直接 grep 会把无关构造计入。

产品分析的核心诊断之一：本仓库的债务全部有记录（36 个 DD 携带 Known
limits、~150 条停放项、多本 ledger JSON），但除 CI 预算外，没有任何账本
**驱动**行为。四个实测的失驱动面：

1. **manual 门禁层无节律**（`config/governance/manual-gate-freshness.json`，
   staleAfterDays: 90 ✓）：**38** 个 manual 门禁中 **12 个 `lastRun: null`
   从未运行**［原:35 个——manifest 在 FR-163 闭环后增至 38，`jq '[.scripts[]
   |select(.enforcement=="manual-runbook")]|length'`；12 这个数与逐名清单在
   `3fbda023` 仍逐字成立］（清单：certify-codex-session-resume、
   fr001-sandbox-matrix、health-policy-check、process-console-metrics/release/ui、
   self-bootstrap-cycle2、slack-dedicated-app-provisioning、
   slack-managed-live-smoke、slack-managed-shared-oauth、
   slack-skill-automation-release、watchdog）。占比 **12/38 = 31.6%**
   ［原:40%——按 12/35 与"40%"皆不合，12/35=34.3%］。

   已记录的 26 条**并非**来自同一修订［原:"其余 23 个的记录全部来自同一天同
   一修订（2026-08-10 @ 2ae4e854）"——**此句在它自己 pin 的修订上即为假**。
   在 `6678144d`：23 条记录散落 **9 个不同修订**，2ae4e854 只占其中 5 条；
   "同一天"成立而"同一修订"不成立。在 `3fbda023`：26 条散落 **14 个修订**、
   横跨两天。派生：`jq -r '.gates|to_entries[]|select(.value.lastRun!=null)
   |"\(.value.lastRun.date) \(.value.lastRun.revision[0:8])"'|sort|uniq -c`］。
   FR-160 是一次清扫而非习惯——这个结论成立，但支撑它的不是"同一修订"。

   其中 **10 个 `worktreeDirty: true`**［原:7——7 在 `6678144d` 成立，
   `3fbda023` 为 10］。FR-160 系列 6 张 ticket 中 5 张是此层的沉积腐化。

   **FR 未察觉的三处缺口（核验中发现，已提为需求 1 的子项）**：

   - **强制机制已经存在，只是 CI 不用它。** `scripts/qa/manual-gate-freshness.rb`
     的 `--strict` 会在 stale 非空时 `exit 1`（脚本头自述 "also fail on stale
     entries (not used by CI)"）。需求 1 的"强制点"因此是**接线**问题，不是
     构建问题。
   - **一次失败的运行同样刷新新鲜度时钟。** stale 判据只有
     `age.nil? || age > stale_after`；`exitStatus` 与 `worktreeDirty` 仅作为
     报告文案的括注，不进入判据。实测：`test-attention-inbox.sh` 于 2026-08-11
     记录 `exitStatus: 1`，今日读作 `ok`，`--strict` 也放行。这是 §4.4 shape 6
     ——状态字段报告的不是你在问的东西——且**当前活跃**。10 条 dirty 记录同理。
   - **该门禁在自己的 fail-closed 诊断里写死了 "35"**（第 52、58 行），
     而 manifest 现为 38。这正是 §4.4 shape 7 第三条实践（"从台账派生期望值，
     绝不复述"）失守在 FR-158 为"可派生"而建的守护自身上。

2. **回滚契约 0 行代码**：`rg 'forward-only' crates/orchestrator-persistence/src/migration.rs`
   → 0 ✓（**但需限定到 daemon 迁移内核**：`crates/slack-gateway/src/store.rs:163`
   与 `lib.rs:23` 已各带一行 "forward-only migrations" 文档注释，所以"代码里
   0 行"对整棵树不成立）。

   **`forward-only` 在本树是三义重载词，需求 2 的任何 grep 式守护必须先分类**
   ［原:"契约以散文复述于 8 份文档（DD-116:73、DD-111/121/122/124、guide ×2、
   威胁模型 T12）"——**既漏计又混类**］：

   - **A 类·daemon 迁移回滚契约**（本 FR 的正题）：**14 份文档 / 17 处**——
     DD-111:65、**DD-114:63**、DD-116:73、DD-121:158、DD-122:139、
     DD-124:73+91、**DD-126:97**、guide/agent-process-console-v1-operations:11、
     guide/slack-managed-connections:246、**guide/slack-reaction-skill-automation:469**
     （故 **guide ×3**，非 ×2）、威胁模型 T12:85、**QA 153:170**、**QA 161:189**、
     CHANGELOG:229-231。FR 漏掉 DD-114、DD-126、第三份 guide 与两份 QA 文档。
   - **B 类·Gateway 自有 schema**（另一个数据库，相关但非同一契约）：
     DD-125:47/112/185、architecture.md:192、slack-gateway/{store.rs:163,lib.rs:23}。
     FR 把 DD-125 计入了 A 类的语气里，但它讲的是 Gateway schema 1-2。
   - **C 类·完全无关**：`crates/orchestrator-collab/src/dag.rs:136`
     "Forward only the last `N` artifacts"——制品转发，与迁移无关。一道
     `rg 'forward-only'` 式的单源守护会把它算作契约引用。
   - 另有 2 处索引行（`docs/design_doc/README.md:115`、
     `docs/feature_request/README.md:275`），改指单源时应作为索引而非散文处理。

   vertical 门禁的钉过期事故（ticket 已闭环）即无人看守此窗口的直接后果。
3. **覆盖率棘轮单边**：`coverage/boundary-baseline.json` 自己的 reapproval
   注释承认 CLI 实际 52.86% 而基线 35.49% 继续通过（"keep passing while
   under-ratcheted"）；Rust 侧无分支信号；基线仅 macos-aarch64。
4. **audit.toml 无 unmatched 棘轮**：**18** 条忽略［原:19——两条独立派生一致：
   `grep -c '^\s*"RUSTSEC' .cargo/audit.toml` = 18；文件自述"17 unmaintained
   acceptances" + RUSTSEC-2024-0429（unsound）= 18。19 疑似
   `rg -c RUSTSEC`（=20，含注释行）的行计数减一，即 §6 记录的
   "grep -c 数的是行不是出现"］的退役条件写成给人看的 `cargo tree -i` 单行
   （`.cargo/audit.toml`），**无退役棘轮 ✓**——`scripts/qa/dependency-policy.rb`
   的 `check_audit` 只断言"每条 ignore 上方有注释"，不断言 crate 仍在树内；
   deny.toml 一侧的 `skip-is-live`（同文件 549-598 行）正是本需求要对齐的既有
   范式。RUSTSEC-2024-0429 "Nothing reminds anyone to re-check"（deny.toml 自注）。

## 需求

### 1. manual 门禁的分诊与节律

12 个 never-run 逐一三分类：可无头运行的降级为 ci-required 或 scheduled
（进 CI 预算核算，DD-172 规则适用）；确需人工的给 owner + 运行节律；
已无价值的按退役程序处理。staleness 从 advisory 升级为至少一个强制点
（建议：release 工作流的前置检查——过期即阻断发布，而非阻断日常推送，
避免重蹈"新鲜度门禁天天红"的反噬）。`worktreeDirty: true` 的记录裁定
是否作废重跑。

**Step 0 补入的三个子项**（依据见背景 1）：

1a. **接线而非新建。** `--strict` 已实现；强制点的工作是把它挂进
    release.yml。挂点已核验存在：job `slack-certification-status`
    （release.yml:26）就是同一形状的先例——它跑
    `certify-slack-managed-live.sh status` 报告证据新鲜度，且 `build` 与
    `gui-build` 都 `needs:` 它。新的强制步骤应挂进同一 job 或并列一个
    同样被 `needs:` 的 job，否则不阻断任何东西。

1b. **新鲜度判据必须把"运行成功"计入。** 当前 `exitStatus` 与
    `worktreeDirty` 不进 stale 判据，一次失败运行读作 fresh
    （`test-attention-inbox.sh` 当前实例）。判据改为：记录存在 **且**
    `exitStatus == 0` **且** 未 dirty，否则视同未运行。这项直接吃掉原
    需求里"`worktreeDirty: true` 的记录裁定是否作废重跑"的一半——裁定
    落进代码而不是留给人。

1c. **写死的 35 改为派生。** 第 52、58 行的 fail-closed 诊断复述常量，
    manifest 已是 38。期望值从 manifest 派生（或至少与 ledger 交叉核对），
    §4.4 shape 7 第三条实践。

### 2. 回滚契约代码化

契约("迁移前向-only；上一 release 二进制必须能服务当前 schema；restore
仅限灾难")写入 `crates/orchestrator-persistence/src/migration.rs` 一处
文档注释 + 一道守护（候选：迁移注册表的结构断言 / 现有 populated-upgrade
测试的契约化命名与强制），**A 类 14 份文档 / 17 处**散文改为指向单源
［原:"8 处散文"——见背景 2 的三类切分］。若判定不加 CI 门禁，理由按
DD-172 成文（vertical 门禁的钉推进规则已覆盖行为半边，QA 161）。

**守护必须先分类再计数。** `forward-only` 三义重载：任何"散文皆指向单源"
的门禁若以字面量 grep 实现，会把 B 类（Gateway 自有 schema，另一个数据库）
与 C 类（`dag.rs` 的制品转发）计为契约引用——§4.4 shape 4，文本模式冒充
语义属性。守护的作用域谓词本身是一条断言，按 §4.4 shape 9 的第三个前提
对待：它现在窄，是因为概念窄，还是因为今天的实例恰好落在里面？

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
- [ ] **强制点确实挂在 release 的阻断路径上**：负夹具须证明它能让 release
      失败，而非只在 `$GITHUB_STEP_SUMMARY` 里印一段字。一个不被任何
      `needs:` 引用的 job 满足"存在"而不阻断任何东西——这是 §4.4 shape 1
      （文本存在冒充执行）在工作流层的形式
- [ ] **`exitStatus != 0` 或 `worktreeDirty` 的记录视同未运行**，有行为断言：
      喂一条 `exitStatus: 1` 的今日记录 → `--strict` 红。当前
      `test-attention-inbox.sh` 就是这条断言的现成正例
- [ ] **freshness 门禁不再复述门禁总数**：manifest 增删一个 manual 门禁后
      诊断文案自动跟随（负夹具：改 manifest 不改脚本，不得出现陈旧数字）
- [ ] 回滚契约单源 + 守护在位；**A 类 17 处**散文指向单源（rg 派生清单差集
      为空），且 **B/C 类不被计入**——负夹具须含一条 B 类与一条 C 类实例，
      守护对它们必须保持沉默
- [ ] 基线更新至实测值且双边规则成文；下一次改进被自动收顶的行为断言
- [ ] audit.toml 棘轮存在负夹具（虚构一条已离树 crate 的 ignore → 红），
      **且含一条"crate 仍在树内但已非重复/已修复"的反向实例**——deny.toml
      一侧记录过 `--deny unmatched-skip` 覆盖不到这一半（CHANGELOG:154 案例 15b），
      同样的缺口不应在 audit 侧重演
- [ ] 治理预算仍 ≤2700s，变动量记录（当前 1793/2700，DD-172）

## 依赖与关联

- 承接 FR-158（新鲜度台账的建立者——本 FR 是它预留的"攒出历史再选阈值"
  的下一步）、DD-172（预算与 shape 规则约束需求 1 的一切降级）、
  QA 161（钉推进规则）。

## 未核验项（明确标注）

- 12 个 never-run 中多少可无头化未逐一验证（分诊时逐个实跑判定）。
  Step 0 的**弱信号**（`rg -c 'SLACK_|_TOKEN|CLIENT_SECRET|read -r|xcrun|codex|claude '`，
  单方法、**未实跑**）：只有 `certify-codex-session-resume.sh`（14 命中）与
  `test-slack-managed-live-smoke.sh`（28 命中）显式提及凭据/交互；其余 10 个
  该模式 0 命中，含三个 process-console 门禁与 `watchdog.sh`（48-81 行的小
  脚本）。此信号只说明"没写这些词"，不说明可无头运行——它正是 §4.4 shape 1
  所警告的形状，实跑才作数。
- cargo-audit 对 unmatched-ignore 的原生支持程度**仍未调研**（step 0 未做）。
  但离线比对范式已在树内：`dependency-policy.rb` 的 `skip-is-live`
  （549-598 行）不依赖 cargo-deny 二进制即可判定 skip 是否仍是重复；
  audit 侧可照搬 Cargo.lock 比对，原生支持与否不构成阻塞。
- ~~release 工作流当前是否有可挂前置检查的结构未核验。~~ **已核验：有。**
  `release.yml:26` 的 `slack-certification-status` 是同形先例，`build`
  （:50）与 `gui-build`（:112）均 `needs:` 它。挂点存在且已被依赖。
