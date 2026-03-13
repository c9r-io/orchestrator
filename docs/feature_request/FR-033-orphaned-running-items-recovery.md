# FR-033: Daemon 重启后孤立 Running Items 自动恢复

## 状态

Implemented

## 优先级

P1 — 直接导致 self-bootstrap 任务不可恢复，需人工干预

## 背景

### 问题发现

2026-03-13 执行 `follow-logs-callback-execution.md` 测试计划时，daemon 崩溃后重启，但以下 task items 永久停留在 `running` 状态：

```
a3286fae  running  order=103  docs/qa/orchestrator/02-cli-task-lifecycle.md  dynamic
87c088b2  running  order=104  docs/qa/orchestrator/53-client-server-architecture.md  dynamic
```

重启后的 daemon 无法恢复这些 items——它们既不会被重新执行，也不会被标记为失败。

### 根因分析

#### 1. `prepare_task_for_start_batch` 只重置 `unresolved`，不处理 `running`

`core/src/task_repository/state.rs:41-86`：

```rust
if matches!(status.as_deref(), Some("failed")) {
    tx.execute(
        "UPDATE task_items SET status='pending' ... WHERE task_id=?1 AND status='unresolved'",
        ...
    )?;
}
```

只有状态为 `unresolved` 的 items 会被重置为 `pending`。`running` 状态的 items 被完全忽略。

#### 2. Task 级别的 `running` 状态阻止重入

`state.rs:55-61`：

```rust
if matches!(status.as_deref(), Some("running")) {
    anyhow::bail!("task {} is already running — cannot start a second instance.");
}
```

如果 daemon 崩溃时 task 仍为 `running`，重启后的 daemon 会拒绝重新认领它（因为 `claim_next_pending_task` 只查 `pending` 或 `restart_pending` 状态的 task）。结果是 task 和 items 都卡在 `running` 状态，无人处理。

#### 3. 无 stall detection 机制

代码库中不存在以下恢复机制：
- 启动时扫描并重置孤立 running items
- 基于超时的 stall 检测（如 item running > 10 min 视为孤立）
- 基于 PID 存活检查的进程关联恢复

#### 4. Item loading 不过滤状态但不触发重新执行

`task_repository/queries.rs:246-267` 的 `list_task_items_for_cycle` 加载所有 items（不过滤 status），但 dispatch 逻辑依赖内存中的 `step_ran` 状态。daemon 重启后内存状态丢失，`running` items 不会被重新分发。

### 影响范围

- 任何 daemon 非正常退出（crash、OOM kill、SIGKILL）都会触发此问题
- `self_restart` exec() 失败后，如果同时有 items 在执行，也可能产生孤立 items
- 问题一旦发生，只能通过手工 SQL 修复：
  ```sql
  UPDATE task_items SET status='pending' WHERE task_id='...' AND status='running';
  UPDATE tasks SET status='pending' WHERE id='...' AND status='running';
  ```

## 需求

### 核心需求

**N1. 启动时孤立 item 恢复**：daemon 启动时扫描 `task_items` 表，将所有 `status='running'` 的 items 重置为 `pending`，并将关联 task 状态设置为 `restart_pending`。

**N2. 关联 task 状态修正**：如果 task 本身也停留在 `running` 状态（无 worker 持有），将其重置为 `restart_pending`，使其可被 worker 重新认领。

**N3. 恢复事件审计**：每次孤立 item 恢复时，emit `orphaned_items_recovered` 事件到 events 表：

```json
{
  "task_id": "803cbabb-...",
  "recovered_items": ["a3286fae", "87c088b2"],
  "previous_status": "running",
  "new_status": "pending"
}
```

### 辅助需求

**A1. 运行时 stall detection**：新增后台 sweep（与 event cleanup sweep 类似），周期性检查 `running` items 的 `started_at` 时间。如果 item 已 running 超过可配置阈值（默认 30 min），且对应的 agent 进程不存在，将其重置为 `pending` 并 emit `item_stall_recovered` 事件。

**A2. CLI 手动恢复命令**：新增 `orchestrator task recover <task_id>` 子命令，手动触发孤立 item 恢复，避免直接操作 SQL。

### 非目标

- 不实现跨 daemon 实例的分布式锁（单机 daemon 场景足够）
- 不变更 item 终态语义（`qa_passed`、`fixed` 等保持不变）
- 不引入心跳机制（agent 向 daemon 定期报告存活）

## 涉及文件

| 文件 | 变更类型 |
|------|---------|
| `core/src/task_repository/state.rs` | `prepare_task_for_start_batch` 新增 `running` items 重置逻辑 |
| `crates/daemon/src/main.rs` | 启动时调用孤立 item recovery |
| `core/src/task_repository/queries.rs` | 新增 `recover_orphaned_running_items` 查询 |
| `crates/daemon/src/main.rs` 或新文件 | Stall detection sweep 后台任务 |
| `crates/cli/src/commands/task.rs` | 新增 `recover` 子命令 |

## 验收标准

1. Daemon 启动时，所有无 worker 持有的 `running` items 被重置为 `pending`
2. 关联 task 从 `running` 变为 `restart_pending`，可被 worker 重新认领
3. Events 表出现 `orphaned_items_recovered` 事件，payload 包含受影响 item 列表
4. `orchestrator task recover <task_id>` 可手动触发恢复
5. 回归：正常运行的 `running` items 不受影响（只在启动时检查，或 stall detection 仅在超时后触发）
6. `cargo test --workspace --lib` 通过

## 复现步骤

```bash
# 1. 启动 daemon 并创建一个长运行任务
orchestrator task create -n test -w self -W self-bootstrap --project self-bootstrap -g "test"

# 2. 等待 items 进入 running 状态
sqlite3 data/agent_orchestrator.db "SELECT status FROM task_items WHERE task_id='<id>' AND status='running';"

# 3. 强制杀 daemon
kill -9 $(cat data/daemon.pid)

# 4. 确认 items 仍为 running
sqlite3 data/agent_orchestrator.db "SELECT id, status FROM task_items WHERE task_id='<id>' AND status='running';"

# 5. 重启 daemon
orchestratord --foreground --workers 2

# 预期（修复后）：daemon 日志显示 orphaned_items_recovered，items 被重置为 pending
# 当前行为：items 永久停留在 running 状态
```

## 参考

- 测试计划：`docs/plan/follow-logs-callback-execution.md`
- Task 启动逻辑：`core/src/task_repository/state.rs:41-86`
- Item 加载：`core/src/task_repository/queries.rs:246-267`
- Scheduler 认领：`core/src/persistence/repository/scheduler.rs:50-83`
- 相关 FR：FR-032（Daemon 崩溃韧性）— 减少崩溃发生频率
