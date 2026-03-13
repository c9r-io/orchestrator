# Design Doc 50: Daemon Restart In-Flight Step Completion Race Condition

**关联 FR**: FR-038
**状态**: Implemented
**日期**: 2026-03-14

---

## 1. 问题

Daemon 重启时，`recover_orphaned_running_items()` 将所有 `running` items 重置为 `pending`，但 agent 子进程可能仍以独立 PID 继续运行。新 worker claim `restart_pending` task 后命中 `max_cycles_enforced`，循环直接 break，`count_unresolved_items()` 不计入 `pending` items，task 被过早标记为 `completed`。

同时，即使 agent 子进程最终完成并写入 `step_finished` 事件，也无 `finalize_items()` 执行来将 items 从 `pending` 转为终态（如 `qa_passed`）。

### 两个独立根因

1. **竞态**: `recover_orphaned_running_items()` 假设旧进程工作已丢失，但 agent 子进程可能仍存活
2. **缺少 finalize 补偿**: recovery 后无机制重新执行 finalize

## 2. 设计决策

### 采用方案：三层防御 (Solutions B + C + Safety Net)

#### Layer 1: Wait for in-flight command runs (Solution C)

在 `run_task_loop_core` 的 post-loop 区域（task completion 判定前），插入 `wait_for_inflight_runs()`:

```rust
// core/src/scheduler/loop_engine/mod.rs
async fn wait_for_inflight_runs(state, task_id) -> Result<()>
```

- 查询 `command_runs` 中 `exit_code = -1` 且 `ended_at` 为空的记录
- 若存在，poll 等待（2s 间隔，120s 超时）
- 检查 PID 存活状态（`libc::kill(pid, 0)`），死进程立即跳过

#### Layer 2: Post-recovery finalize compensation (Solution B)

在 wait 完成后，执行 `compensate_pending_items()`:

```rust
async fn compensate_pending_items(state, task_id, task_ctx) -> Result<u32>
```

- 查询 `pending` items 关联的已完成 `command_runs`
- 从 DB 记录重建 `StepExecutionAccumulator`（exit_codes, step_ran, confidence）
- 调用 `finalize_item_execution()` 执行 CEL finalize rules
- 发出 `item_compensated` 事件

#### Layer 3: Stale pending items safety net

新增 `count_stale_pending_items()` 查询：统计 `pending` 状态、无 in-flight runs、但有已完成 runs 的 items。在 completion 判定中将其加入 unresolved 计数。

## 3. 关键变更

| 文件 | 变更 |
|------|------|
| `core/src/task_repository/write_ops.rs` | 新增 `find_inflight_command_runs_for_task()`, `find_completed_runs_for_pending_items()`, `CompletedRunRecord` |
| `core/src/task_repository/queries.rs` | 新增 `count_stale_pending_items()` |
| `core/src/task_repository/trait_def.rs` | 新增 trait 方法 |
| `core/src/task_repository/mod.rs` | trait impl + async wrapper |
| `core/src/db_write.rs` | async facade |
| `core/src/scheduler/task_state.rs` | 新增 wrapper 函数 |
| `core/src/scheduler/loop_engine/mod.rs` | `wait_for_inflight_runs()`, `compensate_pending_items()`, post-loop wiring |

## 4. 时序保证

```
[post-loop]
  ├─ wait_for_inflight_runs()     ← 等待旧子进程完成或超时
  ├─ compensate_pending_items()   ← 补偿已完成但未 finalize 的 items
  ├─ count_unresolved_items()     ← 标准 unresolved 计数
  ├─ count_stale_pending_items()  ← 安全网：仍卡在 pending 的 items
  └─ effective_unresolved = unresolved + stale_pending
      ├─ > 0 → task_failed
      └─ == 0 → task_completed
```

## 5. Accumulator 重建策略

从 `command_runs` 记录重建 `StepExecutionAccumulator` 时只填充可靠字段：

- `exit_codes`: phase → exit_code
- `step_ran`: phase → true
- `qa_confidence` / `qa_quality_score`: 从 qa_testing run 提取
- `flags["qa_failed"]`: qa exit_code != 0
- `flags["fix_success"]`: fix exit_code == 0
- `flags["retest_success"]`: retest exit_code == 0

未填充字段保持默认值，由 CEL finalize rules 的 fallback 规则（`fallback_qa_passed`）正确处理。
