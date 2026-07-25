# FR-130: Core Crate 拆分 Phase 3 — persistence 提取

## 优先级: P1

## 状态: In Progress

需求 1（边界识别与冻结基线）与需求 3（迁移语义等价证明）已闭环，其设计与验证由
[DD-142](../design_doc/orchestrator/142-core-boundary-freeze.md) 与
[QA 180](../qa/orchestrator/180-core-boundary-freeze.md) 承载。需求 2（crate 提取）与需求 4
（跨边界棘轮的第二条子句）仍未开始，本文档保留这两项，并按实测重写其口径。

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
| 公开项 | 742 | **924**（原文正则漏掉 `pub async fn`；剔除 `cfg(test)` 后同步项为 710，异步 214） |
| 迁移表数 | 51 | **46 张表 + 92 个索引**（74 条已注册迁移） |
| `core/src/lib.rs` churn | 40 次 / 约 400 次提交 | **64 次 / 1022 次提交** |
| `core/src/migration.rs` churn | 45 | 48 |
| `#[allow(clippy::too_many_arguments)]` | 43（读作 core 内部耦合症状） | 43 是**全工作区**总数；core 只有 **3**，集中在 scheduler(21)、gui(7)、daemon(7) |

churn 的结论仍然成立：持久化层是 core 内部最高频的变更簇。

## 已闭环部分

### ✅ 需求 1：边界识别与冻结基线

`config/governance/core-boundary-ledger.json` 冻结 core 的模块面（52 `pub mod`、924 公开项、
143 个文件）、**逐文件**的 200 处 `rusqlite` 引用（37 个文件），以及直接依赖 `rusqlite` 的
6 个 crate。`scripts/qa/core-boundary.rb` 以**精确相等**比对（而非原文要求的单调不增，理由见下），
`--emit-baseline` / `--write` 提供受审的再生路径，`--write` 在 `CI` 下拒绝执行。
`scripts/lib/rust_source.rb` 是两个治理台账共用的唯一扫描器。

**口径偏离说明**：需求 4 要求"单调不增"，实现改为精确相等。单调规则下**下降会静默通过**，
台账继续声称仓库已不再背负的债务——门禁全绿而陈述为假；FR-128 就是因此让
`capturesOrJsonPath` 在 54 对 55 的状态下潜伏了一整个 FR 周期。在本 FR 的场景里这更尖锐，
因为下降正是提取的目的：单调规则会恰好忽略台账唯一存在意义的那个事件。

### ✅ 需求 3：迁移语义等价证明

`config/governance/schema-snapshot.sql` 记录 74 条迁移对空库执行后的规范化 schema
（46 表 + 92 索引）。`core/src/persistence/schema_snapshot.rs` 以 `cargo test` 守住它：
全链等价、幂等（同时比对 applied 计数与 schema，因为只断言"applied=0"对一个重复执行 DDL
却改变 schema 的链条同样成立）、以及**在全部 74 个中断点**逐一验证续跑结果与一次跑完全一致。
抽样验证会漏掉恰好落在未抽中那一步的缺陷。

这条基线必须在提取**之前**提交：提取之后再记录就没有比对对象了。它同时补上一个独立的缺口——
在此之前，新增一条迁移会改变 46 张表的 schema 而没有任何可审阅的产物。

## 未完成部分

### ⏳ 需求 2：orchestrator-persistence crate 提取

**原文的文件清单不包含持久化层，需按台账重写。** 原文列出 `persistence/**`、`db.rs`、
`db_write.rs`、`async_database.rs`、`migration.rs`、`migration_steps.rs`、`task_repository/**`
共 11049 行，并据此提出验收标准"core 不再直接依赖 rusqlite"。二者不相容：

- 实际有 **37 个 core 文件、200 处 `rusqlite` 引用**，其中约 22 个文件不在清单内——
  `trigger_engine.rs`(18)、`action_audit.rs`(9)、`service/bootstrap.rs`(7)、
  `source_automation.rs`(7)，以及 `event_cleanup.rs`、`attention.rs`、`source.rs`、
  `task_ops.rs`、`session_store.rs`、`handoff.rs`、`process_metrics.rs`、`config_load/**`。
  它们不是"被遗漏的持久化模块"，而是**在同一个函数里混装 SQL 与领域逻辑**，所以本来就不在
  persistence 目录下。
- `error.rs` 持有 `impl From<rusqlite::Error> for OrchestratorError`，无论移动哪些文件，
  core 的错误类型都仍与驱动耦合。
- 原文清单中的 `core/src/migration_steps.rs` **不存在**，实际路径是
  `core/src/persistence/migration_steps.rs`。

**core 也不是持久化的收口点。** 6 个 crate 直接声明 `rusqlite`：`core`、`daemon`、
`orchestrator-scheduler`、`orchestrator-security`、`slack-gateway`、`integration-tests`，
涉及 19 个非 core 文件。在 core 里定义 port trait 并不能阻止这一点——那些 crate 会转而
直接依赖新 crate，与目标相反。提取因此有一条原文从未纳入预算的第二轴。

重写后的需求：

- 以 `core-boundary-ledger.json` 的 `rusqlite.files` 逐文件清单为工作项，而非原文的 14 文件清单。
- 对每个文件明确处置：整体迁出、拆分（SQL 迁出、领域逻辑留下）、或书面记录保留理由。
- 单独立项处理"6 个 crate 直接依赖 rusqlite"这一轴；它决定新 crate 是收口点还是又一个共享依赖。
- 提取过程中 `schema-snapshot.sql` 必须逐次保持不变——这是提取正确性的行为证据。

### ⏳ 需求 4：跨边界的 `too_many_arguments`

原文据"43 个豁免"推出本条。实测 core 只有 3 个，工作区 43 个集中在 scheduler(21)。
本条对本次提取近乎空转，应在重写需求 2 时按实际跨边界函数重新界定，或并入 scheduler 的独立议题。

## 验收标准

- [x] `core` 的 `pub mod` 数与公开项数相对基线精确冻结，门禁存在且有负向 fixture（`test-core-boundary.sh` 案例 3、5、8、9）
- [x] 提取前完整迁移链产出的 schema 已以可复现脚本记录并冻结（46 表 + 92 索引）
- [x] 迁移幂等性与断点续跑回归通过（全部 74 个中断点）
- [x] 跨边界的 `too_many_arguments` 已书面记录（core 实为 3 个，见 DD-142）
- [x] 对外 gRPC/CLI 契约无变化，既有集成测试全绿且未修改断言
- [x] `cargo test --workspace`、strict Clippy、协调 strangler 门禁全部通过
- [ ] `orchestrator-persistence` crate 存在，承载迁移内核与 repository 实现，且 `#![deny(missing_docs)]` 通过
- [ ] `core` 对 `rusqlite` 的 37 文件 / 200 处引用按逐文件处置清单收敛，残余点显式清单化
- [ ] 提取前后 schema 逐表逐列一致（以已冻结的 `schema-snapshot.sql` 为比对对象）
- [ ] 提取 commit 可机械回退（与 FR-126 的 reverse-applicable removal patch 同一标准）

## QA 计划（剩余部分）

- **Schema 等价性**：比对对象已存在。提取后运行 `cargo test -p agent-orchestrator schema_snapshot`，
  diff 必须为空。这是本 FR 最关键的行为证据——纯结构性的"符号已移动"检查不足以证明迁移仍然正确。
- **逐文件处置证据**：37 个文件中每一个都要有"已迁出 / 已拆分 / 保留并记录理由"的结论，
  以 `core-boundary-ledger.json` 的收敛作为机器可读证明。
- **下游无感证明**：daemon/cli/scheduler 的既有集成测试在不修改断言的前提下全绿。
- **回滚可行性**：证明提取 commit 可机械回退。
