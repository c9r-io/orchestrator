# Design Doc 65: Parallel Dispatch Completeness Guard (FR-053)

**关联 FR**: FR-053
**前置**: FR-037 (Design Doc 49), FR-038 (Design Doc 50)
**状态**: Implemented
**日期**: 2026-03-16

---

## 1. 问题

使用 `full-qa` workflow 执行 131 个 QA 文档的全量回归测试时，`execute_item_segment` 的并行路径 for-loop 在仅分发 5 个 item 后提前退出，导致 `execute_cycle_segments` 返回、cycle loop 进入下一轮迭代并触发 `max_cycles_enforced`，最终 task 仅完成 1/131 items。

### 复现路径

```
full-qa workflow, 131 items, max_parallel=4, max_cycles=1, mode=fixed
  → cycle 1 started
  → items 1-4 spawned (semaphore permits), item 4 完成, item 5 获得 permit
  → 28 秒后 for-loop 提前退出（根因未确认）
  → execute_cycle_segments 返回 Ok(())
  → cycle loop 迭代, max_cycles_enforced 触发 (current_cycle=1 >= proactive_max=1)
  → items_compensated → task_completed (1/131 items)
```

### 根因候选

| 候选 | 描述 | 已确认? |
|------|------|---------|
| Tokio task 取消 | daemon self-restart exec() 中断 worker | 否 |
| SQLite busy timeout | 4 个并发 JoinSet task 的 heartbeat/event 写入竞争 | 否 |
| Semaphore 异常关闭 | JoinSet panic 导致 semaphore drop | 否 |
| Daemon 进程替换 | cargo build 触发 file-watcher self-restart | 否 |

根因调查延后至本 FR 闭环后，按需开启后续 FR。

---

## 2. 设计方案

### 2.1 方案概览

在 `execute_item_segment` 的并行路径中引入 **dispatch 完成性计数器**：跟踪 for-loop 实际向 JoinSet 提交的 task 数量，若少于预期 item 数，显式 bail 而非静默成功。

### 2.2 实现

```rust
// segment.rs — parallel path
let mut dispatched_count: usize = 0;         // line 323

for item in items {
    // ... ensure_item_isolation, acquire_owned, clone ...
    join_set.spawn(async move { ... });
    dispatched_count += 1;                    // line 365
}

// collect JoinSet results ...

// FR-053: Completeness check
let expected = items.len();
if dispatched_count < expected {
    warn!(dispatched_count, expected, "FR-053 completeness check failed");
    insert_event(state, task_id, None, "parallel_dispatch_incomplete",
        json!({"dispatched": dispatched_count, "expected": expected}));
    anyhow::bail!("parallel item segment incomplete: dispatched {}/{} items",
        dispatched_count, expected);
}
```

### 2.3 错误传播路径

```
bail!() at segment.rs:416
  → execute_item_segment() returns Err
    → execute_cycle_segments() propagates via ?
      → run_task_loop_core() breaks cycle loop
        → post-loop error handling
```

关键效果：`max_cycles_enforced` 检查位于 cycle loop 顶部（`mod.rs:156-183`），bail 导致 loop break，因此 `max_cycles_enforced` **不会被触发**。

### 2.4 范围界定

| 属于本 FR | 不属于本 FR |
|----------|------------|
| 检测 dispatch 不完整 | 修复 for-loop 提前退出的根因 |
| 显式 bail + 事件发射 | Cycle completion barrier (Plan B) |
| 防止静默数据丢失 | SQLite retry/backoff (Plan C) |

---

## 3. 关键变更

| 文件 | 变更 |
|------|------|
| `crates/orchestrator-scheduler/src/scheduler/loop_engine/segment.rs` | 新增 `dispatched_count` 计数器（line 323）、increment（line 365）、completeness check（lines 393-417） |

变更为纯增量（23 行），不修改任何现有逻辑。

---

## 4. 行为矩阵

| 场景 | dispatched_count vs expected | 行为 |
|------|------------------------------|------|
| 所有 items 正常分发 | 相等 | 正常完成，无额外事件 |
| for-loop 中途退出（任何原因） | dispatched < expected | `parallel_dispatch_incomplete` 事件 + bail |
| items 列表为空 | 0 == 0 | 正常完成（不进入 parallel path） |

---

## 5. 后续

若 `full-qa` workflow 重新运行时 completeness check 触发（即 for-loop 仍然提前退出），应开启新 FR 进行根因调查并实施 Plan B 或 Plan C。若 workflow 正常完成，说明根因可能为瞬态条件（如 daemon self-restart，已由 FR-041 缓解）。
