# FR-130: Core Crate 拆分 Phase 3 — persistence 提取

## 优先级: P1

## 状态: 可闭环（三个 phase 均已完成，18 个文件全部处置；闭环判断与依据见文末）

需求 1（边界识别与冻结基线）与需求 3（迁移语义等价证明）已闭环，其设计与验证由
[DD-142](../design_doc/orchestrator/142-core-boundary-freeze.md) 与
[QA 180](../qa/orchestrator/180-core-boundary-freeze.md) 承载。

**需求 2 的 Phase A、Phase B 与 Phase C 均已闭环（18 / 18 文件各有一条书面处置：
15 个已迁出或拆分、3 个保留并记录理由）**，其设计与验证由
[DD-148](../design_doc/orchestrator/148-persistence-crate-extraction.md) 与
[QA 186](../qa/orchestrator/186-persistence-crate-extraction.md) 承载。
core 已从 200 处 `rusqlite` 引用 / 37 文件降至 **9 处 / 3 文件**，逐文件结论见下面的
「逐文件处置表」。残余 9 处全部是**持久化层交出的驱动连接类型**，属
[FR-141](FR-141-persistence-connection-api-boundary.md) 的对象而非本 FR 的。

**2026-07-25 重写**：原需求 2（crate 提取）实际包含三件粒度、风险与前置条件都不同的事，
原需求 4 经实测近乎空转。本文档按此重新切分，并将"非 core crate 直接依赖 `rusqlite`"
一轴移交 FR-136，该 FR 已闭环，结论由
[DD-147](../design_doc/orchestrator/147-persistence-dependency-chokepoint.md) 承载。

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
| 迁移表数 | 51 | **46 张表 + 92 个索引**（**37** 条已注册迁移；治理期曾记作 74，那是 `grep -c m00` 的结果，每条迁移被 `name:` 与 `up:` 各匹配一次） |
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

`config/governance/schema-snapshot.sql` 记录 37 条迁移对空库执行后的规范化 schema
（46 表 + 92 索引）。`core/src/persistence/schema_snapshot.rs` 以 `cargo test` 守住它：
全链等价、幂等（同时比对 applied 计数与 schema）、以及**在全部 37 个中断点**逐一验证
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

2. **FR-136 — 依赖收口决策（已闭环，见
   [DD-147](../design_doc/orchestrator/147-persistence-dependency-chokepoint.md)）。**
   除 core 外另有 4 个生产 crate、15 个文件、55 处直接引用 `rusqlite`（本条原写作
   23 个文件 75 处，那是把测试代码计入的 `grep` 结果；治理期已按 DD-142 的口径改正）。
   若不先决定新 crate
   是收口点还是共享底座，提取最可能的结局是 `orchestrator-persistence` 被 5 个 crate 共同
   依赖，驱动传播面一个字节没减少，只是多了一层目录结构。

## 未完成部分

原需求 2 的文件清单与其验收标准不相容，需按台账重写。原文列出 11049 行的 14 个文件并据此
提出"core 不再直接依赖 rusqlite"，但实际有 **37 个文件、200 处引用**，其中 18 个文件
不在清单内。它们不是"被遗漏的持久化模块"，而是**在同一个函数里混装 SQL 与领域逻辑**，
所以本来就不在 persistence 目录下。原文清单中的 `core/src/migration_steps.rs` 亦不存在，
实际路径是 `core/src/persistence/migration_steps.rs`。

因此按风险与前置条件切分为三个阶段，**各自独立可回退**。

**尺子读法（2026-07-26 治理期补记）**：台账数的是 `rusqlite` 这个**词元**的出现次数，
不是 SQL 语句数。`db_write.rs` 有 1441 行 SQL，计 **1**；`db.rs` 有 1104 行，计 **1**。
所以 Phase A 的"约 115 处"对应约 12100 行，Phase B 的"约 83 处"对应 **17963 行**——
数字更小的 Phase B 反而更大。这个计数是合格的棘轮（它能发现新增的耦合），但**不是工作量估计**，
下面的分期不应被读成按规模排序。

### ✅ Phase A：纯持久化模块外迁（2026-07-26 闭环）

边界明确、风险可控的部分。以台账 `rusqlite.files` 中路径已在持久化语义下的文件为范围：

| 文件 | 引用数 |
|---|---|
| `core/src/task_repository/**`（mod 45、queries 4、write_ops 4、items 2、state 2、types 1） | 58 |
| `core/src/async_database.rs` | 17 |
| `core/src/persistence/repository/**`（workflow_store 14、session 10、scheduler 3） | 27 |
| `core/src/persistence/{migration_steps,migration,sqlite}.rs` | 6 |
| `core/src/{db,db_write,db_maintenance,migration}.rs` | 4 |
| `core/src/session_store.rs` | 3 |

共 115 处、**18 个文件**（原文写作 23，与上表不符；上表为准）。这一阶段建立
`orchestrator-persistence` crate 本体，保持 `#![deny(missing_docs)]` 与既有 lint 约定。

两处成员调整，均为**结构性必然**而非偏好，理由记录如下：

- **`core/src/persistence/repository/config.rs`（3 处）移出 Phase A，改属 Phase B。**
  它有 17 处生产引用指向 `crate::crd`，另有 `crate::resource`；而 `core/src/crd/plugins.rs:328`
  调用 `crate::db::insert_plugin_audit`，`db.rs` 是 Phase A 文件。若 `db.rs` 与 `config.rs`
  同时下沉而 `crd` 留在 core，得到的是 `persistence → crd → persistence` 循环依赖。
  它位于 `persistence/` 目录下但是一个**领域仓储**——目录位置与结构类别不是一回事。
  **这条只对 Phase A 的问题成立**（整个文件能否下沉）。Phase B 问的是语句能否下沉，
  那个答案不同，见下面处置表中该文件那一行。
- **`core/src/session_store.rs`（3 处）移入 Phase A。** 其全部 import 是 `async_database`、
  `config_load::now_ts`、`persistence::repository::{SessionRepository, SqliteSessionRepository}`
  与 `db`，无任何领域耦合；且 Phase A 的 `persistence/repository/session.rs` **依赖它**——
  不带上它，Phase A 无法编译通过。

两项互换后 Phase A 仍为 115 处 / 18 个文件，Phase B 仍为 83 处 / 18 个文件。

验收以 `schema-snapshot.sql` **逐次不变**为行为证据——纯结构性的"符号已移动"不足以
证明迁移仍然正确。

### ✅ Phase B：混装文件的逐批拆分（2026-07-27 闭环）

**18 个文件**（原文写作 22，与上表不符；上表为准），SQL 与领域逻辑在同一函数内，
各自是一次独立的设计判断：

| 文件 | 引用数 |
|---|---|
| `trigger_engine.rs` | 18 |
| `action_audit.rs` | 9 |
| `service/bootstrap.rs`、`source_automation.rs` | 各 7 |
| `event_cleanup.rs`、`handoff.rs`、`source.rs`、`source_connection.rs` | 各 5 |
| `task_ops.rs` | 4 |
| `attention.rs`、`events.rs`、`process_metrics.rs`、`persistence/repository/config.rs` | 各 3 |
| `config_load/build.rs` | 2 |
| `config_load/persist.rs`、`lib.rs`、`service/resource/delete.rs`、`task_cleanup.rs` | 各 1 |

共 83 处、17963 行——按上面的"尺子读法"，这一阶段比 Phase A 大。要求：

- **按文件分批**，每批一次提交、一次 QA、一次可回退证据。不接受"一次性大提交"。
- 每个文件明确处置：整体迁出、拆分（SQL 迁出、领域逻辑留下）、或书面记录保留理由。
- 台账 `rusqlite.files` 的收敛即为机器可读的进度证明——每批之后该文件的条目应消失或减少。
- 每批之后 `schema-snapshot.sql` 不变。

#### 引用形态分类（2026-07-26 治理期实测）

**83 处中约 61% 不是 SQL，而是驱动错误管道**；而 SQL 文本本身在字符串字面量里，
尺子从不计数：

| 形态 | 数量 | 例 |
|---|---|---|
| error-adapter | 28 | `tokio_rusqlite::Error::Other(e.into())`、`-> tokio_rusqlite::Error` |
| sql-params | 21 | `rusqlite::params![…]`、`params_from_iter`、`ToSql` |
| import | 12 | `use rusqlite::{Connection, OptionalExtension, params};` |
| error-construction | 11 | `rusqlite::Error::FromSqlConversionFailure`、`rusqlite::types::Type` |
| connection-type | 6 | `conn: &rusqlite::Connection` |
| row-mapping | 4 | `row: &rusqlite::Row`、`rusqlite::Result<T>` |

其中六个文件带着**完全相同**的两行 `fn other(…) -> tokio_rusqlite::Error` 助手，
所以"18 个各自独立的设计判断"高估了差异性。

**由此产生的诱惑，以及为什么不采纳**：给 `AsyncDatabase` 加接受 `anyhow::Result`
闭包的方法、删掉那六份 `fn other`，能在**不搬动任何一条 SQL** 的前提下把 83 收敛掉约 39。
这与 Phase A 拒绝为 `core/src/migration.rs` 加 `run_pending_count` 是同一笔交易，只是规模
大二十倍。**本 FR 不采纳。** Phase B 的目标是逐文件处置，台账是它的证据而非目标。
（也核对过这个改动能否以另一条理由立足——DD-147 里 `daemon` 与 `orchestrator-scheduler`
的 forbidden 残量。不能：它们 39 处中有 27 处是 sql-params，只有 9 处是 error-adapter。）

#### 逐文件处置表

| 文件 | 引用 | 行数 | 处置 |
|---|---|---|---|
| `lib.rs` | ~~1~~ 0 | 178 | ✅ **已修正陈述**（B0）。唯一一处非代码引用：文档注释把 `async_database` 描述为仍在 core，Phase A 已使其为假 |
| `service/resource/delete.rs` | ~~1~~ 0 | 452 | ✅ **已迁出**（B1）。`DELETE FROM resources` → `db::delete_project_resources`；同时解锁 Phase C |
| `config_load/persist.rs` | ~~1~~ 0 | 465 | ✅ **已修正范围**（B2）。生产代码零引用；`#[cfg(test)] use` 在文件级被扫描器计为生产 |
| `task_cleanup.rs` | ~~1~~ 0 | 290 | ✅ **已拆分**（B3）。保留查询 → `queries::list_terminal_tasks_older_than`；级联删除改用既有 async repository 方法（原为手写重复）；文件系统清理留在 core |
| `config_load/build.rs` | ~~2~~ 0 | 918 | ✅ **已拆分**（B4）。删除守卫改吃 `db::DeletionGuardQueries` port；守卫逻辑现可无数据库单测 |
| `events.rs` | ~~3~~ 0 | 700 | ✅ **已拆分**（B5）。行访问 → `events::{StepEventRow, step_event_rows}`；payload 解释留在 core；事件类型清单**上移**为 core 的 `STEP_EVENT_TYPES` 并作参数下传 |
| `trigger_engine.rs` | ~~18~~ 0 | 1130→983 | ✅ **已拆分**（B16）。`trigger_state` 表与周边任务读共 7 条语句 → `trigger_state`；调度判断留在 core。两条此前无名的判断获得了名字：`ACTIVE_TASK_STATUSES`（原为闭包内的 `matches!` 分支，它决定 `Skip` 策略是否让触发器一直关着）与 `trigger_task_name`（原为写在两处的 `format!`，两处一旦不一致，历史清理会静默匹配不到任何东西）。**搬迁中发现一个真实缺陷**：`DELETE FROM tasks` 不清子行而 `task_items` 不级联，所以触发器历史上限对任何真正跑过的任务都被 FK 拒绝、从未生效；已记入 DD-148 的 Known limits 并由断言钉住当前行为，未在本批修复 |
| `action_audit.rs` | ~~9~~ 0 | 742→442 | ✅ **已拆分**（B8）。表与 7 条语句 → `control_action_audit`；字段边界、canonical hash、生命周期 allowlist、幂等冲突规则留在 core。存储只报告 `Reservation::{Claimed, PriorByRetryIdentity, PriorByRequestId}`，不解释其含义 |
| `service/bootstrap.rs` | ~~7~~ 0 | 597 | ✅ **已拆分**（B7）。6 条 scope backfill + SecretStore 引用探针 → `db`；渲染 workspace_root、序列化 qa_targets、`unwrap_or(false)` 留在 core |
| `source_automation.rs` | ~~7~~ 0 | 2008→686 | ✅ **已拆分**（B15）。四张表 33 条语句 → `source_automation_routes`；字段边界、automation key 与由它派生的四个标识符、退避、哪些状态释放租约、状态投影的时龄算术、以及每个被拒绝的 fence 对运维意味着什么留在 core。这一批清掉了 core 里**最后两处 `FromSqlConversionFailure`**。**28 处变异，其中 5 处无可达 fixture**，逐条记在测试里而不是留成绿断言 |
| `source.rs` | ~~5~~ 0 | 1337→650 | ✅ **已拆分**（B11）。四张表 24 条语句 → `source_events`；校验、确定性 id 推导、30 秒退避、终态 allowlist 留在 core。**发现 5 处安全护栏此前无任何测试守护**，见下 |
| `source_connection.rs` | ~~5~~ 0 | 1923→1100 | ✅ **已拆分**（B13）。五张表 23 条语句 → `source_connections`；字段边界、模式/终态 allowlist、每个被拒绝的 fence 对运维意味着什么、以及枚举与 JSON 的解析留在 core。**16 处 fence 逐一变异，其中 3 处各有多份拷贝**（`version=?3` ×3、`state='active'` ×4、`owner_daemon_id=?3` ×2），每份拷贝各做一次变异 |
| `handoff.rs` | ~~5~~ 0 | 1288→930 | ✅ **已拆分**（B14）。三张表 18 条语句 → `handoff_store`。**动它的理由不在引用数里**：`task_state_version` 跑三个 `git` 子进程并读遍工作区未跟踪文件，而每个调用点都在 `writer().call` 闭包内，`reserve_execution` 更是在其**事务内**——单写者数据库的写锁被一个外部进程树占住。现在读取以 inputs 形式返回，投影、摘要与哈希都在 core |
| `event_cleanup.rs` | ~~5~~ 0 | 845→330 | ✅ **已拆分**（B10）。保留期语句 → `event_retention`；JSONL 归档的分组与写文件留在 core。这一批取消了「磁盘错误伪装成驱动类型转换错误」 |
| `task_ops.rs` | ~~4~~ 0 | 1826 | ✅ **已拆分**（B9）。两条重复的建任务写路径合并为一个事务 → `task_repository::creation`；FR-094 诊断事件改为**构造**而非写入 |
| `attention.rs` | 3 | 1454 | ⏸️ **保留并记录理由**。import 1 + `fn other` 2。其 SQL 全部在 `writer().call` 闭包内，与 `source.rs` 同形；要清零就得整体迁出，而**只动管道的那条路已被本 FR 拒绝**并移交 [FR-141](FR-141-persistence-connection-api-boundary.md)。今天没有搬 SQL，所以今天的诚实结论就是保留 |
| `process_metrics.rs` | 3 | 1953 | ⏸️ **保留并记录理由**。同 `attention.rs`，理由同上，同样指向 FR-141 |
| `persistence/repository/config.rs` | 3 | 695 | ⏸️ **保留并记录理由**（2026-07-27 更正，原记作「被阻塞，解锁条件是 crd 先下沉」）。connection-type 2 + import 1，三处**全部是驱动连接类型**：import、`open_conn(&self) -> Result<rusqlite::Connection>`（即持久化层 `db::open_conn` 的再导出）、以及交给 orchestrator-security 密钥审计的 `&rusqlite::Connection`。**原解锁条件是错的**：它回答的是 Phase A 的问题（整个文件能否下沉），而 Phase B 问的是语句能否下沉——本文件的语句全部落在 `orchestrator_config_versions`、`config_heal_log`、`resources`、`resource_versions` 与 `sqlite_master` 上，皆为平字段加 JSON 文本，**没有任何一条需要 crd 类型**，不成环，也没有任何东西需要一个 crd 下沉 FR 来承接。而即便把语句全部搬下去也清不掉这三处，因为它们存在的理由是 core 要拿到并传递一个连接——这正是 [FR-141](FR-141-persistence-connection-api-boundary.md) 需求 4 的原话 |

**18 / 18 各有一条书面处置**：15 个已迁出或拆分，3 个保留并记录理由（`attention.rs`、
`process_metrics.rs`、`persistence/repository/config.rs`），三者指向同一个后继 FR-141。
core 从 200 处 / 37 文件降至 **9 处 / 3 文件**。

Phase A 的具名残余 `core/src/migration.rs`（1 处）已在 B12 处置：三个兼容包装
**全工作区零生产调用者**，作为「下线死掉的公开 API」删除。删除时立刻暴露了它掩盖的漂移
——`crate::migration::run_pending` 返回裸计数，而真正的 API 返回
`AppliedMigrationSummary`；六处断言因此补上 `.count()`。

#### B11 的发现：5 处安全护栏此前无人守护

`source.rs` 迁出后，对搬走的每条语句做变异确认，其中五条**在本次提交之前没有任何测试会失败**
（每条都实测过：变异后 core 的 96 个 `source::` 测试全绿）：

| 护栏 | 去掉它会发生什么 |
|---|---|
| `complete_routing` 的 `AND routing_state='routing'` | 迟到的 worker 覆盖另一个 worker 已提交的路由结论 |
| `defer_to_automation` 上同一条护栏（**另一条语句里的另一份拷贝**） | 没人在路由的投递被交给 automation worker，两个 worker 同时拥有它 |
| claim 的 `routing_attempts < 5` | 毒消息被无限重试 |
| `CommandActionStart::RequestMismatch` | 重放键被换了请求后**静默重启**——在为另一个请求给出的批准下执行一条没人要求的命令 |
| `INSERT OR IGNORE … == 1` | 重复投递自称是新插入，被路由两次 |

现由 `source_routing_guards_hold_the_line_they_are_there_for` 逐条钉住。
**这一批真正的产出不是搬走了 24 条语句，而是这五条护栏此前由零个测试承担。**

#### 残余 9 处，以及为什么每一处都有一个真实的后继

三个保留文件的引用形态完全相同：**持久化层把驱动类型放在公开 API 上，所以 core 手里有一个
连接**。`attention.rs` 与 `process_metrics.rs` 是 `writer()` 交出的 `tokio_rusqlite::Connection`
加两行 `fn other`；`config.rs` 是 `db::open_conn` 交出的 `rusqlite::Connection`。这是一件事，
不是三件。

它的后继是 FR-141，而且是一个**真实存在且已排好序**的后继：FR-141 的非目标里写明
「不与 FR-130 Phase B 并行……须在 Phase B 闭环之后开始」，它的需求 4 是
「`orchestrator-persistence` 的公开 API 不得出现 `rusqlite` / `tokio_rusqlite` 类型」，
它的迁移面 165 个调用点包含这三个文件的全部。Phase B 闭环正是它的前置条件。

**同时要交待一件本机制照不到的事**：Phase B 最有价值的产出不是搬走的 16 个文件，而是那些
从来没有测试守护的 SQL 不变量（B11 的五处路由护栏、B14 顺手关掉的 resume 竞态、B13–B16 四条
第一次变异就通过的断言）。它们全部是因为「有人正在搬这条语句、不得不读它」才被发现的。
**这三个保留文件的护栏没有被审计过，而这个机制永远照不到它们**——因为机制就是搬迁。
本 FR 与 DD-148 对它们的结论只涉及引用形态，不构成对其不变量的任何证据。接手 FR-141 的人
应当把这三个文件当作一次护栏审计，而不只是一次 API 迁移；那将是第一次有人带着问题去读它们的
语句。已记入 DD-148 的 Known limits。

### ✅ Phase C：`error.rs` 的驱动耦合决策（2026-07-26 闭环）

原文把它写成一个单点阻塞，需要在"引入 port 层错误类型并在边界转换"与"接受耦合并记录理由"
之间先做决定。**两个选项都预设了不存在的消费者。** 在 scratch 副本里删掉该 impl 后
`cargo check --workspace` 只报 **3 个错误，全部位于
`core/src/service/resource/delete.rs:225–230`** —— 整个工作区没有别处把
`rusqlite::Error` 转成 `OrchestratorError`。

所以处置是：先搬走那一段 SQL（Phase B1），该 impl 随即变成死代码并删除。用测量回答了
问题，而不是在两个为不存在的问题设计的方案之间做选择。

**但 impl 提供的保证必须显式保留。** 它把每个驱动错误都归类为 `ExternalDependency`，
而这个 category 在 gRPC/CLI 契约上，不是内部细节。B1 的调用点最初用了
`classify_resource_error`，那是按消息文本分类的——而 SQLite 说"缺表"的措辞是
`no such table: resources`，会被它的 `not found` 分支读成 `NotFound`。同一个故障、
不同的 category、且不会有编译错误。现在调用点用一个显式 `external_dependency` 的命名函数，
并由 `phase_c_preserves_the_external_dependency_category` 以**真实未迁移库产出的真实错误**
钉住；把该函数改回 `classify_resource_error` 会让测试以
`left: NotFound, right: ExternalDependency` 失败（已实测）。

### ❌ 原需求 4：跨边界的 `too_many_arguments` — 不适用，已关闭

原文据"43 个豁免"推出本条。实测 core 只有 **3 个**，工作区的 43 个集中在 scheduler(21)、
gui(7)、daemon(7)。该条对本次提取近乎空转，作为 FR-130 的需求已关闭。scheduler 的 21 个
豁免若值得治理，应作为独立议题立项，不挂在持久化提取之下。

## 验收标准

### 已达成

- [x] `core` 的 `pub mod` 数与公开项数相对基线精确冻结，门禁存在且有负向 fixture
- [x] 提取前完整迁移链产出的 schema 已以可复现脚本记录并冻结（46 表 + 92 索引）
- [x] 迁移幂等性与断点续跑回归通过（全部 **37** 个中断点；治理期曾记作 74，已更正）
- [x] 跨边界的 `too_many_arguments` 已书面记录（core 实为 3 个，见 DD-142）
- [x] 对外 gRPC/CLI 契约无变化，既有集成测试全绿且未修改断言

### 前置

- [x] FR-134 需求 9 已闭环，且修复后 `core-boundary-ledger.json` 的 `200 / 37` 与
      `52 / 924 / 143` 未变化（变化项须逐条说明）—— 2026-07-26 在 `6aeb2ce` 重跑
      `ruby scripts/qa/core-boundary.rb`：`143 files, 52 pub mod, 924 public items;
      200 rusqlite reference(s) across 37 file(s)`，无变化项。尺子先于测量，成立
- [x] FR-136 已闭环，收口形态已选定 —— 分层收口，线画在 `agent_orchestrator.db` 上：
      core／`orchestrator-persistence` 为持久化层，`orchestrator-scheduler` 与 `daemon`
      禁止直接持有驱动（`task_state.rs` 在被禁止的一侧），`orchestrator-security` 因位于
      core 之下而书面豁免，`slack-gateway` 因自有数据库而不在范围内。Phase A 的迁出目标由
      `config/governance/persistence-dependency-ledger.json` 的逐文件残量给出；两个 forbidden
      crate 的 `residualDeclaration` 在残量清零后摘除，届时其 `Cargo.toml` 中的驱动声明本身
      开始失败。跨 crate 事务接口无需设计：两者合计 0 处显式事务，实为多语句工作单元。
      见 [DD-147](../design_doc/orchestrator/147-persistence-dependency-chokepoint.md)

### Phase A —— 已闭环（2026-07-26），其设计与验证由
[DD-148](../design_doc/orchestrator/148-persistence-crate-extraction.md) 与
[QA 186](../qa/orchestrator/186-persistence-crate-extraction.md) 承载

- [x] `orchestrator-persistence` crate 存在，承载迁移内核与 repository 实现，`#![deny(missing_docs)]` 通过。
      "存在"不以 `ls` 或成员清单为证——把 core 的依赖行**注释掉**（而非删除）后
      `cargo check -p agent-orchestrator` 必须失败，QA 186 场景 1
- [x] 台账中属 Phase A 范围的引用已从 core 收敛：**115 处收敛 114 处**。
      core 从 143 文件 / 52 `pub mod` / 924 公开项 / 200 处引用降至
      129 / 50 / 665 / 86（20 个文件）。
      **残余 1 处**：`core/src/migration.rs` 的 `use rusqlite::Connection`。该文件是
      `persistence::migration` 的三个"兼容"包装，而兼容对象不存在——core 之外无人引用
      `agent_orchestrator::migration`，core 内唯一调用者是 `action_audit.rs` 的测试模块。
      收敛它意味着要么下线死掉的公开 API（是决策不是搬运），要么在 persistence crate 里
      加一个只为把计数推到 0 的 `run_pending_count`（比它消除的残余更糟）。归入 Phase B 的逐文件处置
- [x] `cargo test -p agent-orchestrator schema_snapshot` 通过，`schema-snapshot.sql`
      在四个 commit 中逐次字节不变
- [x] 逐对象比对：中断点扫描现在把**自身覆盖范围**与数据库记录的 `schema_migrations`
      行数对比。`for i in 1..=total` 读起来像穷举，正是问题所在——为提速插入的 `step_by`
      会让它在覆盖五分之一链条的情况下静默通过
- [x] 至少一条**依赖旧路径的端到端行为**仍成立：
      `crates/orchestrator-persistence/tests/round_trip.rs` 引导真实数据库跑完整链条，
      再让一个任务穿过每个被搬动的模块，且**每次写入都从另一个模块读回**；
      配对的负向用例针对未迁移的数据库，要求报错而非返回"看似合理的空"。
      另有 core 侧保留的测试从领域侧（`create_task_impl` + `TestState`）驱动同一条路径
- [x] Phase A 的提取 commit 可机械回退：在钉在 `524ed26b` 的 scratch worktree 中
      **逐个具名**（而非用 `A1^..A4` 区间）revert 四个提交，44 个路径无冲突，
      `cargo check --workspace` 通过，两个门禁回到 `143 / 52 / 924`、`200 / 37`、
      `13 members`——台账与代码一同回退。用区间会连带 revert 期间落入的一个无关提交，
      那证明的是"某组提交可回退"而非"本次提取可回退"（首次执行即犯了这个错，44 与 45 之差）

### Phase B —— 已闭环（2026-07-27）

- [x] 18 个混装文件各有"已迁出 / 已拆分 / 保留并记录理由"的结论 —— **18 / 18**：
      15 个已迁出或拆分，3 个保留并记录理由，逐文件结论见上面的处置表。
      三个保留文件指向同一个后继 FR-141，且该后继存在并以 Phase B 闭环为其前置条件
- [x] 每批独立提交且各自可回退；每批之后 `schema-snapshot.sql` 未变 —— B0–B16,
      每批一次提交、一次 QA、一次**逐个 commit 具名**（不用 range）的回退证明，
      每次都重跑 `cargo test --workspace`、clippy `-D warnings`、`schema_snapshot`、
      以及未经修改的 `phase_c_preserves_the_external_dependency_category`，
      且两个治理台账与代码同 commit 重新冻结
- [x] 台账 `rusqlite.files` 随每批单调收敛，残余点显式清单化 —— 86 → 9，20 → 3 文件；
      残余 3 个文件逐一列在处置表中，并由 `core-boundary.rb` 以精确相等双向冻结
- [x] **每条被搬动的语句都有行为断言，而不只是引用消失**：
      `delete_project_resources`（按项目删、别的项目不受影响、二次调用返回 0）、
      `list_terminal_tasks_older_than`（三种排除原因各一个 fixture、LIMIT、窗口放宽后为空）、
      `step_event_rows`（只返回被请求的类型，空清单返回空而非全部）、
      删除守卫与事件解释两半各自可无数据库单测

### Phase C —— 已闭环

- [x] `error.rs` 的驱动耦合已处置 —— `impl From<rusqlite::Error> for OrchestratorError`
      已删除。不是原文的两个选项之一：实测它只有一个消费者（B1 搬走的那段 SQL），
      删掉后即为死代码。它保证的 `ExternalDependency` category 由调用点的具名映射函数
      显式承接，并有反向变异实测
- [x] core 对 `rusqlite` 的引用收敛至 0，或残余点带理由记录并被门禁冻结 ——
      **以第二条分支达成**：残余 9 处 / 3 文件，逐一具名、各带书面理由、由
      `core-boundary.rb` 以精确相等在两个方向上冻结。不取第一条分支是一次判断而非放弃：
      这 9 处是**持久化层公开 API 上的驱动连接类型**，清掉它们要改的是那个 API 而不是这三个
      文件，规模是跨三个 crate 的 165 个调用点，判据在 API 边界上而不在棘轮上。
      本 FR 若为了让计数归零而顺手改那个 API，就正好做了它在 Phase B 开头明确拒绝过的那件事

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

## 闭环判断（2026-07-27）

**判断：本 FR 可以闭环。**

判据不是「三个 phase 都做完了」，也不是「还有残余所以不能关」——两个默认答案都有惯性。
沿用上一轮定下的那条：**「保留并记录理由」与「被阻塞」是合法的闭环状态，因为各自带着一个决策
和一个后继；没有决策的不是。** 于是问题变成：每一条残余是否都有一个真实存在的后继。

三条残余（`attention.rs`、`process_metrics.rs`、`persistence/repository/config.rs`，各 3 处）
指向同一个后继 FR-141。它存在、已立项、需求 4 就是这三处引用的形态本身、迁移面包含它们的
全部调用点，而且它的非目标写明须在 Phase B 闭环之后开始——所以这不只是「有后继」，
是**本 FR 闭环恰好是那个后继的前置条件**。

第三条残余原本是唯一一个后继不存在的：它记作「被阻塞，解锁条件是 crd 先下沉」，而全仓只有
本 FR 提到 crd，本 FR 一关它就无主。收尾时重新推导发现**那个解锁条件是错的**——它回答的是
Phase A 的问题（整个文件能否下沉，答案是会成环），而 Phase B 问的是语句能否下沉，
那些语句没有一条需要 crd 类型。所以不需要立一个 FR 承接 crd 下沉：没有东西需要被承接。
详见处置表该行与 DD-148。

反过来说一条没有被后继覆盖的：**这三个文件的 SQL 护栏从未被审计**，而做审计的那个机制
（搬迁时不得不逐条读语句）永远照不到它们。这一条已写进 DD-148 的 Known limits 并在上面
点名，交给 FR-141 一并处理。它不阻塞本 FR 闭环——它是一件被明确记下来的未做之事，
不是一件被默认已做的事。

**闭环手续尚未执行**（删除本文件、更新 `docs/feature_request/README.md`、重生成
`doc-lifecycle-index.json`）。判断与依据先落在这里，删除是一次独立的动作。
