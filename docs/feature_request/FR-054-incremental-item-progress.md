# FR-054: Item 进度增量更新 — finalize_items 延迟导致 Progress 长时间为零

- **Priority**: P1
- **Status**: Open
- **Created**: 2026-03-16

---

## 1. 问题描述

在 full-qa workflow 中创建 132 个 item 的任务后，`orchestrator task info` 的 `Progress: 0/132` 在整个执行过程中始终为零，即使已有 21+ 个 qa_testing run 以 exit=0 完成。用户无法获知实际进度，直到**所有 item 完成所有 segment** 后 `finalize_items` 批量写入终态状态。

## 2. 根因分析

### 2.1 Progress 计算方式

`core/src/task_repository/queries.rs:159-172` — `load_task_item_counts()`:

```sql
SELECT COUNT(*),
       SUM(CASE WHEN status IN ('qa_passed','fixed','verified','skipped','unresolved') THEN 1 ELSE 0 END),
       SUM(CASE WHEN status IN ('qa_failed','unresolved') THEN 1 ELSE 0 END)
FROM task_items WHERE task_id = ?1
```

`finished_items`（第 2 列）仅统计终态 status。item 在执行期间始终保持 `running` 状态。

### 2.2 终态写入时机

`crates/orchestrator-scheduler/src/scheduler/loop_engine/mod.rs:559`:

```rust
segment::finalize_items(state, task_id, task_ctx, &items, &mut item_state).await?;
```

`finalize_items` 在 **所有 segment 的循环结束之后** 才被调用。对于 full-qa workflow，segment 结构为：

1. **Segment 0** — `qa_testing`（item scope, max_parallel=4）→ 处理全部 132 个 item
2. **Segment 1** — `doc_governance`（task scope）→ 单次执行

Segment 0 需要处理全部 132 个 item（每个 3-5 分钟，4 并行，预计 ~100+ 分钟），Segment 1 执行后才到 `finalize_items`。在此期间所有 item 的 DB status 保持 `running`，progress counter 返回 0。

### 2.3 影响

- 长时间运行的任务（>30 分钟）无法提供有意义的进度反馈
- 用户无法判断任务是否正常推进还是卡死
- 监控脚本（如 `scripts/run-full-qa.sh`）无法展示真实进度

## 3. 方案：增量 finalize + step 级进度展示

两层修改互补：数据层让 item 终态及时写入 DB，展示层让用户在 finalize 之前也能看到 step 级中间进度。

### 3.1 数据层 — Item 级增量 finalize

在 `execute_item_segment` 的并行路径中，当单个 item 完成当前 segment 的所有 step 后，若该 item 后续无更多 item-scope segment，立即调用 `finalize_item_execution` 写入终态（`qa_passed`/`unresolved` 等），使 `Progress: X/132` 实时递增。

**关键修改点**：
- `segment.rs` — `execute_item_segment` 并行路径（line 316+）：每个 item 的 spawn task 完成后，判断是否为最后一个 item-scope segment（可复用 `is_last_item_segment()`），若是则在 spawn task 内部直接调用 `finalize_item_execution`
- `loop_engine/mod.rs:559` — `finalize_items` 仍保留作为兜底，对已 finalize 的 item 跳过（检查 `acc.terminal` 或 item DB status 已为终态）

### 3.2 展示层 — Step 级进度统计

在 `task info` 输出中增加 step 粒度的 run 统计，基于 `runs` 表聚合。即使增量 finalize 尚未触发，用户也能看到各 step 的完成数：

```
Progress: 12/132 items
  qa_testing:     21/132 completed, 4 running
  doc_governance:  0/1   completed
```

**关键修改点**：
- `crates/cli/src/output/task_detail.rs:31-33` — 在 `Progress` 行下方增加 step 级统计
- gRPC `TaskInfoResponse` 或 CLI 层直接查询 `runs` 表按 `phase` 分组聚合 `exit_code`

## 4. 涉及文件

| 文件 | 角色 |
|------|------|
| `core/src/task_repository/queries.rs:159-172` | Progress 计算 SQL |
| `crates/orchestrator-scheduler/src/scheduler/loop_engine/mod.rs:559` | finalize_items 调用点 |
| `crates/orchestrator-scheduler/src/scheduler/loop_engine/segment.rs:205-490` | item segment 执行与 finalize |
| `crates/orchestrator-scheduler/src/scheduler/item_executor/finalize.rs:12-87` | 单 item finalize 逻辑 |
| `crates/cli/src/output/task_detail.rs:31-33` | Progress 展示 |

## 5. 验证方式

1. 创建 full-qa 任务（132 items）
2. 每 30 秒检查 `orchestrator task info`
3. 验证数据层：第一批 item 完成 qa_testing 后，`Progress: X/132` 应 > 0
4. 验证展示层：即使 `Progress` 尚未递增，step 级统计应显示 `qa_testing: N/132 completed`
5. 不应出现长时间 `Progress: 0/132` 且无任何中间进度信息的情况
