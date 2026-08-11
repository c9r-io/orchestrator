# FR-168: Task 删除的引用处置策略 —— 级联清 1/8，其余七表无裁决

## 优先级: P1

## 状态: Proposed

## 背景

计数 at `403f87a2`，方法注明；治理时 step 0 重建。

来源：`docs/ticket/20260811-task-delete-fk-cascade-incomplete.md`（2026-08-11
产品分析的已声明债务清扫）。该缺陷由 DD-150 **自报**并明确不修
（`docs/design_doc/orchestrator/150-trigger-history-limit-cascade.md:228`）：

> **`task_cleanup.rs` 与 `delete_task_impl` 有同一缺陷，本 FR 不修复它。**
> 二者都经由 `items.rs`，它清除八个阻塞引用中的一个。任何用过 handoff、
> resume plan 或 source ingest 的任务都无法被 retention 清扫或
> `orchestrator task delete` 移除；删除以 `FOREIGN KEY constraint failed` 失败。

### 引用面（schema 派生，非手列）

方法：把 `config/governance/schema-snapshot.sql` 的 `CREATE TABLE` 语句载入
sqlite，以 `sqlite_master` join `pragma_foreign_key_list`，筛 `"table"='tasks'`。
at `403f87a2`：

- **10** 张表引用 `tasks(id)`。
- **2** 张声明 `ON DELETE CASCADE`（`task_graph_runs`、`task_graph_snapshots`），
  由 SQLite 自行清除，从不拒绝任何删除。
- **8** 张为 `NO ACTION`。`delete_task_and_collect_log_paths`
  （`crates/orchestrator-persistence/src/task_repository/items.rs:56-102`）
  清除其中恰好 **1** 张——`task_items`，连带挂在其下的 `command_runs`，以及
  **完全没有外键**的 `events` 行。
- 余下 **7** 张阻塞：`handoff_snapshots.task_id`、`resume_plans.task_id`、
  `resume_executions.child_task_id`、`source_bindings.task_id`、
  `source_events.routed_task_id`、`source_routing_attempts.task_id`、
  `source_automation_routes.task_id`。

复现（不依赖 daemon）：schema-snapshot 库中插入一行 `tasks` 与一行
`handoff_snapshots`，逐字运行 `items.rs` 的四条 `DELETE`，在
`PRAGMA foreign_keys = ON` 下于 `DELETE FROM tasks` 处失败，
`FOREIGN KEY constraint failed (19)`。该 pragma 非假设：
`crates/orchestrator-persistence/src/async_database.rs:115` 在启动时断言其为 `1`。

七张表**全部**有生产写入路径（`grep INSERT` 覆盖 `crates/` 与 `core/`、排除
`cfg(test)` 模块，at `403f87a2`）。其中 `source_bindings` 需跨行匹配才能找到：
`create_binding_blocking`，`crates/orchestrator-persistence/src/source_events.rs:694`。
故缺口经由七张表**每一张**均可达，不止 ticket 点名的三条路径。

### 为何这是设计缺口而非实现缺陷

- `docs/design_doc/orchestrator/85-data-lifecycle-governance.md:22` 把级联范围
  定为"items, runs, events, log files"。`items.rs` 实现的正是这个集合。实现
  **符合**设计。
- FR-142 非目标 3 显式排除了扩展级联。DD-150 记录了理由：扩展"是一个爆炸
  半径更大的独立裁决——它改变操作员显式 `task delete` 所摧毁的东西"。
- DD-150:215：**"七张表对『retention 是否可以摧毁这个』有实质不同的答案。"**
  确实如此，且方向相反：
  - `handoff_snapshots`、`resume_plans` 由任务拥有，随任务删除是自然读法；
  - `source_events.routed_task_id`、`source_routing_attempts` 是**入站审计**——
    因为任务被删就销毁它们，等于丢失"该事件曾经到达"的记录；
  - `source_automation_routes` 携带 `UNIQUE deterministic_task_id`，删除它可能
    让一次重放重新触发（幂等键消失）；
  - `resume_executions.child_task_id` 指向的是**另一个**任务，不是被删的那个。

  没有任何设计文档裁决其中任何一条。这是本 FR 要产出的东西。
- QA 为何未捕获：`docs/qa/orchestrator/01-cli-agent-orchestration.md:313` 删除
  的是刚创建的 `--no-start` 任务，七张表一行都没有。

### 已有的、必须复用而非重写的机制

FR-142/DD-150 已经建成本 ticket 所要求的 schema 派生枚举，**不要再造一个**：

- `blocking_references()`（`crates/orchestrator-persistence/src/trigger_state.rs:230-244`）
  在运行时从 `pragma_foreign_key_list` 读出阻塞集，排除 CASCADE 与
  `task_items`。其文档注释已写明理由：手写的七表清单"今天正确，下一次有人
  加表时就静默少一张"——即 §4.4 shape 2。
- `references_holding()`（同文件 `:248-268`）把该集合化为 `table.column` 归因。

新增第二份枚举正是这份注释所要防止的错误。

## 需求

### 1. 归因先于策略（不依赖任何裁决，可独立治理）

`delete_task_impl`（`crates/orchestrator-scheduler/src/scheduler/query/task_queries.rs:59`）
把 rusqlite 的原始错误经 `classify_task_error` 直接上抛，操作员看到的是
`FOREIGN KEY constraint failed` 与零个表名——而 `delete_tasks_within_history_limit`
在一个 crate 之外已经算出了这个归因并写进 `warn!` 日志。

要求：被阻塞的 `task delete` 必须点名扣住该任务的引用，复用既有
`blocking_references()` / `references_holding()`。

该需求不改变任何删除语义（今天失败的，之后仍然失败），只改变失败**说了什么**，
故不被需求 2 的裁决阻塞。与 FR-167 切分需求 1/3 与需求 2 的理由同形：不让一个
需要跨面裁决的条目拖住一个纯粹的可观测性修复。

### 2. 七表逐一裁决处置方式

对七张表各选一种并**记录理由**：

| 处置 | 含义 |
|---|---|
| delete-with-task | 随任务删除（该行由任务拥有） |
| null-the-reference | 置空外键列，保留行（该行是独立审计事实） |
| block-and-report | 拒绝删除并点名（该行的存在本身是拒绝理由） |

`null-the-reference` 对 `source_events.routed_task_id`、
`resume_executions.child_task_id` 是可行的（两列均可空）；
`source_bindings.task_id`、`source_routing_attempts.task_id` 等列的可空性须在
治理时从 schema 重新派生，不得据本表推定——**本项未核验**。

裁决须同时回答：`--force` 是否改变处置（今天 `--force` 只是确认闸门，不改变
级联范围），以及 retention 清扫与显式 delete 是否可以有不同答案（DD-150 的
skip-whole 语义对 retention 已裁决为"跳过并具名"，显式 delete 未裁决）。

### 3. 两条路径共用一个清除例程

`core/src/task_cleanup.rs` 与 `delete_task_impl` 必须共用需求 2 产出的处置
例程，不得各写一份。DD-150 已出于同一理由让 history 清扫经由 repository 级联
（"有一条移除任务的语句序列，而不是两条"）；本 FR 不得引入第三条。

### 4. 孤儿 `events` 的闭包断言

DD-150:244 记录：`events` 没有外键，"任何忘记 `events` 的删除路径都会留下
schema 中没有任何东西能捕获的孤儿行"。要求断言该闭包性质——删除任一任务后
不存在 `task_id` 指向已删任务的 `events` 行——而非断言某条 `DELETE` 语句的
拼写（§4.4 shape 1）。

## 验收标准

复现步骤取自 ticket，逐条转为断言：

- [ ] 创建任务 → 附加 handoff（`orchestrator handoff ...`）→ `task delete --force`：
      **不得**以裸 `FOREIGN KEY constraint failed` 失败。按需求 2 的裁决，或成功，
      或以点名 `handoff_snapshots.task_id` 的诊断拒绝
- [ ] 同上，对 resume plan 与 source ingest 各一条（ticket 点名的三条路径）
- [ ] 需求 1 的断言**断言诊断字符串而非退出码**——退出码无法区分"被引用扣住"
      与任何其他失败（§4.4 shape 7 的第二条实践）
- [ ] 阻塞表集合由 `blocking_references()` 在运行时派生；负夹具：新增一张引用
      `tasks(id)` 且无 CASCADE 的表后，该表**自动**出现在诊断中而无需改动任何
      清单。这条是本 FR 与手列清单的分界线
- [ ] retention 清扫（`cleanup_old_tasks`）与显式 delete 在同一夹具上给出需求 2
      所裁决的结果，且二者经由同一例程（断言行为，非断言调用图）
- [ ] 删除后不存在指向已删任务的 `events` 行（需求 4 的闭包断言）
- [ ] DD-150 既有断言不回归：`docs/qa/orchestrator/188-trigger-history-limit-cascade.md`
      的场景 1–4 全绿，特别是场景 2（被 `resume_plans` 扣住的任务被跳过、具名、
      且**每一行都完整保留**——不得被剥去 items 后留下）

## 依赖与关联

- 直接承接 FR-142 / DD-150 / QA 188：本 FR 关闭的正是 DD-150 Known limits 第
  一条自报且知情不修的缺陷。DD-150 对 FR-142"该路径是正确的"这一表述的撤回，
  在本 FR 闭环时应一并更新。
- `docs/design_doc/orchestrator/85-data-lifecycle-governance.md` 的级联范围
  （"items, runs, events, log files"）在需求 2 裁决后须改写——这是本 FR 修改
  设计而非修改实现的地方。
- QA 侧已记已知限制两处，闭环时清理：
  `docs/qa/orchestrator/01-cli-agent-orchestration.md`（显式 delete 场景）与
  `docs/qa/orchestrator/188-trigger-history-limit-cascade.md`（retention 场景）。

## 未核验项（明确标注）

- **七张表中哪几张对真实任务实际会积累行，未测量。** DD-150 自己记录了这一点
  （"没有测试断言七者中哪些对真实 trigger 任务会发生"）。本 FR 只核验了七张表
  **各有生产写入路径**（grep INSERT，排除 cfg(test)），这不等于某条具体的用户
  流程会写入它们。需求 2 的裁决成本与此强相关，治理时 step 0 应先测量。
- **各阻塞列的可空性未逐列派生**，故 `null-the-reference` 对哪几张表可行尚未
  确定；需求 2 表格中的两个例子取自 schema-snapshot 的目视读取，单方法未核验。
- **`--force` 与处置的关系无先例可循。** `docs/qa/orchestrator/43-cli-force-gate-audit.md`
  把 `task delete <id>` 记为"删除任务 + 停止运行时"，未涉及引用处置。
- **本类缺陷是否还有第二处实例未清点**：其他以 `tasks(id)` 之外的表为中心的
  删除路径（project、workspace、agent 的删除）是否也只清除其阻塞引用的子集，
  未做全面清点。DD-150 的 `blocking_references()` 是按 `tasks` 硬编码的，其形状
  可否泛化到其他父表，是一个独立于本 FR 的问题。
