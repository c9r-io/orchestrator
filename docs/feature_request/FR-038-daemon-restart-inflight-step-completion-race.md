# FR-038: Daemon 重启时在途步骤竞态 — task_completed 提前发出与动态 Item 状态丢失

## 优先级

P1 — 影响任务完成后数据一致性与 QA 测试结果的可靠性

## 背景与问题

### 观测场景

在 `follow_task_logs 流式回调重构` self-bootstrap 执行期间（task `abe3af13`），daemon 在 Cycle 2 的 qa_testing 正在执行时发生了重启（PID 从 2550 变为 98643）。

#### 事件时间线

```
15:32:09  qa_testing step_started (item ac6843a5, 944631e8)  ← Worker A 启动
15:32:09  qa_testing agents spawned (PID 40074, 59892)
15:33:11  self_referential_policy_checked                     ← Worker B (新 daemon)
15:33:11  max_cycles_enforced (cycle=2, max=2)                ← Worker B
15:33:11  task_completed                                      ← Worker B
15:35:32  step_finished qa_testing item ac6843a5 (exit=0)     ← Worker A 的子进程
15:37:41  step_finished qa_testing item 944631e8 (exit=0)     ← Worker A 的子进程
```

`task_completed`（event ID 75306）在两个 qa_testing `step_finished`（event IDs 76158, 80301）之前写入数据库。

#### 影响

1. **task_completed 过早**: 任务被标记为 `completed`，但 qa_testing 尚未完成
2. **动态 Item 状态丢失**: 4 个 dynamic items 的 qa_testing 全部 exit=0，但最终状态仍为 `pending`（而非 `qa_passed`），因为 `finalize_items` 从未对其执行

### 根因分析

本问题包含两个相互独立但互相放大的 bug。

#### 根因 1: Daemon 重启导致在途步骤与新 worker 竞态

```
core/src/task_repository/state.rs:109  recover_orphaned_running_items()
core/src/persistence/repository/scheduler.rs:50  claim_next_pending_task()
core/src/scheduler/loop_engine/mod.rs:130  proactive max_cycles enforcement
```

1. Daemon 重启后，`recover_orphaned_running_items()` 全局扫描 `status='running'` 的 items，将它们重置为 `pending`，并将父 task 设为 `restart_pending`
2. 新 daemon 的 worker 通过 `claim_next_pending_task()` claim 了 `restart_pending` 的 task
3. 新 worker 进入 `run_task_loop`，在 cycle 开始前命中 `max_cycles_enforced`（cycle 2 >= max_cycles 2），直接 break
4. 循环结束后 `count_unresolved_items` 返回 0（items 被 recovery 重置为 `pending`，不算 unresolved），于是 task 被标记 `completed`
5. 旧 daemon 的 qa_testing agent 子进程仍在运行，它们最终完成并写入 `step_finished` 事件和 `command_runs` 记录，但无人执行 `finalize_items`

核心矛盾：`recover_orphaned_running_items()` 假设旧进程的所有工作都已丢失，但 agent 子进程可能以独立 PID 继续运行。

#### 根因 2: Workflow 缺少 finalize rules 导致动态 items 永远停留在 pending

```
core/src/scheduler/item_executor/finalize.rs:46-51     resolve_workflow_finalize_outcome()
core/src/prehook.rs:74                                  WorkflowFinalizeConfig (default: empty rules)
core/src/scheduler/item_executor/accumulator.rs:36-38   item_status 初始值 "pending"
core/src/task_repository/queries.rs:239                 count_unresolved_items() 只统计 'unresolved'/'qa_failed'
```

即使不发生 daemon 重启，动态 items 仍会停留在 `pending` 状态：

1. `StepExecutionAccumulator` 初始化时 `item_status = "pending"`（accumulator.rs:36）
2. `finalize_item_execution()`（finalize.rs:46-51）尝试调用 `resolve_workflow_finalize_outcome()` 来确定最终状态
3. 但 self-bootstrap workflow 的 `WorkflowFinalizeConfig` 没有配置 finalize rules（rules 为空 Vec）
4. `resolve_workflow_finalize_outcome()` 对空 rules 返回 `None`，`item_status` 保持 `"pending"` 不变
5. `count_unresolved_items()` 查询只统计 `status IN ('unresolved', 'qa_failed')`，`pending` 不在其中
6. 因此 task 即使所有 items 都停在 `pending` 状态，也会被判定为 `completed`

**此 bug 独立于根因 1**：即使 daemon 不重启，finalize 也无法将 items 从 `pending` 转为 `qa_passed`。

## 影响范围

- **self-bootstrap workflow**: daemon 可能因 self_restart exec() 重启，此时 Cycle 1 的 agent 进程仍可能存活
- **daemon 崩溃恢复**: crash 后 stale agent 子进程可能仍在写 stdout/DB
- **动态 items**: `items_generated replace=true` 生成的 dynamic items 特别容易受影响，因为它们是 qa_testing 的唯一目标

## 提议的解决方案

### 方案 A: Recovery 前等待 in-flight agents 完成（推荐）

在 `recover_orphaned_running_items()` 中，增加对 in-flight command_runs（`exit_code = -1`, `ended_at IS NULL`）的检测：

1. 查找所有 `exit_code = -1` 且 `pid IS NOT NULL` 的 command_runs
2. 检查对应 PID 是否仍存活（`kill(pid, 0)` 或 `/proc/pid/status`）
3. 如果存活，等待其完成（带超时），而非直接重置
4. 如果进程已死，正常执行 recovery

### 方案 B: Post-recovery finalize 补偿

在新 worker claim `restart_pending` task 进入 `run_task_loop` 后、在 `max_cycles_enforced` 检查前：

1. 扫描 `command_runs` 中 `ended_at IS NOT NULL` 但对应 `task_item.status = 'pending'` 的记录
2. 对这些 items 重新执行 `finalize_items` 逻辑，补偿丢失的状态转换
3. 这样即使 recovery 提前重置了 items，后续的 step_finished 写入也能被补偿

### 方案 C: 延迟 task_completed 直到无 in-flight runs

在 `run_task_loop` 的 post-loop 区域（`mod.rs:283-324`）：

1. 在 `count_unresolved_items` 之前，检查是否有 `exit_code = -1` 的 command_runs
2. 如果有，等待它们完成（带超时），然后重新执行 finalize
3. 最后再决定 `completed` vs `failed`

## 验证标准

1. Daemon 重启后，仍在运行的 qa_testing agent 完成后，其 item 状态正确更新为 `qa_passed`
2. `task_completed` 不在 qa_testing `step_finished` 之前发出
3. `command_runs` 中的 `ended_at` 与 `task_items` 中的终态一致
4. Recovery 不会等待已死进程（避免无限等待）

## 相关

- FR-033: Daemon 重启后孤立 Running Items 自动恢复（已实现，是本问题的前置功能）
- FR-032: Daemon 进程崩溃韧性与 Worker 存活保障
- `core/src/task_repository/state.rs:109` — `recover_orphaned_running_items()`
- `core/src/scheduler/loop_engine/mod.rs:130` — proactive max_cycles enforcement
- `core/src/scheduler/loop_engine/segment.rs:516` — `finalize_items()`
