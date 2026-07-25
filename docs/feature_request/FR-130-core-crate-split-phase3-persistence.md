# FR-130: Core Crate 拆分 Phase 3 — persistence 提取

## 优先级: P1

## 状态: In Progress

需求 1（边界识别与冻结基线）与需求 3（迁移语义等价证明）已闭环，其设计与验证由
[DD-142](../design_doc/orchestrator/142-core-boundary-freeze.md) 与
[QA 180](../qa/orchestrator/180-core-boundary-freeze.md) 承载。

**2026-07-25 重写**：原需求 2（crate 提取）实际包含三件粒度、风险与前置条件都不同的事，
原需求 4 经实测近乎空转。本文档按此重新切分，并将"非 core crate 直接依赖 `rusqlite`"
一轴移交 [FR-136](FR-136-persistence-dependency-chokepoint-decision.md)。

## 背景

FR-047（Phase 1，提取 `orchestrator-config`）与 FR-048（Phase 2，提取 `orchestrator-scheduler`）
已闭环，但 `core`（`agent-orchestrator`）仍是事实上的 god crate。

原文的实测数字有多处偏差，现按 2026-07-25 的实际扫描更正（口径见
`config/governance/core-boundary-ledger.json` 的 `scope`）：

| 指标 | 原文 | 实测 |
|---|---|---|
| 生产代码行数 | 81194（约 46%） | 79659 / 176656 = 45% |
| 扫描文件数 | 157 | 143 |
| `lib.rs` 顶层 `pub mod` | 52 | 52 ✅ |
| 公开项 | 742 | **924**（原文正则漏掉 `pub async fn`） |
| 迁移表数 | 51 | **46 张表 + 92 个索引**（74 条已注册迁移） |
| `core/src/lib.rs` churn | 40 次 / 约 400 次提交 | **64 次 / 1022 次提交** |
| `#[allow(clippy::too_many_arguments)]` | 43（读作 core 内部耦合症状） | 43 是**全工作区**总数；core 只有 **3**，集中在 scheduler(21)、gui(7)、daemon(7) |

churn 的结论仍然成立：持久化层是 core 内部最高频的变更簇。

## 已闭环部分

### ✅ 需求 1：边界识别与冻结基线

`config/governance/core-boundary-ledger.json` 冻结 core 的模块面（52 `pub mod`、924 公开项、
143 个文件）、**逐文件**的 200 处 `rusqlite` 引用（37 个文件），以及直接依赖 `rusqlite` 的
crate 清单。`scripts/qa/core-boundary.rb` 以**精确相等**比对，`--emit-baseline` / `--write`
提供受审的再生路径。`scripts/lib/rust_source.rb` 是两个治理台账共用的唯一扫描器。

**口径偏离说明**：原需求 4 要求"单调不增"，实现改为精确相等。单调规则下**下降会静默通过**，
台账继续声称仓库已不再背负的债务——门禁全绿而陈述为假。在本 FR 的场景里这更尖锐，
因为下降正是提取的目的：单调规则会恰好忽略台账唯一存在意义的那个事件。

### ✅ 需求 3：迁移语义等价证明

`config/governance/schema-snapshot.sql` 记录 74 条迁移对空库执行后的规范化 schema
（46 表 + 92 索引）。`core/src/persistence/schema_snapshot.rs` 以 `cargo test` 守住它：
全链等价、幂等（同时比对 applied 计数与 schema）、以及**在全部 74 个中断点**逐一验证
续跑结果与一次跑完全一致。

这条基线必须在提取**之前**提交：提取之后再记录就没有比对对象了。

## 前置条件（本 FR 不实施，但阻塞其余需求）

在 Phase A 开始之前，下列两项必须完成：

1. ~~扫描器词法安全~~ — **已由 FR-134 需求 9 完成**，见
   [DD-145](../design_doc/orchestrator/145-gate-surface-execution-truth.md)。
   `scripts/lib/rust_source.rb` 的 `strip_test_modules` 曾按行统计花括号，字符串字面量中的
   `{` 会使 `cfg(test)` 块永不闭合，其后的生产代码从扫描中消失；`error.rs`（283 行起）与
   `source_task_template.rs`（363 行起）正是该形态，而 `error.rs` 是 Phase C 的对象。现由
   `scripts/lib/rust_lexer.rb` 跨行维护字符串、字符、原始字符串与嵌套注释状态，
   `test-core-boundary.sh` 的 Case 10/11/12 双向固定：缺陷方向（被隐藏的生产代码必须重新
   被计数）与过修方向（含跨行原始字符串的尾部测试模块不得被提前闭合）。

   **本 FR 的成功判据是 200 → 0 的收敛，而这个数字由该扫描器产出。** 修复后基线经确认仍为
   `200 / 37`——不变才说明只修了缺陷、没换口径；一个逐行正则的"显然修法"会把
   `capturesOrJsonPath` 从 53 推到 60，正是靠这条判据挡下的。尺子现已可信。

2. **[FR-136](FR-136-persistence-dependency-chokepoint-decision.md) — 依赖收口决策。**
   除 core 外另有 4 个生产 crate、23 个文件、75 处直接引用 `rusqlite`。若不先决定新 crate
   是收口点还是共享底座，提取最可能的结局是 `orchestrator-persistence` 被 5 个 crate 共同
   依赖，驱动传播面一个字节没减少，只是多了一层目录结构。

## 未完成部分

原需求 2 的文件清单与其验收标准不相容，需按台账重写。原文列出 11049 行的 14 个文件并据此
提出"core 不再直接依赖 rusqlite"，但实际有 **37 个文件、200 处引用**，其中约 22 个文件
不在清单内。它们不是"被遗漏的持久化模块"，而是**在同一个函数里混装 SQL 与领域逻辑**，
所以本来就不在 persistence 目录下。原文清单中的 `core/src/migration_steps.rs` 亦不存在，
实际路径是 `core/src/persistence/migration_steps.rs`。

因此按风险与前置条件切分为三个阶段，**各自独立可回退**。

### ⏳ Phase A：纯持久化模块外迁

边界明确、风险可控的部分。以台账 `rusqlite.files` 中路径已在持久化语义下的文件为范围：

| 文件 | 引用数 |
|---|---|
| `core/src/task_repository/**`（mod 45、queries 4、write_ops 4、items 2、state 2、types 1） | 58 |
| `core/src/async_database.rs` | 17 |
| `core/src/persistence/repository/**`（workflow_store 14、session 10、config 3、scheduler 3） | 30 |
| `core/src/persistence/{migration_steps,migration,sqlite}.rs` | 6 |
| `core/src/{db,db_write,db_maintenance,migration}.rs` | 4 |

约 115 处、23 个文件。这一阶段建立 `orchestrator-persistence` crate 本体，保持
`#![deny(missing_docs)]` 与既有 lint 约定。

验收以 `schema-snapshot.sql` **逐次不变**为行为证据——纯结构性的"符号已移动"不足以
证明迁移仍然正确。

### ⏳ Phase B：混装文件的逐批拆分

约 22 个文件，SQL 与领域逻辑在同一函数内，各自是一次独立的设计判断：

| 文件 | 引用数 |
|---|---|
| `trigger_engine.rs` | 18 |
| `action_audit.rs` | 9 |
| `service/bootstrap.rs`、`source_automation.rs` | 各 7 |
| `event_cleanup.rs`、`handoff.rs`、`source.rs`、`source_connection.rs` | 各 5 |
| `task_ops.rs` | 4 |
| `attention.rs`、`events.rs`、`process_metrics.rs`、`session_store.rs` | 各 3 |
| `config_load/build.rs` | 2 |
| `config_load/persist.rs`、`lib.rs`、`service/resource/delete.rs`、`task_cleanup.rs` | 各 1 |

约 83 处。要求：

- **按文件分批**，每批一次提交、一次 QA、一次可回退证据。不接受"一次性大提交"。
- 每个文件明确处置：整体迁出、拆分（SQL 迁出、领域逻辑留下）、或书面记录保留理由。
- 台账 `rusqlite.files` 的收敛即为机器可读的进度证明——每批之后该文件的条目应消失或减少。
- 每批之后 `schema-snapshot.sql` 不变。

### ⏳ Phase C：`error.rs` 的驱动耦合决策

`core/src/error.rs:154` 持有 `impl From<rusqlite::Error> for OrchestratorError`。无论移动
哪些文件，core 的错误类型都仍与驱动耦合——这是一个单点阻塞，且**需要先出方案再动手**：
是引入 port 层错误类型并在边界转换，还是接受该耦合并显式记录理由。

该文件同时是前置条件 1 中扫描器缺陷的受影响文件之一（`cfg(test)` 块自 283 行起未闭合），
处置前须确认扫描器已修复。

### ❌ 原需求 4：跨边界的 `too_many_arguments` — 不适用，已关闭

原文据"43 个豁免"推出本条。实测 core 只有 **3 个**，工作区的 43 个集中在 scheduler(21)、
gui(7)、daemon(7)。该条对本次提取近乎空转，作为 FR-130 的需求已关闭。scheduler 的 21 个
豁免若值得治理，应作为独立议题立项，不挂在持久化提取之下。

## 验收标准

### 已达成

- [x] `core` 的 `pub mod` 数与公开项数相对基线精确冻结，门禁存在且有负向 fixture
- [x] 提取前完整迁移链产出的 schema 已以可复现脚本记录并冻结（46 表 + 92 索引）
- [x] 迁移幂等性与断点续跑回归通过（全部 74 个中断点）
- [x] 跨边界的 `too_many_arguments` 已书面记录（core 实为 3 个，见 DD-142）
- [x] 对外 gRPC/CLI 契约无变化，既有集成测试全绿且未修改断言

### 前置

- [ ] FR-134 需求 9 已闭环，且修复后 `core-boundary-ledger.json` 的 `200 / 37` 与
      `52 / 924 / 143` 未变化（变化项须逐条说明）
- [ ] FR-136 已闭环，收口形态已选定

### Phase A

- [ ] `orchestrator-persistence` crate 存在，承载迁移内核与 repository 实现，`#![deny(missing_docs)]` 通过
- [ ] 台账中属 Phase A 范围的约 115 处引用已从 core 收敛
- [ ] `cargo test -p agent-orchestrator schema_snapshot` 通过，`schema-snapshot.sql` 未变
- [ ] Phase A 的提取 commit 可机械回退（与 FR-126 的 reverse-applicable removal patch 同一标准）

### Phase B

- [ ] 22 个混装文件各有"已迁出 / 已拆分 / 保留并记录理由"的结论
- [ ] 每批独立提交且各自可回退；每批之后 `schema-snapshot.sql` 未变
- [ ] 台账 `rusqlite.files` 随每批单调收敛，残余点显式清单化

### Phase C

- [ ] `error.rs` 的驱动耦合已按书面决策处置（转换层或显式保留）
- [ ] core 对 `rusqlite` 的引用收敛至 0，或残余点带理由记录并被门禁冻结

## QA 计划（剩余部分）

- **Schema 等价性**：比对对象已存在。每个 Phase、每一批之后运行
  `cargo test -p agent-orchestrator schema_snapshot`，diff 必须为空。这是本 FR 最关键的
  行为证据——纯结构性的"符号已移动"检查不足以证明迁移仍然正确。
- **逐文件处置证据**：37 个文件中每一个都要有结论，以 `core-boundary-ledger.json` 的收敛
  作为机器可读证明，而非散文式的"已审阅"。
- **下游无感证明**：daemon/cli/scheduler 的既有集成测试在不修改断言的前提下全绿。
- **分批回滚可行性**：每批提取 commit 各自可机械回退。一次性大提交无法满足此项——这是
  分批要求的技术理由，不只是流程偏好。
- **尺子先于测量**：Phase A 开始前重跑边界门禁，确认修复后的扫描器给出与冻结时相同的
  `200 / 37`。若数字变化，说明冻结时的基线本身受缺陷影响，须先重新冻结再开始提取。
