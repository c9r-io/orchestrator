# FR-142: 触发历史上限从未生效 — 级联删除的决策与修复

## 优先级: P1

## 状态: Proposed

## 背景

`Trigger` 的 `history_limit` 是一个对外声明的配置项。**它从未生效过。**

FR-130 Phase B16 把 `trigger_state` 表搬出 core 时暴露了这一点，已记入
[DD-148](../design_doc/orchestrator/148-persistence-crate-extraction.md) 的 Known limits
并由一条断言钉住。以下事实于 2026-07-27 逐条复核：

### 删除被外键拒绝

`crates/orchestrator-persistence/src/trigger_state.rs:212` 执行的是裸删除：

```rust
&format!("DELETE FROM tasks WHERE id IN ({placeholders})"),
```

而冻结的 schema 基线（`config/governance/schema-snapshot.sql`）中：

```sql
CREATE TABLE task_items ( ..., FOREIGN KEY(task_id) REFERENCES tasks(id) );
CREATE TABLE command_runs ( ..., FOREIGN KEY(task_item_id) REFERENCES task_items(id) );
```

**没有 `ON DELETE CASCADE`。** 全库仅有 2 处 `ON DELETE CASCADE`，都不在这条链上。
共有 **10 张表**引用 `tasks(id)`：`task_items`、`command_runs`（经 `task_items`）、
`handoff_snapshots`、`resume_executions`、`resume_plans`、`source_automation_routes`、
`source_bindings`、`source_events`、`source_routing_attempts`、`task_graph_runs`、
`task_graph_snapshots`。

只要目标 task 在其中任何一张表里有子行，删除即被拒绝。

### 而且没有任何症状

`core/src/trigger_engine.rs:648`：

```rust
if trigger.history_limit.is_some()
    && let Err(e) = cleanup_history(...).await
{
    debug!(trigger = trigger_name, error = %e, "history cleanup failed");
}
```

`crates/daemon/src/main.rs:209` 的默认日志级别是 `info`。**`debug!` 在默认部署下不输出**——
所以症状不是"只有一行日志"，而是**一行日志都没有**。表只是永远不缩。

对照同一函数往上两行，入队失败用的是 `error!`：

```rust
error!(task_id = ..., error = %e, "failed to enqueue triggered task");
```

同一个函数里，两个可比的失败取了不同的严重级别，而被降级的那个恰好是无声失效的那个。

### 正确的级联已经存在，只是这条路径没走它

`crates/orchestrator-persistence/src/task_repository/items.rs:88`：

```rust
let tx = conn.unchecked_transaction()?;
tx.execute("DELETE FROM events WHERE task_id = ?1", ...)?;
tx.execute("DELETE FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = ?1)", ...)?;
tx.execute("DELETE FROM task_items WHERE task_id = ?1", ...)?;
tx.execute("DELETE FROM tasks WHERE id = ?1", ...)?;
tx.commit()?;
```

`task_cleanup.rs` 走的就是它。所以这不是"缺一个实现"，是**两条删除路径中只有一条是对的**，
而触发历史走了另一条。

## 需要先作出的决策

**本 FR 的核心不是补一个 `DELETE`，而是回答：历史上限有权删掉多少东西？**

`history_limit` 的语义是"只保留最近 N 次触发产生的 task"。但一个 task 牵着它的 items、
命令运行记录、事件、以及可能的 handoff 快照与 resume 计划。级联删除意味着：

- **命令运行记录连同其 `stdout_path` / `stderr_path` 一起消失**，而磁盘上的日志文件不在
  数据库里——它们会变成无主文件，还是应当一并删除？`task_cleanup.rs` 的级联返回
  `log_paths` 正是为此。
- **`resume_plans` / `resume_executions` / `handoff_snapshots`** 承载的是跨会话的交接状态。
  一个仍可被 resume 的 task 因为触发历史上限被删掉，语义上是否可接受？
- **`source_events` / `source_routing_attempts`** 是投递审计。删掉它们等于删掉"这条消息被
  如何处理过"的记录。

这三类的答案可以不同。可能的形态至少有三种，**须书面选定并说明理由**：

- **A：全级联** —— 历史上限删除 task 及其全部子行，复用 `items.rs` 已有的事务级联并扩展到
  其余表，日志文件按 `task_cleanup.rs` 的既有做法处置。
- **B：受限级联** —— 只删除执行痕迹（items、command_runs、events），保留审计与交接状态，
  并为它们各自定义独立的保留策略。此形态要求那些表能容忍孤儿外键，须逐表确认。
- **C：只删可删的** —— 历史上限只删除没有子行的 task，其余跳过并**如实报告跳过的数量**。
  这是最小改动，但它承认 `history_limit` 对真正跑过的 task 不适用，那必须写进用户文档，
  否则配置项仍在说一句假话。

## 目标

- 让 `history_limit` 要么按其声明的语义生效，要么其真实语义被如实记录在用户文档中。
- 让这条路径的失败可见。

## 非目标

- **不**改变 `history_limit` 的配置语法或默认值。
- **不**顺带给全库补 `ON DELETE CASCADE`。那是一次 schema 迁移，会改变
  `config/governance/schema-snapshot.sql`，涉及 10 张表的删除语义，且影响的远不止触发历史。
  若决策指向它，须作为独立议题立项并按 FR-130 建立的 schema 基线流程处理。
- **不**改变 `task_cleanup.rs` 现有的级联行为。它是对的，本 FR 参照它而非重写它。

## 需求

### 1. 确定失效的真实范围

- 以实测确定：哪些 task 会被拒绝删除。**不要从"每个 task 都有 items"推断**——须验证
  触发创建的 task 是否可能为零 items，以及其余 9 张引用表各自在什么条件下有行。
- 结论决定 `history_limit` 至今的实际行为是"完全无效"还是"仅对未执行的 task 有效"，
  两者在用户文档里该写的话不同。

### 2. 决策并落地

- 在 A / B / C 中选定一种，书面记录理由与被放弃选项的代价。
- 复用 `task_repository/items.rs` 的事务级联，不新写一份。**两条删除路径中已经有一条是对的,
  本 FR 不应产生第三条。**
- 日志文件的处置随之确定（级联返回 `log_paths` 的既有约定）。

### 3. 失败必须可见

- `trigger_engine.rs:652` 的 `debug!` 至少提升为 `warn!`。**同一函数上方的入队失败用
  `error!`，本条不应更低。**
- 更强的形态：历史清理的结果（删除数、跳过数、失败原因）应可被观测，而不是只在出错时
  才出现一行。一个声称"保留最近 N 个"的机制，其实际保留数应当是可查的。

### 4. 行为断言

- 断言必须是**行为的**而非结构的：造出一个带 items、command_runs 与 events 的 task，
  触发历史上限，断言该 task 及按决策应删的子行确实消失、不应删的确实留下。
- **反向断言**：删除被拒绝的情形（若决策为 C 则仍然存在）必须被报告而不是被吞掉。
- DD-148 已有的那条钉住现状的断言须随本 FR 更新，且**不得直接删除**——它记录的是一个
  真实存在过的缺陷。

## 验收标准

- [ ] 失效的真实范围已实测确定并记录（完全无效 / 仅对未执行 task 有效）
- [ ] A / B / C 已选定，理由与被放弃选项的代价已书面记录
- [ ] 级联复用 `task_repository/items.rs`，未产生第三条删除路径
- [ ] 行为断言：带子行的 task 经历史上限后按决策被正确删除或正确跳过
- [ ] 反向断言：被拒绝/被跳过的情形被报告，不再无声
- [ ] `debug!` 已提升，且失败在默认日志级别下可见
- [ ] `config/governance/schema-snapshot.sql` **未变**（若决策要求改 schema，则本项改为
      "schema 变更已按 FR-130 建立的基线流程评审并记录"）
- [ ] 用户文档中 `history_limit` 的描述与实际行为一致
- [ ] `cargo test --workspace`、strict Clippy、全部既有 CI job 状态不因本 FR 变化

## QA 计划

- **先复现再修**。当前状态可复现：造一个带 items 的 task，调用历史清理，观察它被外键拒绝
  且默认日志级别下毫无输出。这个复现本身就是本 FR 的第一条证据。
- **行为断言，不是计数断言**。"`DELETE` 返回 Ok" 不是证据——原缺陷正是在 `Err` 被吞掉的
  情况下发生的。断言应当查询数据库确认行确实消失或确实留下。
- **决策的代价要有测试佐证**。若选 B（受限级联），须有断言证明被保留的表能容忍其外键指向
  一个已删除的 task；若选 C，须有断言证明跳过被如实报告。
- **不需要新的 CI job**。若新增门禁，记得往 `governance` job 的 `OUTCOMES` 加行——FR-137
  未闭环，没有任何东西替你守这一步。

## 附注

这条缺陷是 FR-130 Phase B 搬迁时被人眼发现的，**不是任何门禁报出来的**。仓库现有 62 道
门禁没有一道能看见它：它不是引用计数、不是公开面、不是文档陈述、也不是 CI 接线，
而是一条被静默吞掉的运行时错误。记在这里作为 FR-140 讨论治理成本时的一个对照点。
